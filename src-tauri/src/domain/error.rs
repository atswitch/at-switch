use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, CommandError>;

/// Stable error contract exposed over Tauri IPC.
///
/// `message` and `recovery` must already be safe to show and copy. Never place
/// raw upstream bodies, headers, credentials, or full Agent configuration here.
#[derive(Debug, Clone, Error, Serialize)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recovery: None,
        }
    }

    pub fn with_recovery(mut self, recovery: impl Into<String>) -> Self {
        self.recovery = Some(recovery.into());
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
            .with_recovery("请重试；若问题持续，请复制脱敏诊断摘要。")
    }
}

impl From<rusqlite::Error> for CommandError {
    fn from(error: rusqlite::Error) -> Self {
        log::error!("database operation failed: {error}");
        Self::internal("本地数据库操作失败")
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        log::error!("local I/O operation failed: {error}");
        Self::internal("本地文件操作失败")
    }
}
