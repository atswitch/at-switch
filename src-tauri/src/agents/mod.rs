mod codebuddy;
mod codex;
mod ima;
mod lifecycle;
mod locator;
mod openclaw;
mod trae;
mod workbuddy;

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};

use crate::{
    domain::{
        AgentBindingDraft, AgentBindingMode, AgentConfigHealth, AgentInstallStatus,
        AgentRuntimeStatus, AgentSummary, ApiProtocol, AppResult, CommandError, ProviderSummary,
    },
    infrastructure::{Database, SecretStore, SecretValue, StoredAgentBinding},
    proxy::{ProxySupervisor, RouteSnapshot},
    services::{BaselineSnapshot, ConfigTransaction, FileChange},
};

use self::{
    codebuddy::CodeBuddyAdapter,
    codex::CodexAdapter,
    ima::ImaAdapter,
    lifecycle::RestartOutcome,
    locator::{normalized_path_string, DiscoveryContext, Installation, InstallationKind},
    openclaw::{AutoClawAdapter, QClawAdapter},
    trae::TraeAdapter,
    workbuddy::WorkBuddyAdapter,
};

type ConfigProbe = fn(&PathBuf) -> AppResult<()>;
type LocalTokenMetadata = (String, i64, bool);

struct WorkBuddySessionMutation {
    previous_session: Option<workbuddy::SessionSelection>,
    previous_new_task: workbuddy::NewTaskSelection,
    session_snapshot_created: bool,
    new_task_snapshot_created: bool,
}

struct CodeBuddyWorkspaceMutation {
    previous_workspaces: Vec<codebuddy::WorkspaceSelection>,
    previous_conversations: Vec<codebuddy::ConversationSelection>,
    snapshot_scope_ids: Vec<String>,
}

#[derive(Default)]
struct WorkBuddyRestorePlan {
    previous: Vec<workbuddy::SessionSelection>,
    changes: Vec<workbuddy::SessionModelChange>,
    previous_new_task: Option<workbuddy::NewTaskSelection>,
    restored_new_task_value: Option<Vec<u8>>,
}

#[derive(Default)]
struct CodeBuddyRestorePlan {
    previous_workspaces: Vec<codebuddy::WorkspaceSelection>,
    workspace_changes: Vec<codebuddy::WorkspaceSelection>,
    previous_conversations: Vec<codebuddy::ConversationSelection>,
    conversation_changes: Vec<codebuddy::ConversationSelection>,
}

#[derive(Debug, Clone)]
pub struct AgentDetection {
    id: &'static str,
    display_name: &'static str,
    installation: Option<Installation>,
    config_path: Option<PathBuf>,
    runtime_data_dir: Option<PathBuf>,
    install_status: AgentInstallStatus,
    config_health: AgentConfigHealth,
    write_supported: bool,
    needs_restart: bool,
    message: Option<String>,
    custom_install_path: Option<PathBuf>,
    using_custom_install_path: bool,
}

impl AgentDetection {
    fn from_file_probe(
        id: &'static str,
        display_name: &'static str,
        installation: Option<Installation>,
        config_path: PathBuf,
        probe: ConfigProbe,
        needs_restart: bool,
    ) -> Self {
        let Some(installation) = installation else {
            return Self {
                id,
                display_name,
                installation: None,
                config_path: Some(config_path),
                runtime_data_dir: None,
                install_status: AgentInstallStatus::NotInstalled,
                config_health: AgentConfigHealth::UnsupportedVersion,
                write_supported: false,
                needs_restart: false,
                custom_install_path: None,
                using_custom_install_path: false,
                message: Some(format!(
                    "未在标准目录、系统应用索引、运行进程或 PATH 中检测到 {display_name}"
                )),
            };
        };

        let (install_status, config_health, write_supported, message) = if config_path.exists() {
            match probe(&config_path) {
                Ok(()) if writable_target(&config_path) => (
                    AgentInstallStatus::Installed,
                    AgentConfigHealth::Healthy,
                    true,
                    Some(format!(
                        "{display_name} 安装与配置已识别；AT-Switch 会在切换后读取实际配置进行校验。"
                    )),
                ),
                Ok(()) => (
                    AgentInstallStatus::Installed,
                    AgentConfigHealth::Unwritable,
                    false,
                    Some(format!(
                        "{display_name} 配置已识别，但当前用户没有写入权限"
                    )),
                ),
                Err(error) => {
                    let health = match error.code.as_str() {
                        "agent_config_unparseable" => AgentConfigHealth::Unparseable,
                        "agent_config_shape_unsupported" => AgentConfigHealth::UnsupportedVersion,
                        _ => AgentConfigHealth::Unreadable,
                    };
                    (
                        AgentInstallStatus::Installed,
                        health,
                        false,
                        Some(error.message),
                    )
                }
            }
        } else if writable_target(&config_path) {
            (
                AgentInstallStatus::InstalledUninitialized,
                AgentConfigHealth::Healthy,
                true,
                Some(format!(
                    "{display_name} 已安装；首次切换时将创建并校验标准配置文件"
                )),
            )
        } else {
            (
                AgentInstallStatus::InstalledUninitialized,
                AgentConfigHealth::Unwritable,
                false,
                Some(format!("{display_name} 已安装，但无法创建配置文件")),
            )
        };

        Self {
            id,
            display_name,
            installation: Some(installation),
            config_path: Some(config_path),
            runtime_data_dir: None,
            install_status,
            config_health,
            write_supported,
            needs_restart,
            message,
            custom_install_path: None,
            using_custom_install_path: false,
        }
    }

    fn manual(
        id: &'static str,
        display_name: &'static str,
        installation: Option<Installation>,
        message: &str,
    ) -> Self {
        let installed = installation.is_some();
        Self {
            id,
            display_name,
            installation,
            config_path: None,
            runtime_data_dir: None,
            install_status: if installed {
                AgentInstallStatus::Installed
            } else {
                AgentInstallStatus::NotInstalled
            },
            config_health: AgentConfigHealth::UnsupportedVersion,
            write_supported: false,
            needs_restart: false,
            custom_install_path: None,
            using_custom_install_path: false,
            message: Some(if installed {
                message.to_owned()
            } else {
                format!("未在系统标准安装位置检测到 {display_name}")
            }),
        }
    }

    fn summary(&self) -> AgentSummary {
        AgentSummary {
            id: self.id.to_owned(),
            display_name: self.display_name.to_owned(),
            install_status: self.install_status,
            runtime_status: self
                .installation
                .as_ref()
                .map(|installation| lifecycle::runtime_status(installation, self.display_name))
                .unwrap_or(AgentRuntimeStatus::Unknown),
            config_health: self.config_health,
            adapter_verified: self.write_supported,
            detected_version: self
                .installation
                .as_ref()
                .and_then(|installation| installation.version.clone()),
            is_latest_version: self.installation.is_some(),
            install_path: self
                .installation
                .as_ref()
                .map(|installation| normalized_path_string(&installation.path)),
            custom_install_path: self
                .custom_install_path
                .as_ref()
                .map(|path| normalized_path_string(path)),
            using_custom_install_path: self.using_custom_install_path,
            config_path: self
                .config_path
                .as_ref()
                .map(|path| normalized_path_string(path)),
            provider_name: None,
            provider_id: None,
            model_id: None,
            mode: None,
            needs_restart: self.needs_restart,
            automatic_restart_supported: self.needs_restart
                && self
                    .installation
                    .as_ref()
                    .is_some_and(|installation| installation.kind == InstallationKind::DesktopApp),
            activation_required: false,
            message: self.message.clone(),
        }
    }
}

pub struct DesiredAgentBinding<'a> {
    pub mode: AgentBindingMode,
    pub provider_name: &'a str,
    pub model_id: &'a str,
    pub supports_tools: bool,
    pub upstream_protocol: ApiProtocol,
    pub source_protocol: ApiProtocol,
    pub base_url: &'a str,
    pub credential: &'a str,
}

trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn detect(&self, context: &DiscoveryContext) -> AgentDetection;
    fn source_protocol(
        &self,
        desired_mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol;
    fn validate_binding(&self, _desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        Ok(())
    }
    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>>;
    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>>;
    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()>;
    fn activation_required(&self, _detection: &AgentDetection) -> bool {
        false
    }
    fn native_activation_required(&self, _detection: &AgentDetection) -> bool {
        false
    }
}

fn matched_binding_protocols(
    adapter: &dyn AgentAdapter,
    mode: AgentBindingMode,
    provider: &ProviderSummary,
) -> (ApiProtocol, ApiProtocol) {
    // Proxy adapters declare the protocol spoken by the Agent. Direct-capable
    // adapters may instead mirror the selected upstream protocol, so resolve
    // twice to reach a stable source/upstream pair without Agent-specific
    // routing branches.
    let initial_source = adapter.source_protocol(mode, provider.protocol);
    let initial_upstream = provider.upstream_protocol_for(initial_source);
    let source = adapter.source_protocol(mode, initial_upstream);
    let upstream = provider.upstream_protocol_for(source);
    (source, upstream)
}

struct AgentRegistry {
    adapters: Vec<Box<dyn AgentAdapter>>,
    context: DiscoveryContext,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Box::new(WorkBuddyAdapter),
                Box::new(CodeBuddyAdapter),
                Box::new(QClawAdapter),
                Box::new(ImaAdapter),
                Box::new(AutoClawAdapter),
                Box::new(TraeAdapter),
                Box::new(CodexAdapter),
            ],
            context: DiscoveryContext::native(),
        }
    }
}

impl AgentRegistry {
    fn adapter(&self, id: &str) -> AppResult<&dyn AgentAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.id() == id)
            .map(|adapter| adapter.as_ref())
            .ok_or_else(|| CommandError::new("agent_not_supported", "Agent 不在首版支持清单中"))
    }

    #[cfg(all(test, target_os = "windows"))]
    fn detections(&self) -> Vec<AgentDetection> {
        self.detections_with_custom_paths(&HashMap::new())
    }

    fn detections_with_custom_paths(
        &self,
        custom_paths: &HashMap<String, String>,
    ) -> Vec<AgentDetection> {
        self.adapters
            .iter()
            .map(|adapter| {
                let custom_path = custom_paths.get(adapter.id()).map(PathBuf::from);
                self.detect_adapter(adapter.as_ref(), custom_path.as_deref())
            })
            .collect()
    }

    fn detect_adapter(
        &self,
        adapter: &dyn AgentAdapter,
        custom_path: Option<&std::path::Path>,
    ) -> AgentDetection {
        let mut context = self.context.refreshed();
        context.custom_installation_path = custom_path.map(PathBuf::from);
        let mut detection = adapter.detect(&context);
        detection.custom_install_path = custom_path.map(PathBuf::from);
        detection.using_custom_install_path = custom_path.is_some_and(|selected| {
            detection.installation.as_ref().is_some_and(|installation| {
                installation_matches_custom_selection(selected, &installation.path)
            })
        });
        if custom_path.is_some()
            && !detection.using_custom_install_path
            && detection.installation.is_none()
        {
            detection.message = Some(format!(
                "{} 的自定义安装位置已失效；请选择包含应用程序的目录，或恢复自动发现。",
                detection.display_name
            ));
        }
        detection
    }
}

fn installation_matches_custom_selection(
    selected: &std::path::Path,
    detected: &std::path::Path,
) -> bool {
    let selected = fs::canonicalize(selected).unwrap_or_else(|_| selected.to_path_buf());
    let detected = fs::canonicalize(detected).unwrap_or_else(|_| detected.to_path_buf());
    if selected.is_file() {
        detected == selected
    } else {
        detected.starts_with(selected)
    }
}

pub struct AgentService {
    registry: AgentRegistry,
    database: Arc<Database>,
    secret_store: Arc<dyn SecretStore>,
    transaction: ConfigTransaction,
    proxy: Arc<ProxySupervisor>,
    proxy_routes_restored: AtomicBool,
}

impl AgentService {
    pub fn new(
        database: Arc<Database>,
        secret_store: Arc<dyn SecretStore>,
        backup_root: PathBuf,
        proxy: Arc<ProxySupervisor>,
    ) -> Self {
        Self {
            registry: AgentRegistry::default(),
            database,
            transaction: ConfigTransaction::new(Arc::clone(&secret_store), backup_root),
            secret_store,
            proxy,
            proxy_routes_restored: AtomicBool::new(false),
        }
    }

    pub fn scan(&self) -> AppResult<Vec<AgentSummary>> {
        let custom_paths = self.database.custom_agent_install_paths()?;
        let bindings = self
            .database
            .list_agent_bindings()?
            .into_iter()
            .map(|binding| (binding.agent_id.clone(), binding))
            .collect::<HashMap<_, _>>();
        self.registry
            .detections_with_custom_paths(&custom_paths)
            .into_iter()
            .map(|detection| {
                let mut summary = detection.summary();
                if let Some(binding) = bindings.get(&summary.id) {
                    enrich_summary(&self.database, &mut summary, binding);
                    self.enrich_binding_health(&mut summary, binding);
                    if matches!(summary.config_health, AgentConfigHealth::Healthy) {
                        if let Ok(adapter) = self.registry.adapter(&summary.id) {
                            if let Err(error) =
                                self.verify_stored_binding(adapter, &detection, binding)
                            {
                                summary.config_health = AgentConfigHealth::ExternalChanged;
                                summary.message = Some(format!(
                                    "{} 的实际配置与 AT-Switch 记录不一致：{}。请重新点击目标模型的“切换”，AT-Switch 会重新写入并校验。",
                                    summary.display_name, error.message,
                                ));
                            } else {
                                summary.message = Some(format!(
                                    "{} 当前配置与 AT-Switch 记录一致；可直接切换 Provider、模型和接入方式。",
                                    summary.display_name
                                ));
                            }
                        }
                    }
                }
                if summary.id == "workbuddy"
                    && matches!(summary.config_health, AgentConfigHealth::Healthy)
                {
                    if let Ok(adapter) = self.registry.adapter(&summary.id) {
                        if bindings.contains_key(&summary.id) {
                            let new_task_snapshot_ready = self
                                .database
                                .list_runtime_selections("workbuddy")?
                                .iter()
                                .any(|selection| {
                                    selection.scope_id.starts_with("new-task:")
                                });
                            summary.activation_required =
                                adapter.activation_required(&detection)
                                    || !new_task_snapshot_ready;
                            summary.message = Some(
                                if summary.activation_required {
                                    "WorkBuddy 的当前会话或新会话模型尚未完成同步。点击目标模型的“切换”即可修复；无需进入 WorkBuddy 模型菜单。"
                                } else {
                                    "当前 WorkBuddy 会话已接入 AT-Switch；重新切换时会同步当前会话和新会话默认模型。"
                                }
                                    .to_owned(),
                            );
                        } else {
                            summary.activation_required =
                                adapter.native_activation_required(&detection);
                            if summary.activation_required {
                                summary.message = Some(
                                    "WorkBuddy 会话仍引用已移除的 AT-Switch 入口。请再次点击“默认配置”，AT-Switch 会自动恢复会话原模型。"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                }
                self.database.upsert_agent_state(&summary)?;
                Ok(summary)
            })
            .collect()
    }

    fn detect_agent(&self, adapter: &dyn AgentAdapter) -> AppResult<AgentDetection> {
        let custom_paths = self.database.custom_agent_install_paths()?;
        Ok(self.registry.detect_adapter(
            adapter,
            custom_paths.get(adapter.id()).map(std::path::Path::new),
        ))
    }

    pub fn set_custom_install_path(
        &self,
        agent_id: &str,
        path: Option<&str>,
    ) -> AppResult<AgentSummary> {
        if !matches!(std::env::consts::OS, "macos" | "windows") {
            return Err(CommandError::new(
                "custom_install_path_unsupported",
                "自定义安装位置仅支持 macOS 和 Windows。",
            ));
        }
        if !matches!(
            agent_id,
            "workbuddy" | "codebuddy" | "qclaw" | "autoclaw" | "codex"
        ) {
            return Err(CommandError::new(
                "custom_install_path_agent_unsupported",
                "该 Agent 不支持选择自定义安装位置。",
            ));
        }
        let adapter = self.registry.adapter(agent_id)?;
        let normalized = path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        if let Some(selected) = normalized.as_deref() {
            if !selected.is_absolute() || (!selected.is_file() && !selected.is_dir()) {
                return Err(CommandError::new(
                    "custom_install_path_invalid",
                    "选择的安装位置不存在或不是绝对路径。",
                )
                .with_recovery("请选择 Agent 主程序或其所在目录后重试。"));
            }
            let detection = self.registry.detect_adapter(adapter, Some(selected));
            if !detection.using_custom_install_path {
                return Err(CommandError::new(
                    "custom_installation_not_found",
                    format!(
                        "所选目录中没有找到有效的 {} 应用程序。",
                        adapter.display_name()
                    ),
                )
                .with_recovery(
                    "请选择 Agent 主程序或包含它的目录；AT-Switch 最多检查所选目录下一层。",
                ));
            }
        }
        let normalized = normalized.as_deref().map(normalized_path_string);
        self.database
            .set_custom_agent_install_path(agent_id, normalized.as_deref())?;
        self.scan()?
            .into_iter()
            .find(|agent| agent.id == agent_id)
            .ok_or_else(|| CommandError::internal("保存安装位置后无法读取 Agent 状态"))
    }

    pub async fn restore_proxy_routes(&self) -> AppResult<()> {
        if self.proxy_routes_restored.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self.restore_proxy_routes_once().await;
        if result.is_err() {
            self.proxy_routes_restored.store(false, Ordering::Release);
        }
        result
    }

    async fn restore_proxy_routes_once(&self) -> AppResult<()> {
        for binding in self.database.list_agent_bindings()? {
            if binding.mode != AgentBindingMode::Proxy.as_str() {
                continue;
            }
            let Some(local_ref) = binding.local_token_ref.as_deref() else {
                continue;
            };
            let Ok(local_token) = self.secret_store.get(local_ref) else {
                continue;
            };
            let Ok(provider) = self.database.get_provider(&binding.provider_id) else {
                continue;
            };
            let Some(upstream_ref) = provider.api_key_ref else {
                continue;
            };
            // Warm the process-local secret cache once on application start.
            // Proxy requests can then run without repeated Keychain prompts.
            if self.secret_store.get(&upstream_ref).is_err() {
                continue;
            }
            self.proxy
                .routes()
                .register(
                    local_token.expose(),
                    RouteSnapshot {
                        agent_id: binding.agent_id,
                        source_protocol: binding.request_protocol,
                        upstream_protocol: provider
                            .summary
                            .upstream_protocol_for(binding.request_protocol),
                        upstream_base_url: provider.summary.base_url,
                        upstream_model: binding.default_model_id,
                        upstream_api_key_ref: upstream_ref,
                    },
                )
                .await;
        }
        Ok(())
    }

    pub async fn apply(&self, draft: AgentBindingDraft) -> AppResult<AgentSummary> {
        let adapter = self.registry.adapter(&draft.agent_id)?;
        let detection = self.detect_agent(adapter)?;
        if detection.install_status == AgentInstallStatus::NotInstalled {
            return Err(CommandError::new("agent_not_installed", "未检测到该 Agent"));
        }
        if !detection.write_supported {
            return Err(CommandError::new(
                "agent_adapter_read_only",
                detection
                    .message
                    .clone()
                    .unwrap_or_else(|| "该 Agent 当前保持只读".to_owned()),
            ));
        }
        let config_path = detection.config_path.clone().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "Agent 配置路径不可用")
        })?;
        self.database.upsert_agent_state(&detection.summary())?;
        let previous_binding = self.database.get_agent_binding(&draft.agent_id)?;
        let provider = self.database.get_provider(&draft.provider_id)?;
        let model = provider
            .summary
            .models
            .iter()
            .find(|model| model.model_id == draft.model_id)
            .ok_or_else(|| CommandError::new("model_not_found", "所选模型不属于该 Provider"))?;
        let upstream_ref = provider
            .api_key_ref
            .as_deref()
            .ok_or_else(|| CommandError::new("secret_missing", "Provider 的 API Key 不存在"))?;
        let (source_protocol, upstream_protocol) =
            matched_binding_protocols(adapter, draft.mode, &provider.summary);
        let proxy_port = self.database.proxy_port()?;

        let (credential, local_token_metadata) = match draft.mode {
            AgentBindingMode::Direct => (self.secret_store.get(upstream_ref)?, None),
            AgentBindingMode::Proxy => {
                let (secret, reference, revision, created) =
                    self.load_or_create_local_token(&draft.agent_id)?;
                (secret, Some((reference, revision, created)))
            }
        };
        let local_base_url = format!("http://127.0.0.1:{proxy_port}/v1");
        let desired = DesiredAgentBinding {
            mode: draft.mode,
            provider_name: &provider.summary.name,
            model_id: &model.model_id,
            supports_tools: model.supports_tools,
            upstream_protocol,
            source_protocol,
            base_url: if draft.mode == AgentBindingMode::Proxy {
                &local_base_url
            } else {
                &provider.summary.base_url
            },
            credential: credential.expose(),
        };
        if let Err(error) = adapter.validate_binding(&desired) {
            self.cleanup_new_local_token(&local_token_metadata);
            return Err(error);
        }
        // 退出失败不再阻塞配置写入。如果 Agent 退不掉（权限、AV 拦截、托盘常驻），
        // 仍然继续写入配置——配置写入成功即视为切换成功，只是不能自动重启，
        // 需要用户手动重启 Agent。配置写入失败仍然回滚并恢复原运行状态，
        // 但单独的退出失败不会触发回滚。
        let mut agent_stop_failed_message: Option<String> = None;
        let workbuddy_pause = if draft.agent_id == "workbuddy" {
            match workbuddy::pause_for_runtime_update(&detection) {
                Ok(pause) => Some(pause),
                Err(error) => {
                    log::warn!(
                        "WorkBuddy could not be stopped before configuration write: {}; \
                         continuing with configuration write anyway",
                        error.message
                    );
                    agent_stop_failed_message = Some(format!(
                        "WorkBuddy 仍在运行，未能自动退出。{}",
                        error.recovery.as_deref().unwrap_or("")
                    ));
                    None
                }
            }
        } else {
            None
        };
        let desktop_pause = if detection.needs_restart && draft.agent_id != "workbuddy" {
            match lifecycle::pause_for_config_update(&detection) {
                Ok(pause) => Some(pause),
                Err(error) => {
                    log::warn!(
                        "{} could not be stopped before configuration write: {}; \
                         continuing with configuration write anyway",
                        draft.agent_id,
                        error.message
                    );
                    agent_stop_failed_message = Some(format!(
                        "{} 仍在运行，未能自动退出。",
                        detection.display_name
                    ));
                    None
                }
            }
        } else {
            None
        };
        let new_content = match adapter.build_config(&detection, &desired) {
            Ok(content) => content,
            Err(error) => {
                self.cleanup_new_local_token(&local_token_metadata);
                return Err(error);
            }
        };
        if let Err(error) = self.transaction.baseline(&draft.agent_id, &config_path) {
            self.cleanup_new_local_token(&local_token_metadata);
            return Err(error);
        }
        let result = match self.transaction.apply_file(
            &draft.agent_id,
            FileChange {
                path: config_path,
                new_content,
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.cleanup_new_local_token(&local_token_metadata);
                return Err(error);
            }
        };
        // verify_config 自己读 config_path 上的最新文件内容，不依赖 detect 的
        // 副产物（installation/config_health 等都不会在写文件后变化），所以
        // 直接复用上一次 detection，避免再次跑 locate_desktop_app 的全平台扫描。
        // 这是 apply 流程里最大的一笔性能节省——每次 detect 在 Windows 上
        // 要枚举全系统进程表 + 查注册表 + 扫 PATH，单次 1~3 秒。
        if let Err(error) = adapter.verify_config(&detection, &desired) {
            let restore_result = self.transaction.restore_backup(&result.backup_path);
            self.cleanup_new_local_token(&local_token_metadata);
            return match restore_result {
                Ok(()) => Err(CommandError::new(
                    "agent_apply_verification_failed",
                    format!("Agent 写入后校验失败，已恢复原配置：{}", error.message),
                )),
                Err(_) => Err(CommandError::new(
                    "agent_apply_manual_recovery_required",
                    "Agent 写入后校验失败且自动恢复失败，需要人工处理",
                )),
            };
        }

        let workbuddy_session_mutation = if draft.agent_id == "workbuddy" {
            match self.activate_workbuddy_session_while_paused(&detection) {
                Ok(mutation) => Some(mutation),
                Err(error) => {
                    let _ = self.transaction.restore_backup(&result.backup_path);
                    self.cleanup_new_local_token(&local_token_metadata);
                    return Err(error);
                }
            }
        } else {
            None
        };
        let codebuddy_workspace_mutation = if draft.agent_id == "codebuddy" {
            let selection_id =
                codebuddy::model_selection_id(Some(desired.provider_name), desired.model_id);
            match self.activate_codebuddy_workspaces_while_paused(&detection, &selection_id) {
                Ok(mutation) => Some(mutation),
                Err(error) => {
                    let _ = self.transaction.restore_backup(&result.backup_path);
                    self.cleanup_new_local_token(&local_token_metadata);
                    return Err(error);
                }
            }
        } else {
            None
        };

        let (local_ref, local_revision) = local_token_metadata
            .as_ref()
            .map(|(reference, revision, _)| (Some(reference.as_str()), *revision))
            .unwrap_or((None, 0));
        let next_binding = StoredAgentBinding {
            agent_id: draft.agent_id.clone(),
            mode: draft.mode.as_str().to_owned(),
            provider_id: draft.provider_id.clone(),
            default_model_id: draft.model_id.clone(),
            request_protocol: source_protocol,
            local_token_ref: local_ref.map(ToOwned::to_owned),
            local_token_revision: local_revision,
        };
        if let Err(error) = self.database.save_agent_binding(&next_binding) {
            let _ = self.transaction.restore_backup(&result.backup_path);
            self.rollback_workbuddy_session(&detection, workbuddy_session_mutation.as_ref());
            self.rollback_codebuddy_workspaces(codebuddy_workspace_mutation.as_ref());
            self.cleanup_new_local_token(&local_token_metadata);
            return Err(error);
        }

        // Commit the process-local route only after both the Agent file and
        // database binding are durable. Until this point an existing proxy
        // route remains untouched, so a failed direct/proxy transition cannot
        // silently break the Agent's previously working route.
        if draft.mode == AgentBindingMode::Proxy {
            self.proxy
                .routes()
                .register(
                    credential.expose(),
                    RouteSnapshot {
                        agent_id: draft.agent_id.clone(),
                        source_protocol,
                        upstream_protocol,
                        upstream_base_url: provider.summary.base_url.clone(),
                        upstream_model: model.model_id.clone(),
                        upstream_api_key_ref: upstream_ref.to_owned(),
                    },
                )
                .await;
        }
        log::info!(
            "agent configuration applied: agent={}, operation={}, sha256={}",
            draft.agent_id,
            result.operation_id,
            result.final_sha256
        );

        if draft.mode == AgentBindingMode::Direct {
            if let Some(previous) = previous_binding {
                if let Some(reference) = previous.local_token_ref {
                    if let Ok(token) = self.secret_store.get(&reference) {
                        self.proxy.routes().unregister(token.expose()).await;
                    }
                    if let Err(error) = self.secret_store.delete(&reference) {
                        log::warn!(
                            "unable to remove retired local routing token for {}: {}",
                            draft.agent_id,
                            error.message
                        );
                    }
                }
            }
        }
        let workbuddy_relaunch_failed = workbuddy_pause
            .map(|pause| {
                pause.resume().is_err_and(|error| {
                    log::warn!(
                        "WorkBuddy configuration was applied but relaunch failed: {}",
                        error.message
                    );
                    true
                })
            })
            .unwrap_or(false);
        let desktop_restart = desktop_pause.map(|pause| pause.resume());
        let refreshed_detection = self.detect_agent(adapter)?;
        let mut summary = refreshed_detection.summary();
        enrich_summary(&self.database, &mut summary, &next_binding);
        self.enrich_binding_health(&mut summary, &next_binding);
        // 退出失败或重启失败都属于「配置已写入，需要手动重启」。
        // 这两种情况都不影响 binding 的有效性，只影响 Agent 是否立即可用。
        let needs_manual_restart = agent_stop_failed_message.is_some() || workbuddy_relaunch_failed;
        if draft.agent_id == "workbuddy" {
            summary.activation_required = adapter.activation_required(&refreshed_detection);
            summary.needs_restart = needs_manual_restart;
            summary.message = Some(if needs_manual_restart {
                "WorkBuddy 当前会话和新会话默认模型均已切换。配置已写入，重启 WorkBuddy 后即可使用新模型。".to_owned()
            } else {
                "WorkBuddy 当前会话和新会话默认模型均已切换；若切换时正在运行，AT-Switch 已自动重新打开。"
                    .to_owned()
            });
        } else if draft.agent_id == "codebuddy" {
            apply_restart_outcome(&mut summary, desktop_restart);
            let restart_message = summary.message.take().unwrap_or_default();
            summary.message = Some(if agent_stop_failed_message.is_some() {
                "CodeBuddy CN 模型目录、工作区默认值和当前会话模型均已同步。配置已写入，重启 CodeBuddy 后即可使用新模型。".to_owned()
            } else {
                format!(
                    "CodeBuddy CN 模型目录、工作区默认值和当前会话模型均已同步；{restart_message}"
                )
            });
            summary.needs_restart = needs_manual_restart;
        } else if detection.needs_restart {
            if agent_stop_failed_message.is_some() {
                summary.needs_restart = true;
                summary.message = Some(format!(
                    "配置已写入。重启 {} 后即可使用新模型。",
                    summary.display_name
                ));
            } else {
                apply_restart_outcome(&mut summary, desktop_restart);
            }
        }
        Ok(summary)
    }

    pub async fn restore_native(&self, agent_id: &str) -> AppResult<AgentSummary> {
        let adapter = self.registry.adapter(agent_id)?;
        let detection = self.detect_agent(adapter)?;
        if detection.install_status == AgentInstallStatus::NotInstalled {
            return Err(CommandError::new("agent_not_installed", "未检测到该 Agent"));
        }
        if !detection.write_supported {
            return Err(CommandError::new(
                "agent_adapter_read_only",
                detection
                    .message
                    .clone()
                    .unwrap_or_else(|| "该 Agent 当前保持只读".to_owned()),
            ));
        }
        let config_path = detection.config_path.clone().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "Agent 配置路径不可用")
        })?;
        let previous_binding = self.database.get_agent_binding(agent_id)?;
        // 与 apply_agent_binding 一致：退出失败不阻塞恢复写入。
        let mut agent_stop_failed_message: Option<String> = None;
        let workbuddy_pause = if agent_id == "workbuddy" {
            match workbuddy::pause_for_runtime_update(&detection) {
                Ok(pause) => Some(pause),
                Err(error) => {
                    log::warn!(
                        "WorkBuddy could not be stopped before native restore: {}; \
                         continuing with restore anyway",
                        error.message
                    );
                    agent_stop_failed_message = Some(format!(
                        "WorkBuddy 仍在运行，未能自动退出。{}",
                        error.recovery.as_deref().unwrap_or("")
                    ));
                    None
                }
            }
        } else {
            None
        };
        let desktop_pause = if detection.needs_restart && agent_id != "workbuddy" {
            match lifecycle::pause_for_config_update(&detection) {
                Ok(pause) => Some(pause),
                Err(error) => {
                    log::warn!(
                        "{} could not be stopped before native restore: {}; \
                         continuing with restore anyway",
                        agent_id,
                        error.message
                    );
                    agent_stop_failed_message = Some(format!(
                        "{} 仍在运行，未能自动退出。",
                        detection.display_name
                    ));
                    None
                }
            }
        } else {
            None
        };
        let baseline = self.transaction.baseline(agent_id, &config_path)?;
        let native_content = adapter.build_native_config(&detection, &baseline)?;
        let workbuddy_restore = if agent_id == "workbuddy" {
            self.plan_workbuddy_restore(&detection)?
        } else {
            WorkBuddyRestorePlan::default()
        };
        let codebuddy_restore = if agent_id == "codebuddy" {
            self.plan_codebuddy_restore(&detection)?
        } else {
            CodeBuddyRestorePlan::default()
        };
        let result = self.transaction.apply_file(
            agent_id,
            FileChange {
                path: config_path,
                new_content: native_content,
            },
        )?;

        if let Err(error) = workbuddy::apply_session_changes(&detection, &workbuddy_restore.changes)
        {
            let _ = self.transaction.restore_backup(&result.backup_path);
            return Err(error);
        }
        if let Some(selection) = &workbuddy_restore.previous_new_task {
            if let Err(error) = workbuddy::apply_new_task_selection(
                &detection,
                &selection.user_id,
                workbuddy_restore.restored_new_task_value.as_deref(),
            ) {
                let _ = self.transaction.restore_backup(&result.backup_path);
                self.rollback_workbuddy_restore(&detection, &workbuddy_restore);
                return Err(error);
            }
        }
        // 恢复原始配置时清除全局 state.vscdb 里 AT-Switch 写入的
        // chatSelectedModelGlobalMap，让 CodeBuddy 回到自己的默认选中。
        if agent_id == "codebuddy" {
            let mode = codebuddy_restore
                .previous_workspaces
                .first()
                .map(|w| w.selected_mode.as_str())
                .unwrap_or("craft");
            if let Err(error) = codebuddy::clear_global_model_selection(&detection, mode) {
                log::warn!(
                    "CodeBuddy global model selection could not be cleared: {}",
                    error.message
                );
            }
        }
        if let Err(error) =
            codebuddy::apply_workspace_selections(&codebuddy_restore.workspace_changes)
        {
            let _ = self.transaction.restore_backup(&result.backup_path);
            self.rollback_workbuddy_restore(&detection, &workbuddy_restore);
            self.rollback_codebuddy_restore(&codebuddy_restore);
            return Err(error);
        }
        if let Err(error) =
            codebuddy::apply_conversation_selections(&codebuddy_restore.conversation_changes)
        {
            let _ = self.transaction.restore_backup(&result.backup_path);
            self.rollback_workbuddy_restore(&detection, &workbuddy_restore);
            self.rollback_codebuddy_restore(&codebuddy_restore);
            return Err(error);
        }

        if let Err(error) = self
            .database
            .delete_agent_binding_and_runtime_selections(agent_id)
        {
            let _ = self.transaction.restore_backup(&result.backup_path);
            self.rollback_workbuddy_restore(&detection, &workbuddy_restore);
            self.rollback_codebuddy_restore(&codebuddy_restore);
            return Err(error);
        }

        if let Some(binding) = previous_binding {
            if let Some(reference) = binding.local_token_ref {
                if let Ok(token) = self.secret_store.get(&reference) {
                    self.proxy.routes().unregister(token.expose()).await;
                }
                if let Err(error) = self.secret_store.delete(&reference) {
                    log::warn!(
                        "unable to remove retired local routing token for {}: {}",
                        agent_id,
                        error.message
                    );
                }
            }
        }

        let workbuddy_relaunch_failed = workbuddy_pause
            .map(|pause| {
                pause.resume().is_err_and(|error| {
                    log::warn!(
                        "WorkBuddy native configuration was restored but relaunch failed: {}",
                        error.message
                    );
                    true
                })
            })
            .unwrap_or(false);
        let desktop_restart = desktop_pause.map(|pause| pause.resume());
        // workbuddy 的 activation_required 依赖 session DB 状态——session DB
        // 在 restore_native 期间被改写过，所以 workbuddy 必须重新 probe。
        // 其它 agent 的 installation/config_path 在 restore 期间没变，复用
        // detection 即可，避免一次全平台扫描。
        let restored_detection = if adapter.id() == "workbuddy" {
            self.detect_agent(adapter)?
        } else {
            detection.clone()
        };
        let mut summary = restored_detection.summary();
        let needs_manual_restart = agent_stop_failed_message.is_some() || workbuddy_relaunch_failed;
        if adapter.id() == "workbuddy" {
            summary.activation_required = adapter.native_activation_required(&restored_detection);
            summary.needs_restart = needs_manual_restart;
            summary.message = Some(if needs_manual_restart {
                "WorkBuddy 已恢复切换前的原模型。配置已写入，重启 WorkBuddy 后即可使用原模型。"
                    .to_owned()
            } else {
                "WorkBuddy 已恢复切换前的原模型；若切换时正在运行，AT-Switch 已自动重新打开。"
                    .to_owned()
            });
        } else if adapter.id() == "codebuddy" {
            apply_restart_outcome(&mut summary, desktop_restart);
            let restart_message = summary.message.take().unwrap_or_default();
            summary.message = Some(if agent_stop_failed_message.is_some() {
                "CodeBuddy CN 已恢复工作区和会话切换前的模型选择。配置已写入，重启 CodeBuddy 后即可使用原模型。".to_owned()
            } else {
                format!("CodeBuddy CN 已恢复工作区和会话切换前的模型选择；{restart_message}")
            });
            summary.needs_restart = needs_manual_restart;
        } else if detection.needs_restart {
            if agent_stop_failed_message.is_some() {
                summary.needs_restart = true;
                summary.message = Some(format!(
                    "已恢复默认配置。重启 {} 后即可使用原模型。",
                    summary.display_name
                ));
            } else {
                apply_restart_outcome(&mut summary, desktop_restart);
            }
        }
        self.database.upsert_agent_state(&summary)?;
        Ok(summary)
    }

    /// Best-effort restore of an Agent's native disk configuration after its
    /// bound provider has been deleted.  Unlike [`restore_native`] this method:
    ///
    /// * Does **not** pause/restart the Agent process (the user is performing a
    ///   management operation; they can restart manually if needed).
    /// * Does **not** delete the `agent_bindings` row (already removed by the
    ///   provider deletion transaction).
    /// * Silently succeeds when the Agent is not installed or read-only.
    pub async fn restore_native_after_provider_deletion(&self, agent_id: &str) -> AppResult<()> {
        let adapter = self.registry.adapter(agent_id)?;
        let detection = self.detect_agent(adapter)?;
        if detection.install_status == AgentInstallStatus::NotInstalled {
            return Ok(());
        }
        if !detection.write_supported {
            return Ok(());
        }
        let config_path = match detection.config_path.clone() {
            Some(p) => p,
            None => return Ok(()),
        };
        let baseline = self.transaction.baseline(agent_id, &config_path)?;
        let native_content = adapter.build_native_config(&detection, &baseline)?;

        let workbuddy_restore = if agent_id == "workbuddy" {
            self.plan_workbuddy_restore(&detection)?
        } else {
            WorkBuddyRestorePlan::default()
        };
        let codebuddy_restore = if agent_id == "codebuddy" {
            self.plan_codebuddy_restore(&detection)?
        } else {
            CodeBuddyRestorePlan::default()
        };

        self.transaction.apply_file(
            agent_id,
            FileChange {
                path: config_path,
                new_content: native_content,
            },
        )?;

        // Best-effort session/workspace restore for special Agents.
        let _ = workbuddy::apply_session_changes(&detection, &workbuddy_restore.changes);
        if let Some(selection) = &workbuddy_restore.previous_new_task {
            let _ = workbuddy::apply_new_task_selection(
                &detection,
                &selection.user_id,
                workbuddy_restore.restored_new_task_value.as_deref(),
            );
        }
        // 恢复原始配置时清除全局 state.vscdb 里 AT-Switch 写入的
        // chatSelectedModelGlobalMap。
        if agent_id == "codebuddy" {
            let mode = codebuddy_restore
                .previous_workspaces
                .first()
                .map(|w| w.selected_mode.as_str())
                .unwrap_or("craft");
            let _ = codebuddy::clear_global_model_selection(&detection, mode);
        }
        let _ = codebuddy::apply_workspace_selections(&codebuddy_restore.workspace_changes);
        let _ = codebuddy::apply_conversation_selections(&codebuddy_restore.conversation_changes);

        // Clean up any remaining runtime selections (binding row is already
        // gone from the provider deletion transaction, but runtime_selections
        // may still exist).
        let _ = self.database.delete_runtime_selections(agent_id);

        let restored_detection = self.detect_agent(adapter)?;
        let summary = restored_detection.summary();
        self.database.upsert_agent_state(&summary)?;
        Ok(())
    }

    fn verify_stored_binding(
        &self,
        adapter: &dyn AgentAdapter,
        detection: &AgentDetection,
        binding: &StoredAgentBinding,
    ) -> AppResult<()> {
        let provider = self.database.get_provider(&binding.provider_id)?;
        let model = provider
            .summary
            .models
            .iter()
            .find(|model| model.model_id == binding.default_model_id)
            .ok_or_else(|| CommandError::new("model_not_found", "绑定模型已不存在"))?;
        let mode = AgentBindingMode::parse(&binding.mode)
            .ok_or_else(|| CommandError::new("binding_mode_invalid", "绑定模式无法识别"))?;
        let credential = match mode {
            AgentBindingMode::Direct => {
                let reference = provider
                    .api_key_ref
                    .as_deref()
                    .ok_or_else(|| CommandError::new("secret_missing", "Provider 密钥不存在"))?;
                self.secret_store.get(reference)?
            }
            AgentBindingMode::Proxy => {
                let reference = binding.local_token_ref.as_deref().ok_or_else(|| {
                    CommandError::new("local_token_missing", "本地代理令牌不存在")
                })?;
                self.secret_store.get(reference)?
            }
        };
        let proxy_base_url = format!("http://127.0.0.1:{}/v1", self.database.proxy_port()?);
        let desired = DesiredAgentBinding {
            mode,
            provider_name: &provider.summary.name,
            model_id: &model.model_id,
            supports_tools: model.supports_tools,
            upstream_protocol: provider
                .summary
                .upstream_protocol_for(binding.request_protocol),
            source_protocol: binding.request_protocol,
            base_url: if mode == AgentBindingMode::Proxy {
                &proxy_base_url
            } else {
                &provider.summary.base_url
            },
            credential: credential.expose(),
        };
        adapter.verify_config(detection, &desired)?;
        if adapter.id() == "codebuddy" {
            codebuddy::verify_workspace_model(detection, &model.model_id)?;
            codebuddy::verify_current_conversation_model(detection, &model.model_id)?;
        }
        Ok(())
    }

    fn enrich_binding_health(&self, summary: &mut AgentSummary, binding: &StoredAgentBinding) {
        match AgentBindingMode::parse(&binding.mode) {
            Some(AgentBindingMode::Direct) => {}
            Some(AgentBindingMode::Proxy) => {
                let local_token_ready = binding.local_token_ref.is_some();
                let upstream_ready = self
                    .database
                    .get_provider(&binding.provider_id)
                    .ok()
                    .and_then(|provider| provider.api_key_ref)
                    .is_some();
                if !local_token_ready || !upstream_ready {
                    summary.config_health = AgentConfigHealth::TakeoverInterrupted;
                    summary.message = Some(
                        "代理绑定所需的本地令牌或上游密钥不可用；请解锁系统凭据库后重新应用"
                            .to_owned(),
                    );
                }
            }
            None => {
                summary.config_health = AgentConfigHealth::TakeoverInterrupted;
                summary.message = Some("Agent 绑定模式无法识别，请重新应用配置".to_owned());
            }
        }
    }

    fn activate_workbuddy_session_while_paused(
        &self,
        detection: &AgentDetection,
    ) -> AppResult<WorkBuddySessionMutation> {
        let previous_session = workbuddy::latest_session_selection(detection)?;
        let previous_new_task = workbuddy::read_new_task_selection(detection)?;
        let managed_session_model = workbuddy::managed_session_model(detection)?;
        let managed_new_task_selection = workbuddy::managed_new_task_selection(detection)?;

        let session_snapshot_created = if let Some(selection) = &previous_session {
            self.database.remember_runtime_selection(
                "workbuddy",
                &selection.id,
                selection.model.as_deref(),
            )?
        } else {
            false
        };
        let encoded_new_task =
            workbuddy::encode_runtime_selection(previous_new_task.value.as_deref());
        let new_task_snapshot_created = self.database.remember_runtime_selection(
            "workbuddy",
            &previous_new_task.scope_id,
            encoded_new_task.as_deref(),
        )?;

        let session_changes = previous_session
            .as_ref()
            .filter(|selection| selection.model.as_deref() != Some(managed_session_model.as_str()))
            .map(|selection| {
                vec![workbuddy::SessionModelChange {
                    id: selection.id.clone(),
                    model: Some(managed_session_model.clone()),
                }]
            })
            .unwrap_or_default();

        if let Err(error) = workbuddy::apply_session_changes(detection, &session_changes) {
            self.cleanup_workbuddy_runtime_snapshots(
                previous_session.as_ref(),
                &previous_new_task,
                session_snapshot_created,
                new_task_snapshot_created,
            );
            return Err(error);
        }

        if let Err(error) = workbuddy::apply_new_task_selection(
            detection,
            &previous_new_task.user_id,
            Some(&managed_new_task_selection),
        ) {
            self.restore_workbuddy_runtime_values(
                detection,
                previous_session.as_ref(),
                Some(&previous_new_task),
            );
            self.cleanup_workbuddy_runtime_snapshots(
                previous_session.as_ref(),
                &previous_new_task,
                session_snapshot_created,
                new_task_snapshot_created,
            );
            return Err(error);
        }

        Ok(WorkBuddySessionMutation {
            previous_session,
            previous_new_task,
            session_snapshot_created,
            new_task_snapshot_created,
        })
    }

    fn rollback_workbuddy_session(
        &self,
        detection: &AgentDetection,
        mutation: Option<&WorkBuddySessionMutation>,
    ) {
        let Some(mutation) = mutation else {
            return;
        };
        let Ok(pause) = workbuddy::pause_for_runtime_update(detection) else {
            return;
        };
        self.restore_workbuddy_runtime_values(
            detection,
            mutation.previous_session.as_ref(),
            Some(&mutation.previous_new_task),
        );
        self.cleanup_workbuddy_runtime_snapshots(
            mutation.previous_session.as_ref(),
            &mutation.previous_new_task,
            mutation.session_snapshot_created,
            mutation.new_task_snapshot_created,
        );
        let _ = pause.resume();
    }

    fn activate_codebuddy_workspaces_while_paused(
        &self,
        detection: &AgentDetection,
        model_id: &str,
    ) -> AppResult<CodeBuddyWorkspaceMutation> {
        let previous_workspaces = codebuddy::workspace_selections(detection)?;
        if previous_workspaces.is_empty() {
            return Err(CommandError::new(
                "codebuddy_workspace_state_missing",
                "CodeBuddy CN 尚未生成可同步的工作区模型状态",
            )
            .with_recovery("请先用 CodeBuddy CN 打开任意项目，完全退出后再重试切换。"));
        }
        let previous_conversations = codebuddy::current_conversation_selections(detection)?;
        let desired_workspaces =
            codebuddy::desired_workspace_selections(&previous_workspaces, model_id)?;
        let desired_conversations =
            codebuddy::desired_conversation_selections(&previous_conversations, model_id)?;
        let mut encoded_snapshots = previous_workspaces
            .iter()
            .map(|selection| {
                codebuddy::encode_workspace_selection(selection)
                    .map(|encoded| (selection.scope_id.clone(), encoded))
            })
            .collect::<AppResult<Vec<_>>>()?;
        encoded_snapshots.extend(
            previous_conversations
                .iter()
                .map(|selection| {
                    codebuddy::encode_conversation_selection(selection)
                        .map(|encoded| (selection.scope_id.clone(), encoded))
                })
                .collect::<AppResult<Vec<_>>>()?,
        );

        let mut snapshot_scope_ids = Vec::new();
        for (scope_id, encoded) in encoded_snapshots {
            match self
                .database
                .remember_runtime_selection("codebuddy", &scope_id, Some(&encoded))
            {
                Ok(true) => snapshot_scope_ids.push(scope_id),
                Ok(false) => {}
                Err(error) => {
                    for scope_id in &snapshot_scope_ids {
                        let _ = self
                            .database
                            .forget_runtime_selection("codebuddy", scope_id);
                    }
                    return Err(error);
                }
            }
        }

        // 写入全局 state.vscdb 的 chatSelectedModelGlobalMap 字段。
        // CodeBuddy UI 启动时优先读全局级别的这个字段来决定默认勾选，
        // 如果只写 workspace 级别的 chatSelectedModelMapV2，UI 仍会
        // 读全局的旧值并勾选旧模型。
        let global_selected_mode = previous_workspaces
            .first()
            .map(|w| w.selected_mode.as_str())
            .unwrap_or("craft");
        if let Err(error) =
            codebuddy::apply_global_model_selection(detection, global_selected_mode, model_id)
        {
            for scope_id in &snapshot_scope_ids {
                let _ = self
                    .database
                    .forget_runtime_selection("codebuddy", scope_id);
            }
            return Err(error);
        }
        if let Err(error) = codebuddy::apply_workspace_selections(&desired_workspaces) {
            let rollback = codebuddy::apply_workspace_selections(&previous_workspaces);
            if rollback.is_ok() {
                for scope_id in &snapshot_scope_ids {
                    let _ = self
                        .database
                        .forget_runtime_selection("codebuddy", scope_id);
                }
            }
            return match rollback {
                Ok(()) => Err(error),
                Err(_) => Err(CommandError::new(
                    "codebuddy_workspace_manual_recovery_required",
                    "CodeBuddy 工作区模型同步失败且自动回滚未完成",
                )
                .with_recovery("请保持 CodeBuddy CN 关闭，并在 CodeBuddy 的模型菜单中手动恢复。")),
            };
        }
        if let Err(error) = codebuddy::apply_conversation_selections(&desired_conversations) {
            let conversations_rollback =
                codebuddy::apply_conversation_selections(&previous_conversations);
            let workspaces_rollback = codebuddy::apply_workspace_selections(&previous_workspaces);
            if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                for scope_id in &snapshot_scope_ids {
                    let _ = self
                        .database
                        .forget_runtime_selection("codebuddy", scope_id);
                }
            }
            return if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                Err(error)
            } else {
                Err(CommandError::new(
                    "codebuddy_conversation_manual_recovery_required",
                    "CodeBuddy 会话模型同步失败且自动回滚未完成",
                )
                .with_recovery("请保持 CodeBuddy CN 关闭，并在 CodeBuddy 的模型菜单中手动恢复。"))
            };
        }
        if let Err(error) = codebuddy::verify_workspace_model(detection, model_id) {
            let conversations_rollback =
                codebuddy::apply_conversation_selections(&previous_conversations);
            let workspaces_rollback = codebuddy::apply_workspace_selections(&previous_workspaces);
            if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                for scope_id in &snapshot_scope_ids {
                    let _ = self
                        .database
                        .forget_runtime_selection("codebuddy", scope_id);
                }
            }
            return if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                Err(error)
            } else {
                Err(CommandError::new(
                    "codebuddy_workspace_manual_recovery_required",
                    "CodeBuddy 工作区模型校验失败且自动回滚未完成",
                )
                .with_recovery("请保持 CodeBuddy CN 关闭，并在 CodeBuddy 的模型菜单中手动恢复。"))
            };
        }
        if let Err(error) = codebuddy::verify_current_conversation_model(detection, model_id) {
            let conversations_rollback =
                codebuddy::apply_conversation_selections(&previous_conversations);
            let workspaces_rollback = codebuddy::apply_workspace_selections(&previous_workspaces);
            if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                for scope_id in &snapshot_scope_ids {
                    let _ = self
                        .database
                        .forget_runtime_selection("codebuddy", scope_id);
                }
            }
            return if conversations_rollback.is_ok() && workspaces_rollback.is_ok() {
                Err(error)
            } else {
                Err(CommandError::new(
                    "codebuddy_conversation_manual_recovery_required",
                    "CodeBuddy 当前会话模型校验失败且自动回滚未完成",
                )
                .with_recovery("请保持 CodeBuddy CN 关闭，并在 CodeBuddy 的模型菜单中手动恢复。"))
            };
        }

        Ok(CodeBuddyWorkspaceMutation {
            previous_workspaces,
            previous_conversations,
            snapshot_scope_ids,
        })
    }

    fn rollback_codebuddy_workspaces(&self, mutation: Option<&CodeBuddyWorkspaceMutation>) {
        let Some(mutation) = mutation else {
            return;
        };
        let conversations =
            codebuddy::apply_conversation_selections(&mutation.previous_conversations);
        let workspaces = codebuddy::apply_workspace_selections(&mutation.previous_workspaces);
        match (conversations, workspaces) {
            (Ok(()), Ok(())) => {
                for scope_id in &mutation.snapshot_scope_ids {
                    let _ = self
                        .database
                        .forget_runtime_selection("codebuddy", scope_id);
                }
            }
            (conversation_result, workspace_result) => {
                log::warn!(
                    "CodeBuddy runtime rollback after interrupted binding failed: conversation={:?}, workspace={:?}",
                    conversation_result.err().map(|error| error.message),
                    workspace_result.err().map(|error| error.message)
                );
            }
        }
    }

    fn plan_codebuddy_restore(
        &self,
        detection: &AgentDetection,
    ) -> AppResult<CodeBuddyRestorePlan> {
        let previous_workspaces = codebuddy::managed_workspace_selections(detection)?;
        let previous_conversations = codebuddy::managed_conversation_selections(detection)?;
        let originals = self
            .database
            .list_runtime_selections("codebuddy")?
            .into_iter()
            .map(|selection| (selection.scope_id, selection.original_value))
            .collect::<HashMap<_, _>>();
        let workspace_changes = previous_workspaces
            .iter()
            .map(|selection| {
                let original = originals
                    .get(&selection.scope_id)
                    .and_then(|value| value.as_deref());
                codebuddy::restore_workspace_selection(selection, original)
            })
            .collect::<AppResult<Vec<_>>>()?;
        let conversation_changes = previous_conversations
            .iter()
            .map(|selection| {
                let original = originals
                    .get(&selection.scope_id)
                    .and_then(|value| value.as_deref());
                codebuddy::restore_conversation_selection(selection, original)
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(CodeBuddyRestorePlan {
            previous_workspaces,
            workspace_changes,
            previous_conversations,
            conversation_changes,
        })
    }

    fn rollback_codebuddy_restore(&self, plan: &CodeBuddyRestorePlan) {
        if let Err(error) = codebuddy::apply_conversation_selections(&plan.previous_conversations) {
            log::warn!(
                "CodeBuddy conversation rollback after native restore failed: {}",
                error.message
            );
        }
        if let Err(error) = codebuddy::apply_workspace_selections(&plan.previous_workspaces) {
            log::warn!(
                "CodeBuddy workspace rollback after native restore failed: {}",
                error.message
            );
        }
    }

    fn plan_workbuddy_restore(
        &self,
        detection: &AgentDetection,
    ) -> AppResult<WorkBuddyRestorePlan> {
        let managed = workbuddy::managed_session_selections(detection)?;
        let originals = self
            .database
            .list_runtime_selections("workbuddy")?
            .into_iter()
            .map(|selection| (selection.scope_id, selection.original_value))
            .collect::<HashMap<_, _>>();
        let changes = managed
            .iter()
            .map(|selection| workbuddy::SessionModelChange {
                id: selection.id.clone(),
                model: originals
                    .get(&selection.id)
                    .cloned()
                    .unwrap_or_else(|| Some("auto".to_owned())),
            })
            .collect::<Vec<_>>();
        let current_new_task = workbuddy::read_new_task_selection(detection)?;
        let previous_new_task =
            workbuddy::is_managed_new_task_selection(detection, current_new_task.value.as_deref())
                .then_some(current_new_task);
        let restored_new_task_value = previous_new_task
            .as_ref()
            .and_then(|selection| originals.get(&selection.scope_id))
            .map(|value| workbuddy::decode_runtime_selection(value.as_deref()))
            .transpose()?
            .flatten();
        Ok(WorkBuddyRestorePlan {
            previous: managed,
            changes,
            previous_new_task,
            restored_new_task_value,
        })
    }

    fn rollback_workbuddy_restore(&self, detection: &AgentDetection, plan: &WorkBuddyRestorePlan) {
        let changes = plan
            .previous
            .iter()
            .map(|selection| workbuddy::SessionModelChange {
                id: selection.id.clone(),
                model: selection.model.clone(),
            })
            .collect::<Vec<_>>();
        let _ = workbuddy::apply_session_changes(detection, &changes);
        if let Some(selection) = &plan.previous_new_task {
            let _ = workbuddy::apply_new_task_selection(
                detection,
                &selection.user_id,
                selection.value.as_deref(),
            );
        }
    }

    fn restore_workbuddy_runtime_values(
        &self,
        detection: &AgentDetection,
        session: Option<&workbuddy::SessionSelection>,
        new_task: Option<&workbuddy::NewTaskSelection>,
    ) {
        if let Some(session) = session {
            let _ = workbuddy::apply_session_changes(
                detection,
                &[workbuddy::SessionModelChange {
                    id: session.id.clone(),
                    model: session.model.clone(),
                }],
            );
        }
        if let Some(new_task) = new_task {
            let _ = workbuddy::apply_new_task_selection(
                detection,
                &new_task.user_id,
                new_task.value.as_deref(),
            );
        }
    }

    fn cleanup_workbuddy_runtime_snapshots(
        &self,
        session: Option<&workbuddy::SessionSelection>,
        new_task: &workbuddy::NewTaskSelection,
        session_snapshot_created: bool,
        new_task_snapshot_created: bool,
    ) {
        if session_snapshot_created {
            if let Some(session) = session {
                let _ = self
                    .database
                    .forget_runtime_selection("workbuddy", &session.id);
            }
        }
        if new_task_snapshot_created {
            let _ = self
                .database
                .forget_runtime_selection("workbuddy", &new_task.scope_id);
        }
    }

    fn load_or_create_local_token(
        &self,
        agent_id: &str,
    ) -> AppResult<(SecretValue, String, i64, bool)> {
        if let Some(binding) = self.database.get_agent_binding(agent_id)? {
            if let Some(reference) = binding.local_token_ref {
                if self.secret_store.exists(&reference) {
                    return Ok((
                        self.secret_store.get(&reference)?,
                        reference,
                        binding.local_token_revision,
                        false,
                    ));
                }
            }
        }
        let revision = self
            .database
            .get_agent_binding(agent_id)?
            .map(|binding| binding.local_token_revision + 1)
            .unwrap_or(1);
        let reference = format!("agent/{agent_id}/local-token/v{revision}");
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let secret = SecretValue::new(URL_SAFE_NO_PAD.encode(bytes));
        self.secret_store.put(&reference, &secret)?;
        Ok((secret, reference, revision, true))
    }

    fn cleanup_new_local_token(&self, metadata: &Option<LocalTokenMetadata>) {
        if let Some((reference, _, true)) = metadata {
            if let Err(error) = self.secret_store.delete(reference) {
                log::warn!(
                    "unable to remove unused local routing token: {}",
                    error.message
                );
            }
        }
    }
}

fn enrich_summary(database: &Database, summary: &mut AgentSummary, binding: &StoredAgentBinding) {
    summary.provider_id = Some(binding.provider_id.clone());
    summary.model_id = Some(binding.default_model_id.clone());
    summary.mode = Some(binding.mode.clone());
    if let Ok(provider) = database.get_provider(&binding.provider_id) {
        summary.provider_name = Some(provider.summary.name);
    }
}

fn apply_restart_outcome(summary: &mut AgentSummary, outcome: Option<AppResult<RestartOutcome>>) {
    match outcome {
        Some(Ok(RestartOutcome::Relaunched)) => {
            summary.needs_restart = false;
            summary.message = Some(format!(
                "{} 已自动重新打开，新配置已经生效。",
                summary.display_name
            ));
        }
        Some(Ok(RestartOutcome::WasNotRunning)) => {
            summary.needs_restart = false;
            summary.message = Some(format!(
                "{} 切换时未运行；下次启动会直接使用新配置。",
                summary.display_name
            ));
        }
        Some(Ok(RestartOutcome::ManualRequired)) => {
            // 仅命令行（Command）类型的 Agent 会产出 ManualRequired；
            // 这里展示 CLI 专属提示，避免对桌面应用显示错误的 CLI 文案。
            summary.needs_restart = true;
            summary.message = Some(format!(
                "已保存配置，但只检测到 {} 命令行版本；请重新启动相关 CLI 任务。",
                summary.display_name
            ));
        }
        None => {
            // 当前调用流程下 desktop_restart 总会被 pause.resume() 填值；
            // 保留 None 兜底以应对调用方调整：当作桌面应用未运行处理。
            summary.needs_restart = false;
            summary.message = Some(format!(
                "已保存 {} 配置，下次启动即可生效。",
                summary.display_name
            ));
        }
        Some(Err(error)) => {
            log::warn!(
                "{} configuration was saved but automatic relaunch failed: {}",
                summary.display_name,
                error.message
            );
            summary.needs_restart = true;
            summary.message = Some(format!(
                "配置已保存，但 {} 未能自动重新打开；请手动启动。",
                summary.display_name
            ));
        }
    }
}

fn writable_target(path: &std::path::Path) -> bool {
    if path.exists()
        && fs::metadata(path)
            .map(|metadata| metadata.permissions().readonly())
            .unwrap_or(true)
    {
        return false;
    }
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent.exists() {
            return fs::metadata(parent)
                .map(|metadata| !metadata.permissions().readonly())
                .unwrap_or(false);
        }
        current = parent.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ModelDraft, ModelOutputModality, ProviderDraft, ProviderKind, ProxyRuntimeStatus,
        VerificationStatus,
    };
    use crate::infrastructure::MemorySecretStore;

    fn strict_json_probe(path: &PathBuf) -> AppResult<()> {
        serde_json::from_slice::<serde_json::Value>(&fs::read(path)?)
            .map(|_| ())
            .map_err(|_| CommandError::new("agent_config_unparseable", "invalid test config"))
    }

    #[test]
    fn registry_keeps_the_public_agent_ids_stable() {
        let registry = AgentRegistry::default();
        let ids = registry
            .adapters
            .iter()
            .map(|adapter| adapter.id())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "workbuddy",
                "codebuddy",
                "qclaw",
                "ima",
                "autoclaw",
                "trae",
                "codex"
            ]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_standard_installations_are_discovered_for_every_registered_agent() {
        let temp = tempfile::tempdir().expect("temp");
        let local_app_data = temp.path().join("LocalAppData");
        for relative in [
            "Programs/WorkBuddy/WorkBuddy.exe",
            "Programs/CodeBuddy CN/CodeBuddy CN.exe",
            "Programs/QClaw/QClaw.exe",
            "Programs/ima.copilot/ima.copilot.exe",
            "Programs/AutoClaw/AutoClaw.exe",
            "Programs/TRAE/TRAE.exe",
            "Programs/Codex/Codex.exe",
        ] {
            let executable = local_app_data.join(relative);
            fs::create_dir_all(executable.parent().expect("parent")).expect("app directory");
            fs::write(executable, b"test executable").expect("executable");
        }
        let registry = AgentRegistry {
            context: DiscoveryContext {
                home: temp.path().join("home"),
                application_data_dir: temp.path().join("Roaming"),
                application_dirs: Vec::new(),
                path_entries: Vec::new(),
                system_application_search: false,
                custom_installation_path: None,
                local_app_data: Some(local_app_data),
                program_files: Vec::new(),
            },
            ..AgentRegistry::default()
        };

        let detections = registry.detections();
        assert_eq!(detections.len(), 7);
        for detection in detections {
            assert_ne!(
                detection.install_status,
                AgentInstallStatus::NotInstalled,
                "{} should be detected from its Windows standard path",
                detection.display_name
            );
            assert!(detection.installation.is_some());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn custom_installation_directories_work_for_all_five_switchable_agents() {
        let temp = tempfile::tempdir().expect("temp");
        let registry = AgentRegistry {
            context: DiscoveryContext {
                home: temp.path().join("home"),
                application_data_dir: temp.path().join("Roaming"),
                application_dirs: Vec::new(),
                path_entries: Vec::new(),
                system_application_search: false,
                custom_installation_path: None,
                local_app_data: None,
                program_files: Vec::new(),
            },
            ..AgentRegistry::default()
        };

        for (agent_id, executable_name) in [
            ("workbuddy", "WorkBuddy.exe"),
            ("codebuddy", "CodeBuddy CN.exe"),
            ("qclaw", "QClaw.exe"),
            ("autoclaw", "AutoClaw.exe"),
            ("codex", "Codex.exe"),
        ] {
            let custom_directory = temp.path().join(format!("custom-{agent_id}"));
            let executable = custom_directory.join(executable_name);
            fs::create_dir_all(&custom_directory).expect("custom directory");
            fs::write(&executable, b"test executable").expect("executable");

            let adapter = registry.adapter(agent_id).expect("adapter");
            let detection = registry.detect_adapter(adapter, Some(&custom_directory));

            assert!(
                detection.using_custom_install_path,
                "{agent_id} should use its selected installation directory"
            );
            assert_eq!(
                detection
                    .installation
                    .as_ref()
                    .map(|installation| installation.path.as_path()),
                Some(executable.as_path())
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn custom_installation_directories_work_for_all_five_switchable_agents_on_macos() {
        let temp = tempfile::tempdir().expect("temp");
        let registry = AgentRegistry {
            context: DiscoveryContext {
                home: temp.path().join("home"),
                application_data_dir: temp.path().join("Application Support"),
                application_dirs: Vec::new(),
                path_entries: Vec::new(),
                system_application_search: false,
                custom_installation_path: None,
            },
            ..AgentRegistry::default()
        };

        for (agent_id, app_name) in [
            ("workbuddy", "WorkBuddy.app"),
            ("codebuddy", "CodeBuddy CN.app"),
            ("qclaw", "QClaw.app"),
            ("autoclaw", "AutoClaw.app"),
            ("codex", "Codex.app"),
        ] {
            let custom_directory = temp.path().join(format!("custom-{agent_id}"));
            let app = custom_directory.join(app_name);
            fs::create_dir_all(app.join("Contents")).expect("custom app bundle");

            let adapter = registry.adapter(agent_id).expect("adapter");
            let detection = registry.detect_adapter(adapter, Some(&custom_directory));

            assert!(
                detection.using_custom_install_path,
                "{agent_id} should use its selected macOS installation directory"
            );
            assert_eq!(
                detection
                    .installation
                    .as_ref()
                    .map(|installation| installation.path.as_path()),
                Some(app.as_path())
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn custom_macos_install_path_is_validated_persisted_and_used_first() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let custom_directory = temp.path().join("Custom WorkBuddy");
        let app = custom_directory.join("WorkBuddy.app");
        fs::create_dir_all(app.join("Contents")).expect("custom app bundle");
        fs::create_dir_all(home.join(".workbuddy")).expect("config directory");
        fs::write(home.join(".workbuddy/models.json"), b"[]").expect("config");
        let database = Arc::new(Database::in_memory().expect("database"));
        let secret_store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let proxy = ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy");
        let mut service = AgentService::new(
            Arc::clone(&database),
            secret_store,
            temp.path().join("backups"),
            proxy,
        );
        service.registry.context = DiscoveryContext {
            home,
            application_data_dir: temp.path().join("Application Support"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        };

        let summary = service
            .set_custom_install_path("workbuddy", custom_directory.to_str())
            .expect("custom installation");

        assert!(summary.using_custom_install_path);
        assert_eq!(summary.install_path.as_deref(), app.to_str());
        assert_eq!(
            database
                .custom_agent_install_paths()
                .expect("custom paths")
                .get("workbuddy")
                .map(String::as_str),
            custom_directory.to_str()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn custom_install_path_is_validated_persisted_and_used_first() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let custom_directory = temp.path().join("Custom WorkBuddy");
        let executable = custom_directory.join("WorkBuddy.exe");
        fs::create_dir_all(&custom_directory).expect("custom directory");
        fs::write(&executable, b"test executable").expect("executable");
        fs::create_dir_all(home.join(".workbuddy")).expect("config directory");
        fs::write(home.join(".workbuddy/models.json"), b"[]").expect("config");
        let database = Arc::new(Database::in_memory().expect("database"));
        let secret_store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let proxy = ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy");
        let mut service = AgentService::new(
            Arc::clone(&database),
            secret_store,
            temp.path().join("backups"),
            proxy,
        );
        service.registry.context = DiscoveryContext {
            home,
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: None,
            program_files: Vec::new(),
        };

        let summary = service
            .set_custom_install_path("workbuddy", custom_directory.to_str())
            .expect("custom installation");

        assert!(summary.using_custom_install_path);
        assert_eq!(summary.install_path.as_deref(), executable.to_str());
        assert_eq!(
            database
                .custom_agent_install_paths()
                .expect("custom paths")
                .get("workbuddy")
                .map(String::as_str),
            custom_directory.to_str()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn custom_install_path_rejects_a_directory_for_another_agent() {
        let temp = tempfile::tempdir().expect("temp");
        let custom_directory = temp.path().join("Not WorkBuddy");
        fs::create_dir_all(&custom_directory).expect("custom directory");
        fs::write(custom_directory.join("QClaw.exe"), b"test executable")
            .expect("wrong executable");
        let database = Arc::new(Database::in_memory().expect("database"));
        let secret_store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let proxy = ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy");
        let mut service =
            AgentService::new(database, secret_store, temp.path().join("backups"), proxy);
        service.registry.context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: None,
            program_files: Vec::new(),
        };

        let error = service
            .set_custom_install_path("workbuddy", custom_directory.to_str())
            .expect_err("wrong agent must be rejected");

        assert_eq!(error.code, "custom_installation_not_found");
    }

    #[test]
    fn proxy_routes_match_each_agent_native_protocol() {
        let registry = AgentRegistry::default();
        let provider = ProviderSummary {
            id: "preset-mongyun".to_owned(),
            name: "蒙云智算".to_owned(),
            kind: ProviderKind::Mongyun,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://api.example.test/v1".to_owned(),
            is_recommended: false,
            is_enabled: true,
            has_api_key: true,
            masked_api_key: None,
            verification_status: VerificationStatus::Verified,
            verified_model_id: None,
            default_model_id: None,
            models: Vec::new(),
        };

        for (agent_id, expected) in [
            ("workbuddy", ApiProtocol::OpenaiChatCompletions),
            ("codebuddy", ApiProtocol::OpenaiChatCompletions),
            ("qclaw", ApiProtocol::OpenaiChatCompletions),
            ("ima", ApiProtocol::OpenaiChatCompletions),
            ("autoclaw", ApiProtocol::OpenaiChatCompletions),
            ("codex", ApiProtocol::OpenaiResponses),
        ] {
            let adapter = registry.adapter(agent_id).expect("adapter");
            assert_eq!(
                matched_binding_protocols(adapter, AgentBindingMode::Proxy, &provider),
                (expected, expected),
                "{agent_id} should use its native upstream endpoint"
            );
        }
    }

    #[test]
    fn direct_routes_match_multi_protocol_provider_capabilities() {
        let registry = AgentRegistry::default();
        let provider = ProviderSummary {
            id: "preset-mongyun".to_owned(),
            name: "蒙云智算".to_owned(),
            kind: ProviderKind::Mongyun,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://api.example.test/v1".to_owned(),
            is_recommended: false,
            is_enabled: true,
            has_api_key: true,
            masked_api_key: None,
            verification_status: VerificationStatus::Verified,
            verified_model_id: None,
            default_model_id: None,
            models: Vec::new(),
        };

        for (agent_id, expected) in [
            ("workbuddy", ApiProtocol::OpenaiChatCompletions),
            ("codebuddy", ApiProtocol::OpenaiChatCompletions),
            ("qclaw", ApiProtocol::OpenaiChatCompletions),
            ("autoclaw", ApiProtocol::OpenaiChatCompletions),
            ("codex", ApiProtocol::OpenaiResponses),
        ] {
            let adapter = registry.adapter(agent_id).expect("adapter");
            assert_eq!(
                matched_binding_protocols(adapter, AgentBindingMode::Direct, &provider),
                (expected, expected),
                "{agent_id} should write a directly supported upstream endpoint"
            );
        }
    }

    #[test]
    fn failed_first_binding_removes_its_unused_local_token() {
        let temp = tempfile::tempdir().expect("temp");
        let database = Arc::new(Database::in_memory().expect("database"));
        let memory_store = Arc::new(MemorySecretStore::default());
        let secret_store: Arc<dyn SecretStore> = memory_store.clone();
        let proxy =
            ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy supervisor");
        let service = AgentService::new(database, secret_store, temp.path().join("backups"), proxy);
        let reference = "agent/workbuddy/local-token/v1";
        memory_store
            .put(reference, &SecretValue::new("local-token".to_owned()))
            .expect("seed");

        service.cleanup_new_local_token(&Some((reference.to_owned(), 1, true)));

        assert!(!memory_store.exists(reference));
    }

    #[test]
    fn codebuddy_workspace_switch_is_snapshotted_and_rollback_safe() {
        let temp = tempfile::tempdir().expect("temp");
        let database = Arc::new(Database::in_memory().expect("database"));
        let memory_store = Arc::new(MemorySecretStore::default());
        let secret_store: Arc<dyn SecretStore> = memory_store;
        let proxy =
            ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy supervisor");
        let service = AgentService::new(
            Arc::clone(&database),
            secret_store,
            temp.path().join("backups"),
            proxy,
        );
        let runtime_data_dir = temp.path().join("CodeBuddy CN");
        let workspace_dir = runtime_data_dir.join("User/workspaceStorage/workspace-a");
        fs::create_dir_all(&workspace_dir).expect("workspace directory");
        let workspace_database = workspace_dir.join("state.vscdb");
        let connection = rusqlite::Connection::open(&workspace_database).expect("workspace db");
        connection
            .execute_batch(
                "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
            )
            .expect("workspace schema");
        connection
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                rusqlite::params![
                    "Tencent-Cloud.coding-copilot",
                    serde_json::json!({
                        "chatSelectedMode": "craft",
                        "chatSelectedModelMapV2": "{\"craft\":\"glm-old\"}"
                    })
                    .to_string()
                ],
            )
            .expect("workspace state");
        drop(connection);
        let history_dir = runtime_data_dir
            .parent()
            .expect("application data")
            .join("CodeBuddyExtension/Data/account/CodeBuddyIDE/user/history/workspace-a");
        fs::create_dir_all(&history_dir).expect("history directory");
        fs::write(
            history_dir.join("index.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "current": "conversation-a",
                "conversations": [{
                    "id": "conversation-a",
                    "type": "craft",
                    "modelMap": {"craft": "glm-old"}
                }]
            }))
            .expect("history json"),
        )
        .expect("history index");
        let detection = AgentDetection {
            id: "codebuddy",
            display_name: "CodeBuddy",
            installation: None,
            config_path: Some(temp.path().join("models.json")),
            runtime_data_dir: Some(runtime_data_dir),
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };

        let mutation = service
            .activate_codebuddy_workspaces_while_paused(&detection, "glm-next")
            .expect("activate workspace");
        codebuddy::verify_workspace_model(&detection, "glm-next").expect("switched");
        codebuddy::verify_current_conversation_model(&detection, "glm-next")
            .expect("current conversation switched");
        assert_eq!(
            database
                .list_runtime_selections("codebuddy")
                .expect("snapshots")
                .len(),
            2
        );

        service.rollback_codebuddy_workspaces(Some(&mutation));
        codebuddy::verify_workspace_model(&detection, "glm-old").expect("rolled back");
        codebuddy::verify_current_conversation_model(&detection, "glm-old")
            .expect("conversation rolled back");
        assert!(database
            .list_runtime_selections("codebuddy")
            .expect("snapshots")
            .is_empty());
    }

    #[tokio::test]
    async fn restoring_persisted_proxy_routes_keeps_listener_stopped() {
        let temp = tempfile::tempdir().expect("temp");
        let database = Arc::new(Database::in_memory().expect("database"));
        let memory_store = Arc::new(MemorySecretStore::default());
        let secret_store: Arc<dyn SecretStore> = memory_store.clone();
        let proxy =
            ProxySupervisor::new(54187, Arc::clone(&secret_store)).expect("proxy supervisor");
        let service = AgentService::new(
            Arc::clone(&database),
            secret_store,
            temp.path().join("backups"),
            Arc::clone(&proxy),
        );

        let provider_secret_ref = "provider/provider-test/api-key/v1";
        let local_token_ref = "agent/workbuddy/local-token/v1";
        memory_store
            .put(
                provider_secret_ref,
                &SecretValue::new("upstream-test-key".to_owned()),
            )
            .expect("seed provider secret");
        memory_store
            .put(
                local_token_ref,
                &SecretValue::new("local-test-token".to_owned()),
            )
            .expect("seed local token");

        let provider = ProviderDraft {
            id: Some("provider-test".to_owned()),
            name: "Test Provider".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-test",
                &provider,
                Some(provider_secret_ref),
                1,
                Some("••••-key"),
            )
            .expect("save provider");
        let workbuddy = service
            .registry
            .adapter("workbuddy")
            .expect("adapter")
            .detect(&service.registry.context)
            .summary();
        database
            .upsert_agent_state(&workbuddy)
            .expect("save agent state");
        database
            .save_agent_binding(&StoredAgentBinding {
                agent_id: "workbuddy".to_owned(),
                mode: AgentBindingMode::Proxy.as_str().to_owned(),
                provider_id: "provider-test".to_owned(),
                default_model_id: "model-a".to_owned(),
                request_protocol: ApiProtocol::OpenaiChatCompletions,
                local_token_ref: Some(local_token_ref.to_owned()),
                local_token_revision: 1,
            })
            .expect("save proxy binding");

        service
            .restore_proxy_routes()
            .await
            .expect("restore routes");

        assert_eq!(proxy.status().await.status, ProxyRuntimeStatus::Stopped);
    }

    #[test]
    fn invalid_config_shape_stays_read_only() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("config.json");
        fs::write(&path, b"{invalid").expect("seed");
        let detection = AgentDetection::from_file_probe(
            "test",
            "Test",
            Some(Installation {
                path: temp.path().join("Test.app"),
                version: Some("1.0.0".to_owned()),
                kind: locator::InstallationKind::DesktopApp,
            }),
            path,
            strict_json_probe,
            false,
        );

        assert_eq!(detection.install_status, AgentInstallStatus::Installed);
        assert!(matches!(
            detection.config_health,
            AgentConfigHealth::Unparseable
        ));
        assert!(!detection.write_supported);
    }

    #[test]
    fn installed_agent_without_config_can_create_its_standard_file() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = AgentDetection::from_file_probe(
            "test",
            "Test",
            Some(Installation {
                path: temp.path().join("Test.app"),
                version: None,
                kind: locator::InstallationKind::DesktopApp,
            }),
            temp.path().join("state/config.json"),
            strict_json_probe,
            false,
        );

        assert_eq!(
            detection.install_status,
            AgentInstallStatus::InstalledUninitialized
        );
        assert!(matches!(
            detection.config_health,
            AgentConfigHealth::Healthy
        ));
        assert!(detection.write_supported);
        assert!(detection
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("Test ")));
    }
}
