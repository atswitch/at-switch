mod agent;
mod error;
mod provider;
mod proxy;
mod settings;

pub use agent::*;
pub use error::*;
pub use provider::*;
pub use proxy::*;
pub use settings::*;

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
    pub settings: AppSettings,
}
