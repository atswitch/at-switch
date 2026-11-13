use serde::{Deserialize, Serialize};

/// 会员信息（与服务端 profile 载荷一致，serde camelCase 对齐前端契约）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountProfile {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub provider_type: String,
}

/// 存入系统凭据库的会话载荷（JSON 序列化后整体加密存储）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub provider_type: String,
    pub profile: AccountProfile,
}

/// 前端可见的账号状态快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    /// 服务端地址是否已在打包期注入。
    pub configured: bool,
    pub logged_in: bool,
    /// 校验请求网络失败时保持登录态并标记离线（设计 §3.3）。
    pub offline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frozen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<AccountProfile>,
}

/// 登录发起结果：微信返回二维码地址；谷歌由客户端浏览器完成授权。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub method: String,
    pub login_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_url: Option<String>,
}

/// 轮询结果：pending / success / expired / error。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPoll {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<AccountProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
