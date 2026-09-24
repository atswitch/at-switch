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
    /// 最近若干条请求，新的在前。只在内存中保留，代理重启后清空。
    pub recent_requests: Vec<ProxyRequestLogEntry>,
    /// 当前已被代理路由的 Agent 列表（用量统计开关已开启 + 存在有效 binding）。
    /// 前端按此列表展示"用量明细"。
    #[serde(default)]
    pub proxied_agents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 使用日志的条目类型。
///
/// - `Request`：流量确实经过 AT-Switch 本地代理，能拿到上游状态码与用量。
/// - `Switch`：直连模式下请求由智能体直发上游，AT-Switch 不在链路上，唯一能观察到
///   的就是"切换"这个动作本身。两类合起来才是 AT-Switch 的完整使用情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum UsageKind {
    Request,
    Switch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageOutcome {
    Ok,
    Failed,
}

/// 一条使用记录。只含展示字段——**不记录请求体、响应体或任何凭据**。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogEntry {
    pub kind: UsageKind,
    /// RFC 3339 时间戳。
    pub at: String,
    pub agent_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    /// 上游 HTTP 状态码；切换类记录与连不上上游之外的场景为 `None`。
    pub status: Option<u16>,
    pub outcome: UsageOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl UsageLogEntry {
    pub fn request(
        agent_id: &str,
        provider_id: &str,
        provider_name: &str,
        model: &str,
        status: u16,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Self {
        Self {
            kind: UsageKind::Request,
            at: chrono::Utc::now().to_rfc3339(),
            agent_id: agent_id.to_owned(),
            provider_id: provider_id.to_owned(),
            provider_name: provider_name.to_owned(),
            model: model.to_owned(),
            status: Some(status),
            outcome: if (200..300).contains(&status) {
                UsageOutcome::Ok
            } else {
                UsageOutcome::Failed
            },
            input_tokens,
            output_tokens,
            error_code: None,
        }
    }

    #[allow(dead_code)]
    pub fn switch(
        agent_id: &str,
        provider_id: &str,
        provider_name: &str,
        model: &str,
        error_code: Option<String>,
    ) -> Self {
        Self {
            kind: UsageKind::Switch,
            at: chrono::Utc::now().to_rfc3339(),
            agent_id: agent_id.to_owned(),
            provider_id: provider_id.to_owned(),
            provider_name: provider_name.to_owned(),
            model: model.to_owned(),
            status: None,
            outcome: if error_code.is_some() {
                UsageOutcome::Failed
            } else {
                UsageOutcome::Ok
            },
            input_tokens: None,
            output_tokens: None,
            error_code,
        }
    }
}

/// 一条请求的摘要，仅含展示所需字段——**不记录请求体、响应体或任何凭据**。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestLogEntry {
    /// RFC 3339 时间戳。
    pub at: String,
    pub agent_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    /// 上游返回的 HTTP 状态码；连不上上游时记为 502。
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}
