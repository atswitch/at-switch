use serde::de::DeserializeOwned;
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

pub enum LoginPollOutcome {
    Pending,
    Success(Box<StoredTokens>),
    Expired,
}

pub struct TelemetryBatch<'a> {
    pub events: &'a [serde_json::Value],
    pub errors: &'a [serde_json::Value],
}

impl ApiClient {
    pub fn new(base: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                // 直连打包注入的服务端地址：不继承系统/环境代理，
                // 否则本地或企业代理会以 502 拦截遥测与登录请求。
                .no_proxy()
                .build()
                .unwrap_or_default(),
            base,
        }
    }

    pub async fn create_login_state(&self) -> Result<(String, Option<String>), CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/auth/state", self.base))
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

    pub async fn google_exchange(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<StoredTokens, CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/auth/google/exchange", self.base))
            .json(&json!({ "code": code, "codeVerifier": code_verifier }))
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
    ) -> Result<(), CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/account/logout", self.base))
            .bearer_auth(access_token)
            .json(&json!({ "refreshToken": refresh_token }))
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

    pub async fn telemetry_batch(&self, batch: &TelemetryBatch<'_>) -> Result<(), CommandError> {
        let response = self
            .http
            .post(format!("{}/v1/telemetry/batch", self.base))
            .json(&json!({ "events": batch.events, "errors": batch.errors }))
            .send()
            .await
            .map_err(|_| offline())?;
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
    pub async fn feedback(
        &self,
        access_token: Option<&str>,
        content: &str,
        contact: &str,
    ) -> Result<(), CommandError> {
        let mut request = self
            .http
            .post(format!("{}/v1/feedback", self.base))
            .json(&json!({ "content": content, "contact": contact }));
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
