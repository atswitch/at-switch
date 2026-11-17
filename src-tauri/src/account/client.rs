use serde::de::DeserializeOwned;
use serde::Serialize;
use serde::Deserialize;
use serde_json::json;

use crate::domain::CommandError;

use super::sync::SyncPayload;
use super::types::{AccountProfile, StoredTokens};

/// 服务端 API client：所有网络请求统一从这里发出并映射稳定错误码。
/// 网络失败一律 `account_offline`（调用方决定是否静默），业务失败按服务端错误码透传。
pub struct ApiClient {
    http: reqwest::Client,
    base: String,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateResponse {
    state: String,
    qr_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenEnvelope {
    token: TokenResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    provider_type: String,
    #[serde(default)]
    profile: AccountProfile,
}

impl From<TokenResponse> for StoredTokens {
    fn from(value: TokenResponse) -> Self {
        Self {
            access_token: value.access_token,
            refresh_token: value.refresh_token,
            provider_type: value.provider_type,
            profile: value.profile,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginStatePoll {
    status: String,
    #[serde(default)]
    token: Option<TokenEnvelope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleAuthorizeResponse {
    authorize_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProviderPreset {
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub priority: i64,
    /// 客户端 ProviderPresetModel[] 的 JSON 文本（原样透传给前端解析）。
    pub models: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteProviderPresetsResponse {
    items: Vec<RemoteProviderPreset>,
    /// 后台停用的内置预设 kind：客户端据此跳过内置预设兜底。
    /// 旧服务端不返回该字段，缺省空列表（行为与本改动前一致）。
    #[serde(default)]
    disabled_builtin_kinds: Vec<String>,
}

/// 供应商预设目录的快照：启用项 + 被停用的内置项 kind。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresetsSnapshot {
    pub items: Vec<RemoteProviderPreset>,
    pub disabled_builtin_kinds: Vec<String>,
}

pub enum LoginPollOutcome {
    Pending,
    Success(Box<StoredTokens>),
    Expired,
}

/// 登录/登出上报的设备上下文：服务端据此写入登录明细（设备、平台、版本）。
/// 与遥测使用同一 device_id，不额外派生设备标识。
#[derive(Debug, Clone, Default)]
pub struct DeviceContext {
    pub device_id: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub app_version: String,
}

impl DeviceContext {
    pub fn local(device_id: String) -> Self {
        Self {
            device_id,
            os: std::env::consts::OS.to_owned(),
            os_version: super::telemetry::os_version(),
            arch: std::env::consts::ARCH.to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    fn as_json(&self) -> serde_json::Value {
        json!({
            "deviceId": self.device_id,
            "os": self.os,
            "osVersion": self.os_version,
            "arch": self.arch,
            "appVersion": self.app_version,
        })
    }
}

pub struct TelemetryBatch<'a> {
    pub events: &'a [serde_json::Value],
    pub errors: &'a [serde_json::Value],
}

impl ApiClient {
    pub fn new(base: String) -> Self {
        // 直连打包注入的服务端地址：不继承系统/环境代理，
        // 否则本地或企业代理会以 502 拦截遥测与登录请求。
        // build 失败时不再静默退化成 `Client::new()`（那个会继承系统代理），
        // 现场打印一次原因，便于定位「会员服务离线」这类难复现问题。
        let http = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .no_proxy()
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                log::error!("api client build failed, falling back to default: {error}");
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(15))
                    .no_proxy()
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new())
            }
        };
        Self { http, base }
    }

    /// 申请微信扫码 state：同时把设备上下文中转给服务端，登录成功时写入登录明细。
    pub async fn create_login_state(
        &self,
        device: &DeviceContext,
    ) -> Result<(String, Option<String>), CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/auth/state", self.base))
            .json(&device.as_json())
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: StateResponse = parse_json(response).await?;
        Ok((parsed.state, parsed.qr_url))
    }

    pub async fn poll_login_state(&self, state: &str) -> Result<LoginPollOutcome, CommandError> {
        let response = self
            .http
            .get(format!("{}/v1/auth/state/{state}", self.base))
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: LoginStatePoll = parse_json(response).await?;
        match parsed.status.as_str() {
            "success" => {
                let tokens = parsed
                    .token
                    .map(|envelope| StoredTokens::from(envelope.token))
                    .ok_or_else(|| CommandError::internal("登录服务响应缺少 token"))?;
                Ok(LoginPollOutcome::Success(Box::new(tokens)))
            }
            "expired" => Ok(LoginPollOutcome::Expired),
            _ => Ok(LoginPollOutcome::Pending),
        }
    }

    /// 申请 Google 授权页地址（client_id / redirect_uri 由服务端拼装）。
    pub async fn google_authorize_url(
        &self,
        state: &str,
        challenge: &str,
    ) -> Result<String, CommandError> {
        let response = self
            .http
            .get(format!("{}/v1/auth/google/authorize", self.base))
            .query(&[("state", state), ("challenge", challenge)])
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: GoogleAuthorizeResponse = parse_json(response).await?;
        Ok(parsed.authorize_url)
    }

    /// 供应商预设目录（匿名可拉，与用户无关）：客户端供应商表单下拉的数据源。
    pub async fn provider_presets(&self) -> Result<ProviderPresetsSnapshot, CommandError> {
        let response = self
            .http
            .get(format!("{}/v1/provider-presets", self.base))
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: RemoteProviderPresetsResponse = parse_json(response).await?;
        Ok(ProviderPresetsSnapshot {
            items: parsed.items,
            disabled_builtin_kinds: parsed.disabled_builtin_kinds,
        })
    }

    pub async fn google_exchange(
        &self,
        code: &str,
        code_verifier: &str,
        device: &DeviceContext,
    ) -> Result<StoredTokens, CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/auth/google/exchange", self.base))
            .json(&json!({
                "code": code,
                "codeVerifier": code_verifier,
                "device": device.as_json(),
            }))
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: TokenEnvelope = parse_json(response).await?;
        Ok(StoredTokens::from(parsed.token))
    }

    pub async fn refresh(
        &self,
        refresh_token: &str,
        provider_type: &str,
    ) -> Result<StoredTokens, CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/auth/refresh", self.base))
            .json(&json!({ "refreshToken": refresh_token, "providerType": provider_type }))
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: TokenEnvelope = parse_json(response).await?;
        Ok(StoredTokens::from(parsed.token))
    }

    /// 校验登录态：成功返回最新 profile；403 冻结透传 `account_frozen`。
    pub async fn check(&self, access_token: &str) -> Result<AccountProfile, CommandError> {
        let response = self
            .http
            .get(format!("{}/v1/account/check", self.base))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: serde_json::Value = parse_json(response).await?;
        Ok(parse_profile(&parsed))
    }

    pub async fn logout(
        &self,
        access_token: &str,
        refresh_token: &str,
        device: &DeviceContext,
    ) -> Result<(), CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/account/logout", self.base))
            .bearer_auth(access_token)
            .json(&json!({
                "refreshToken": refresh_token,
                "device": device.as_json(),
            }))
            .send()
            .await
            .map_err(|_| offline())?;
        parse_json::<serde_json::Value>(response).await?;
        Ok(())
    }

    pub async fn sync_providers(
        &self,
        access_token: &str,
        payload: &SyncPayload,
    ) -> Result<(), CommandError> {
        let response = self
            .http
            .put(format!("{}/v1/sync/providers", self.base))
            .bearer_auth(access_token)
            .json(payload)
            .send()
            .await
            .map_err(|_| offline())?;
        parse_json::<serde_json::Value>(response).await?;
        Ok(())
    }

    /// 遥测批量上报：登录态携带 Bearer token，服务端据此写入 user_id；
    /// 未登录时匿名上报（token 为空）。token 过期不阻断上报，事件仍入库但 user_id 为空。
    /// `claim_user_id` 为客户端声明的用户身份，仅在服务端无 JWT 时作为兜底。
    pub async fn telemetry_batch(
        &self,
        access_token: Option<&str>,
        claim_user_id: Option<&str>,
        batch: &TelemetryBatch<'_>,
    ) -> Result<(), CommandError> {
        let mut request = self
            .http
            .post(format!("{}/v1/telemetry/batch", self.base))
            .json(&json!({
                "events": batch.events,
                "errors": batch.errors,
                "claimUserId": claim_user_id,
            }));
        if let Some(token) = access_token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|_| offline())?;
        parse_json::<serde_json::Value>(response).await?;
        Ok(())
    }

    /// 供应商配置云存档同步（匿名可报，登录态自动关联 user_id）。
    pub async fn sync_provider_configs(
        &self,
        token: Option<&str>,
        device_id: &str,
        os: &str,
        payload: &serde_json::Value,
    ) -> Result<(), CommandError> {
        let mut request = self
            .http
            .put(format!("{}/v1/providers/sync", self.base))
            .json(&json!({
                "deviceId": device_id,
                "os": os,
                "providers": payload,
            }));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|_| offline())?;
        parse_json::<serde_json::Value>(response).await?;
        Ok(())
    }

    /// 拉取当前用户的云端供应商配置（需登录），用于换设备恢复。
    pub async fn fetch_provider_configs(&self, token: &str) -> Result<serde_json::Value, CommandError> {
        let response = self
            .http
            .get(format!("{}/v1/providers/sync", self.base))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| offline())?;
        let parsed: serde_json::Value = parse_json(response).await?;
        Ok(parsed.get("providers").cloned().unwrap_or(serde_json::Value::Null))
    }

    /// 反馈免登录：token 为空时匿名提交。
    ///
    /// 反馈同样必须携带完整设备上下文（设备号/平台/系统版本/架构/客户端版本），
    /// 否则后台无法按设备与版本定位问题——匿名反馈也不能丢这组公共字段。
    ///
    /// 身份关联优先靠 Bearer token（服务端可信）；`claim_user_id` 作为兜底，
    /// 保证 token 恰好过期时反馈仍能挂到登录用户身上。
    pub async fn feedback(
        &self,
        access_token: Option<&str>,
        claim_user_id: Option<&str>,
        content: &str,
        contact: &str,
        device: &DeviceContext,
    ) -> Result<(), CommandError> {
        let mut request = self
            .http
            .post(format!("{}/v1/feedback", self.base))
            .json(&json!({
                "content": content,
                "contact": contact,
                "deviceId": device.device_id,
                "os": device.os,
                "osVersion": device.os_version,
                "arch": device.arch,
                "appVersion": device.app_version,
                "claimUserId": claim_user_id,
            }));
        if let Some(token) = access_token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|_| offline())?;
        parse_json::<serde_json::Value>(response).await?;
        Ok(())
    }
}

fn offline() -> CommandError {
    CommandError::new("account_offline", "网络不可达，会员服务暂不可用")
}

/// 统一响应处理：HTTP 2xx → 解析 JSON；非 2xx → 优先取服务端稳定错误码。
async fn parse_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, CommandError> {
    let status = response.status();
    let text = response.text().await.map_err(|_| offline())?;
    if !status.is_success() {
        let server_code = serde_json::from_str::<ErrorBody>(&text)
            .ok()
            .filter(|e| !e.code.is_empty())
            .map(|e| e.code);
        let code = match status.as_u16() {
            401 => server_code.unwrap_or_else(|| "auth_expired".into()),
            403 => server_code.unwrap_or_else(|| "account_frozen".into()),
            400 => server_code.unwrap_or_else(|| "auth_login_expired".into()),
            _ => server_code.unwrap_or_else(|| "auth_login_failed".into()),
        };
        return Err(CommandError::new(
            code,
            format!("会员服务返回错误（HTTP {status}）"),
        ));
    }
    if text.trim().is_empty() {
        // 2xx + 空 body（如 202/204）：按 null 处理
        return serde_json::from_str("null").map_err(|_| CommandError::internal("会员服务响应格式无效"));
    }
    serde_json::from_str(&text).map_err(|_| CommandError::internal("会员服务响应格式无效"))
}

fn parse_profile(value: &serde_json::Value) -> AccountProfile {
    let profile = value.get("profile").cloned().unwrap_or(value.clone());
    serde_json::from_value(profile).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_uses_nested_payload() {
        let raw = serde_json::json!({
            "profile": { "userId": "1", "username": "u", "displayName": "U",
                         "email": "e@x.io", "providerType": "google" }
        });
        let profile = parse_profile(&raw);
        assert_eq!(profile.user_id, "1");
        assert_eq!(profile.provider_type, "google");
    }

    #[test]
    fn parse_profile_tolerates_missing_fields() {
        let profile = parse_profile(&serde_json::json!({}));
        assert_eq!(profile.user_id, "");
    }
}
