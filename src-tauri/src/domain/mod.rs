mod agent;
mod error;
mod provider;
mod proxy;
mod settings;
mod update;

pub use agent::*;
pub use error::*;
pub use provider::*;
pub use proxy::*;
pub use settings::*;
pub use update::*;

use serde::Serialize;

/// Complete, already-redacted state returned to the WebView.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub app_version: String,
    pub platform: String,
    pub providers: Vec<ProviderSummary>,
    pub agents: Vec<AgentSummary>,
    pub proxy: ProxyStatus,
    /// 全量使用日志（代理请求 + 切换操作），新的在前。
    pub usage_log: Vec<UsageLogEntry>,
    pub settings: AppSettings,
}
