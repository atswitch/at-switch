use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRuntimeStatus {
    Stopped,
    Starting,
    Running,
    Draining,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub status: ProxyRuntimeStatus,
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub active_connections: u64,
    pub completed_requests: u64,
    pub successful_requests: u64,
    pub conversion_failures: u64,
    pub upstream_failures: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
