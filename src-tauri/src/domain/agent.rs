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
    pub needs_restart: bool,
    pub automatic_restart_supported: bool,
    pub activation_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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
