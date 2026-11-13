pub mod auth;
pub mod client;
pub mod commands;
pub mod config;
pub mod session;
pub mod sync;
pub mod telemetry;
pub mod types;
pub mod update;

use std::sync::Arc;

use crate::infrastructure::SecretStore;

use self::auth::GoogleLoginRegistry;
use self::client::ApiClient;
use self::session::SessionStore;
use self::telemetry::TelemetryCollector;

/// 会员/遥测服务的聚合入口：注册到 `AppState`，命令层只与它交互。
pub struct AccountService {
    pub session: SessionStore,
    pub api: Arc<ApiClient>,
    pub google_registry: GoogleLoginRegistry,
    pub telemetry: Arc<TelemetryCollector>,
    device_id: String,
    sync_in_flight: std::sync::Mutex<bool>,
}

impl AccountService {
    pub fn new(store: Arc<dyn SecretStore>, device_id: String) -> Option<Self> {
        let base = config::api_base()?;
        println!("[account] service configured, api_base={base}");
        Some(Self {
            session: SessionStore::new(store),
            api: Arc::new(ApiClient::new(base.to_owned())),
            google_registry: GoogleLoginRegistry::default(),
            telemetry: Arc::new(TelemetryCollector::new(device_id.clone(), base.to_owned())),
            device_id,
            sync_in_flight: std::sync::Mutex::new(false),
        })
    }



    /// 登录态快照：本地有会话则向服务端校验；401 先 refresh 轮换；
    /// 冻结清除本地凭据；网络失败保持登录态并标记离线（设计 §3.3）。
    pub async fn status(&self) -> types::AccountStatus {
        let Some(tokens) = self.session.load() else {
            return types::AccountStatus {
                configured: true,
                logged_in: false,
                offline: false,
                frozen: None,
                profile: None,
            };
        };
        let fresh = match self.api.check(&tokens.access_token).await {
            Ok(profile) => Some(profile),
            Err(error) if error.code == "auth_expired" => match self
                .api
                .refresh(&tokens.refresh_token, &tokens.provider_type)
                .await
            {
                Ok(new_tokens) => {
                    let profile = new_tokens.profile.clone();
                    if let Err(save_error) = self.session.save(&new_tokens) {
                        log::warn!("session refresh persist failed: {}", save_error.message);
                    }
                    Some(profile)
                }
                Err(refresh_error) if refresh_error.code == "account_frozen" => {
                    self.session.clear();
                    return types::AccountStatus {
                        configured: true,
                        logged_in: false,
                        offline: false,
                        frozen: Some(true),
                        profile: None,
                    };
                }
                Err(_) => None,
            },
            Err(error) if error.code == "account_frozen" => {
                self.session.clear();
                return types::AccountStatus {
                    configured: true,
                    logged_in: false,
                    offline: false,
                    frozen: Some(true),
                    profile: None,
                };
            }
            Err(_) => None,
        };
        match fresh {
            Some(profile) => types::AccountStatus {
                configured: true,
                logged_in: true,
                offline: false,
                frozen: Some(false),
                profile: Some(profile),
            },
            None => types::AccountStatus {
                configured: true,
                logged_in: true,
                offline: true,
                frozen: None,
                profile: Some(tokens.profile),
            },
        }
    }

    /// 登录态下上报供应商元数据；并发去重、失败静默（设计 §3.4：不影响本地功能）。
    /// 供应商配置云存档上报：匿名（device_id 主体）或登录态（关联 user_id）均可；
    /// 失败静默（不影响本地功能）。内容变化由调用方（save/delete）触发。
    pub fn spawn_provider_sync(self: &Arc<Self>, providers: Vec<crate::domain::ProviderSummary>) {
        let token = self.session.load().map(|tokens| tokens.access_token);
        {
            let mut in_flight = self.sync_in_flight.lock().expect("sync guard poisoned");
            if *in_flight {
                return;
            }
            *in_flight = true;
        }
        let service = Arc::clone(self);
        let payload = serde_json::to_value(sync::build_payload(&providers))
            .unwrap_or(serde_json::Value::Null)
            .get("providers")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let os = std::env::consts::OS.to_owned();
        tauri::async_runtime::spawn(async move {
            // 新表：设备维度云存档（匿名可报，登录态关联 user_id）
            if let Err(error) = service
                .api
                .sync_provider_configs(
                    token.as_deref(),
                    &service.device_id,
                    &os,
                    &payload,
                )
                .await
            {
                log::warn!("provider config sync skipped: {}", error.message);
            }
            // 旧表：登录态同步 at_user_providers（后台 4.2 用户供应商管理）
            if let Some(token) = &token {
                if let Err(error) = service
                    .api
                    .sync_providers(
                        token,
                        &sync::build_payload(&providers),
                    )
                    .await
                {
                    log::warn!("provider metadata sync skipped: {}", error.message);
                }
            }
            *service.sync_in_flight.lock().expect("sync guard poisoned") = false;
        });
    }

    /// 登录成功后从云端拉取该账号全部设备的供应商配置，合入本地缺失的供应商。
    /// 已存在（name+baseUrl 匹配）的跳过；恢复的供应商不带 API Key，用户需重新填写后验证。
    pub fn spawn_cloud_restore(
        self: &Arc<Self>,
        providers_service: Arc<crate::services::ProviderService>,
    ) {
        let token = match self.session.load() {
            Some(tokens) => tokens.access_token,
            None => return,
        };
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let cloud = match service.api.fetch_provider_configs(&token).await {
                Ok(value) => value,
                Err(error) => {
                    log::warn!("cloud restore skipped: {}", error.message);
                    return;
                }
            };
            let Some(items) = cloud.as_array() else {
                return;
            };
            let existing = providers_service.list().unwrap_or_default();
            let mut restored = 0usize;
            for item in items {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let base_url = item.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() || base_url.is_empty() {
                    continue;
                }
                if existing
                    .iter()
                    .any(|p| p.name == name && p.base_url.trim_end_matches('/') == base_url.trim_end_matches('/'))
                {
                    continue;
                }
                let models: Vec<crate::domain::ModelDraft> = item
                    .get("models")
                    .and_then(|v| v.as_str())
                    .and_then(|json| serde_json::from_str::<Vec<serde_json::Value>>(json).ok())
                    .map(|list| {
                        list.iter()
                            .filter_map(|m| {
                                let model_id = m.get("modelId")?.as_str()?.to_owned();
                                Some(crate::domain::ModelDraft {
                                    display_name: m
                                        .get("displayName")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(&model_id)
                                        .to_owned(),
                                    model_id,
                                    output_modality: Default::default(),
                                    supports_streaming: true,
                                    supports_tools: false,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let draft = crate::domain::ProviderDraft {
                    id: None,
                    name: name.to_owned(),
                    kind: crate::domain::ProviderKind::Custom,
                    protocol: crate::domain::ApiProtocol::OpenaiChatCompletions,
                    base_url: base_url.to_owned(),
                    api_key: None,
                    default_model_id: None,
                    models,
                    allow_insecure_http: false,
                };
                if providers_service.save(draft).is_ok() {
                    restored += 1;
                }
            }
            if restored > 0 {
                println!("[account] cloud restore: {restored} providers recovered");
            }
        });
    }
}
