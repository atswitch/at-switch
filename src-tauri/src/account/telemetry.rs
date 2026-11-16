use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::client::{ApiClient, TelemetryBatch};
use super::config;
use super::session::SessionStore;

/// 遥测收集器：白名单元数据（使用事件 + 卡点错误）先进内存队列，
/// 每 60s 或满 20 条批量上报；失败静默丢弃、不重试、不阻塞主流程（设计 §3.5.1）。
/// 未登录也上报（匿名 device_id）；flush 时自动附上当前登录态 token 以便服务端写入 user_id。
pub struct TelemetryCollector {
    device_id: String,
    api: Option<ApiClient>,
    queue: Mutex<CollectorState>,
    /// 用于 flush 时获取当前 access token；未注入时始终匿名上报。
    session: Option<Arc<dyn AccessTokenSource>>,
}

/// 登录态抽象：收集器只需要「当前 token 与用户」，不关心 token 如何持久化。
pub trait AccessTokenSource: Send + Sync {
    fn access_token(&self) -> Option<String>;

    /// 客户端声明的用户 ID：服务端无 JWT 时作为兜底，避免整批数据变成匿名。
    fn user_id(&self) -> Option<String>;
}

impl AccessTokenSource for SessionStore {
    fn access_token(&self) -> Option<String> {
        self.load().map(|tokens| tokens.access_token)
    }

    fn user_id(&self) -> Option<String> {
        self.load().map(|tokens| tokens.profile.user_id)
    }
}

const MAX_QUEUE: usize = 200;

#[derive(Default)]
struct CollectorState {
    events: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
}

/// 埋点调用方提供的最小上下文；设备、平台与本地时间由收集器统一生成。
pub struct Event<'a> {
    pub event: &'a str,
    pub app_version: &'a str,
    pub target_id: &'a str,
    /// 事件关联的模型（agent_switch / 模型使用统计用，可为空）。
    pub model_id: &'a str,
    /// 供应商名称（provider_save/delete、使用明细用，可为空）。
    pub provider_name: &'a str,
    /// 模型明细摘要（供应商下的模型列表，逗号分隔，可为空）。
    pub model_detail: &'a str,
    /// 代理状态（direct/proxy/stopped/started，使用明细用，可为空）。
    pub proxy_mode: &'a str,
    pub result: &'a str,
    pub user_id: Option<&'a str>,
}

pub struct ErrorEvent<'a> {
    pub error_code: &'a str,
    pub message: &'a str,
    pub app_version: &'a str,
    pub context_tag: &'a str,
    pub user_id: Option<&'a str>,
}

impl TelemetryCollector {
    pub fn new(device_id: String, api_base: String) -> Self {
        Self {
            device_id,
            api: Some(ApiClient::new(api_base)),
            queue: Mutex::new(CollectorState::default()),
            session: None,
        }
    }

    /// 注入登录态来源：flush 时读取 access token 让服务端关联 user_id。
    pub fn with_session(mut self, session: Arc<dyn AccessTokenSource>) -> Self {
        self.session = Some(session);
        self
    }

    #[cfg(test)]
    fn without_network(device_id: String) -> Self {
        Self {
            device_id,
            api: None,
            queue: Mutex::new(CollectorState::default()),
            session: None,
        }
    }

    pub fn record_event(&self, event: Event<'_>) {
        let mut state = self.queue.lock().expect("telemetry queue poisoned");
        if state.events.len() >= MAX_QUEUE {
            return;
        }
        state.events.push(json!({
            "deviceId": self.device_id,
            "event": event.event,
            "appVersion": event.app_version,
            "os": std::env::consts::OS,
            "osVersion": os_version(),
            "arch": std::env::consts::ARCH,
            "targetId": event.target_id,
            "modelId": event.model_id,
            "providerName": event.provider_name,
            "modelDetail": event.model_detail,
            "proxyMode": event.proxy_mode,
            "result": event.result,
            "userId": event.user_id,
            "ts": local_timestamp(),
        }));
    }

    /// 卡点错误与使用事件保持同一套设备/平台/版本/时间维度，便于明细表逐条对齐排查。
    pub fn record_error(&self, error: ErrorEvent<'_>) {
        let mut state = self.queue.lock().expect("telemetry queue poisoned");
        if state.errors.len() >= MAX_QUEUE {
            return;
        }
        state.errors.push(json!({
            "deviceId": self.device_id,
            "errorCode": error.error_code,
            "errorMessage": error.message,
            "appVersion": error.app_version,
            "os": std::env::consts::OS,
            "osVersion": os_version(),
            "arch": std::env::consts::ARCH,
            "contextTag": error.context_tag,
            "userId": error.user_id,
            "ts": local_timestamp(),
        }));
    }

    /// 批量 flush：满 20 条由记录方触发一次尝试；周期 flush 由后台任务负责。
    pub async fn flush(&self) {
        let owned = {
            let mut state = self.queue.lock().expect("telemetry queue poisoned");
            if state.events.is_empty() && state.errors.is_empty() {
                return;
            }
            OwnedBatch {
                events: std::mem::take(&mut state.events),
                errors: std::mem::take(&mut state.errors),
            }
        };
        let Some(api) = self.api.as_ref() else {
            return;
        };
        let token = self.session.as_ref().and_then(|s| s.access_token());
        let claim_user_id = self.session.as_ref().and_then(|s| s.user_id());
        // 静默丢弃：遥测失败不重试（设计 §3.5.1）。诊断输出走 stdout 便于终端排查。
        match api
            .telemetry_batch(token.as_deref(), claim_user_id.as_deref(), &owned.as_batch())
            .await
        {
            Ok(()) => println!("[telemetry] batch flushed: {} events, {} errors",
                owned.events.len(), owned.errors.len()),
            Err(error) => println!("[telemetry] flush failed: {} ({})", error.code, error.message),
        }
    }

    /// 后台周期 flush 任务；服务端地址未注入时为空操作。
    pub fn spawn_flusher(self: &Arc<Self>) {
        if config::api_base().is_none() {
            return;
        }
        let collector = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                collector.flush().await;
            }
        });
    }
}

// 批量载荷需要跨锁返回，内部用拥有所有权的等价结构再临时借用。
struct OwnedBatch {
    events: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
}

impl OwnedBatch {
    fn as_batch(&self) -> TelemetryBatch<'_> {
        TelemetryBatch {
            events: &self.events,
            errors: &self.errors,
        }
    }
}

/// 服务端 LocalDateTime 只接受无时区偏移的 ISO 格式，不能用 to_rfc3339()。
fn local_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// 系统版本号：随事件与错误一起上报，供明细表区分同平台的不同系统版本。
pub(crate) fn os_version() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    {
        if let Ok(version) = String::from_utf8(output.stdout) {
            return version.trim().to_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_accumulates_and_respects_limit() {
        let collector = TelemetryCollector::without_network("device-1".to_owned());
        for _ in 0..(MAX_QUEUE + 10) {
            collector.record_event(Event {
                event: "app_start",
                app_version: "3.18.0",
                target_id: "",
                model_id: "",
                provider_name: "",
                model_detail: "",
                proxy_mode: "",
                result: "success",
                user_id: None,
            });
        }
        assert_eq!(collector.queue.lock().unwrap().events.len(), MAX_QUEUE);
    }

    #[test]
    fn error_event_carries_platform_fields() {
        let collector = TelemetryCollector::without_network("device-1".to_owned());
        collector.record_error(ErrorEvent {
            error_code: "AUTH_LOGIN_FAILED",
            message: "boom",
            app_version: "3.18.0",
            context_tag: "login=google",
            user_id: None,
        });
        let queue = collector.queue.lock().unwrap();
        let row = &queue.errors[0];
        assert_eq!(row["os"], std::env::consts::OS);
        assert_eq!(row["arch"], std::env::consts::ARCH);
        assert!(row.get("osVersion").is_some());
        assert!(row.get("appVersion").is_some());
        assert!(row.get("ts").is_some());
    }
}
