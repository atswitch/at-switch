use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallStatus {
    NotInstalled,
    InstalledUninitialized,
    Installed,
}

impl AgentInstallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::InstalledUninitialized => "installed_uninitialized",
            Self::Installed => "installed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    Running,
    NotRunning,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Reserved recovery states are persisted for forward-compatible migrations.
pub enum AgentConfigHealth {
    Healthy,
    Unreadable,
    Unparseable,
    Unwritable,
    UnsupportedVersion,
    ExternalChanged,
    TakeoverInterrupted,
    ManualRecoveryRequired,
}

impl AgentConfigHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Unreadable => "unreadable",
            Self::Unparseable => "unparseable",
            Self::Unwritable => "unwritable",
            Self::UnsupportedVersion => "unsupported_version",
            Self::ExternalChanged => "external_changed",
            Self::TakeoverInterrupted => "takeover_interrupted",
            Self::ManualRecoveryRequired => "manual_recovery_required",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub id: String,
    pub display_name: String,
    pub install_status: AgentInstallStatus,
    pub runtime_status: AgentRuntimeStatus,
    pub config_health: AgentConfigHealth,
    pub adapter_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_version: Option<String>,
    #[serde(default)]
    pub is_latest_version: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_install_path: Option<String>,
    #[serde(default)]
    pub using_custom_install_path: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// 用户在「本地代理」页是否开启了该 Agent 的用量统计开关。与 binding/mode
    /// 解耦：开启只表示"流量经过本地代理并被统计"，不改变切到了哪个模型。
    #[serde(default)]
    pub proxy_pref_enabled: bool,
    pub needs_restart: bool,
    pub automatic_restart_supported: bool,
    pub activation_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 当 Agent 无法被 AT-Switch 自动恢复到出厂默认模型（用户从未被 AT-Switch
    /// 接管过，磁盘上没有原始 baseline）时，由后端给出的人工恢复步骤，前端在
    /// 弹窗里逐条展示给用户。每条对应一行可执行的 UI 提示（如「打开 Hermes 设置
    /// → 选择默认模型」）。仅在恢复路径真正需要人工介入时填写，正常情况为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_recovery_steps: Option<Vec<ManualRecoveryStep>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRecoveryStep {
    /// 简短标题，例如「打开 Hermes 设置面板」。
    pub title: String,
    /// 可执行的详细说明，例如「在 Hermes 应用内点击左上角菜单 → Settings → Default Model」。
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBindingMode {
    Direct,
    Proxy,
}

impl AgentBindingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Proxy => "proxy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "proxy" => Some(Self::Proxy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBindingDraft {
    pub agent_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub mode: AgentBindingMode,
}
