use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};

use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    agents::lifecycle,
    domain::{AppResult, CommandError},
    services::endpoint_url,
};

use super::{
    endpoint_path, AgentDetection, IdeSelectionBaseline, TraeKind, TraeModelInput, TraeUiSnapshot,
};

const BRIDGE_SOURCE: &str = include_str!("../trae_bridge.cjs");

pub(super) struct NativeTraeControl {
    session: Mutex<Option<BridgeSession>>,
}

impl NativeTraeControl {
    pub(super) fn new() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }

    fn with_session<T>(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        operation: impl FnOnce(&mut BridgeSession) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self.session.lock().map_err(|_| {
            CommandError::new("trae_bridge_lock_failed", "Trae 原生模型服务锁不可用")
        })?;
        if guard.as_mut().is_some_and(BridgeSession::is_finished) {
            *guard = None;
        }
        if guard.is_none() {
            *guard = Some(BridgeSession::start(kind, detection)?);
        }
        operation(guard.as_mut().expect("Trae session was just initialized"))
    }

    pub(super) fn finish_operation(&self) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = None;
        }
    }

    pub(super) fn complete_operation(&self) -> AppResult<()> {
        let mut guard = self.session.lock().map_err(|_| {
            CommandError::new("trae_bridge_lock_failed", "Trae 原生模型服务锁不可用")
        })?;
        if let Some(session) = guard.as_mut() {
            session.release()?;
        }
        *guard = None;
        Ok(())
    }

    pub(super) fn snapshot(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
    ) -> AppResult<TraeUiSnapshot> {
        self.snapshot_for_label(kind, detection, kind.selection_label())
    }

    pub(super) fn snapshot_for_label(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        label: &str,
    ) -> AppResult<TraeUiSnapshot> {
        self.with_session(kind, detection, |session| {
            let command = if kind == TraeKind::Code && label == "ide_legacy" {
                json!({"operation": "snapshot_ide"})
            } else {
                json!({"operation": "snapshot", "label": label})
            };
            let raw: Snapshot = session.call(&command)?;
            if raw.code != 0 {
                return Err(CommandError::new(
                    "trae_native_model_list_failed",
                    "Trae 原生模型列表读取失败",
                ));
            }
            let effective = if raw.active_session {
                &raw.active_selection
            } else {
                &raw.selection
            };
            let selection = if effective.mode == Some(1) {
                "Auto Mode".to_owned()
            } else {
                effective.display_name.clone()
            };
            if selection.is_empty() {
                return Err(CommandError::new(
                    "trae_native_selection_missing",
                    "Trae 当前模型尚未加载完成",
                ));
            }
            let mut custom_models = HashSet::new();
            let mut custom_models_by_id: HashMap<String, BTreeSet<String>> = HashMap::new();
            let mut custom_endpoints_by_name: HashMap<String, BTreeSet<String>> = HashMap::new();
            for model in raw.models {
                if !model.provider.starts_with("custom_") || model.display_name.is_empty() {
                    continue;
                }
                custom_models.insert(model.display_name.clone());
                if let Some((_, model_id)) = model.name.split_once("//") {
                    custom_models_by_id
                        .entry(model_id.to_owned())
                        .or_default()
                        .insert(model.display_name.clone());
                }
                if !model.base_url.is_empty() {
                    custom_endpoints_by_name
                        .entry(model.display_name)
                        .or_default()
                        .insert(model.base_url);
                }
            }
            Ok(TraeUiSnapshot {
                selection,
                recent_selection: if raw.selection.mode == Some(1) {
                    "Auto Mode".to_owned()
                } else {
                    raw.selection.display_name.clone()
                },
                active_session_id: raw.active_session_id,
                active_selection: raw.active_session.then(|| {
                    if raw.active_selection.mode == Some(1) {
                        "Auto Mode".to_owned()
                    } else {
                        raw.active_selection.display_name.clone()
                    }
                }),
                custom_models,
                custom_models_by_id,
                custom_endpoints_by_name,
                ide_baseline: raw.ide_baseline,
            })
        })
    }

    pub(super) fn add_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        input: &TraeModelInput,
    ) -> AppResult<()> {
        let endpoint = endpoint_url(&input.base_url, endpoint_path(input.protocol))?;
        self.with_session(kind, detection, |session| {
            let protocol = match input.protocol {
                crate::domain::ApiProtocol::OpenaiChatCompletions => "openai_chat_completions",
                crate::domain::ApiProtocol::OpenaiResponses => "openai_responses",
                crate::domain::ApiProtocol::AnthropicMessages => "anthropic_messages",
            };
            let command = Zeroizing::new(
                serde_json::to_string(&json!({
                    "operation": "add_model",
                    "displayName": input.display_name,
                    "modelId": input.model_id,
                    "endpoint": endpoint.as_str(),
                    "apiKey": input.credential.as_str(),
                    "protocol": protocol,
                }))
                .map_err(|_| CommandError::internal("Trae 模型请求无法编码"))?,
            );
            let result: MutationResult = session.call_line(command.as_str())?;
            if result.added != Some(true) || result.verified != Some(true) {
                return Err(CommandError::new(
                    "trae_native_add_failed",
                    "Trae 未确认新增自定义模型",
                )
                .with_recovery("请检查 Provider 地址、模型 ID 和凭据，然后重新切换。"));
            }
            Ok(())
        })
    }

    pub(super) fn select_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.select_model_for_label(kind, detection, display_name, None, Some(None))
    }

    pub(super) fn select_model_scoped(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
        session_id: Option<&str>,
    ) -> AppResult<()> {
        self.select_model_for_label(kind, detection, display_name, None, Some(session_id))
    }

    pub(super) fn select_model_for_selection_label(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        label: &str,
        display_name: &str,
        session_id: Option<&str>,
    ) -> AppResult<()> {
        if kind == TraeKind::Code && label == "ide_legacy" {
            return self.with_session(kind, detection, |session| {
                let result: MutationResult = session.call(&json!({
                    "operation": "select_ide", "displayName": display_name,
                }))?;
                if result.verified != Some(true) {
                    return Err(CommandError::new(
                        "trae_native_selection_unverified",
                        "TraeCode IDE 模型选择未完成写回校验",
                    ));
                }
                Ok(())
            });
        }
        self.select_model_for_label(kind, detection, display_name, Some(label), Some(session_id))
    }

    pub(super) fn restore_ide_baseline(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        baseline: &IdeSelectionBaseline,
    ) -> AppResult<()> {
        if kind != TraeKind::Code {
            return Err(CommandError::new(
                "trae_ide_unsupported",
                "当前 Agent 不支持 IDE 模型选择",
            ));
        }
        self.with_session(kind, detection, |session| {
            let result: MutationResult = session.call(&json!({
                "operation": "restore_ide", "baseline": baseline,
            }))?;
            if result.verified != Some(true) {
                return Err(CommandError::new(
                    "trae_native_selection_unverified",
                    "TraeCode IDE 原模型恢复未完成校验",
                ));
            }
            Ok(())
        })
    }

    pub(super) fn restore_legacy_work_remote(
        &self,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.select_model_for_label(
            TraeKind::Work,
            detection,
            display_name,
            Some("solo_work_remote"),
            Some(None),
        )
    }

    fn select_model_for_label(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
        label: Option<&str>,
        session_id: Option<Option<&str>>,
    ) -> AppResult<()> {
        self.with_session(kind, detection, |session| {
            let mut command = if matches!(display_name, "Auto" | "Auto Mode") {
                json!({"operation": "select_auto", "label": label})
            } else {
                json!({"operation": "select", "displayName": display_name, "mode": 0, "label": label})
            };
            if let Some(session_id) = session_id {
                command["sessionId"] = json!(session_id);
            }
            let result: MutationResult = session.call(&command)?;
            if result.verified != Some(true) {
                if result.reason.as_deref() == Some("model_not_unique") {
                    return Err(CommandError::new(
                        if result.count == Some(0) {
                            "trae_native_model_missing"
                        } else {
                            "trae_native_model_ambiguous"
                        },
                        "Trae 模型列表中找不到唯一的目标模型",
                    ));
                }
                return Err(CommandError::new(
                    "trae_native_selection_unverified",
                    "Trae 原生模型选择未完成写回校验",
                ));
            }
            Ok(())
        })?;
        if label.is_some() || session_id.is_some() {
            return Ok(());
        }
        let actual = self.snapshot(kind, detection)?.selection;
        if actual == display_name
            || (matches!(display_name, "Auto" | "Auto Mode") && actual == "Auto Mode")
        {
            Ok(())
        } else {
            Err(CommandError::new(
                "trae_native_selection_mismatch",
                "Trae 当前会话的模型与目标不一致",
            ))
        }
    }

    pub(super) fn delete_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.with_session(kind, detection, |session| {
            let result: MutationResult = session.call(&json!({
                "operation": "delete_model",
                "displayName": display_name,
            }))?;
            if result.deleted != Some(true) || result.verified != Some(true) {
                return Err(CommandError::new(
                    "trae_native_delete_failed",
                    "Trae 未确认删除受管模型",
                ));
            }
            Ok(())
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    code: i64,
    selection: Selection,
    active_session: bool,
    active_session_id: Option<String>,
    active_selection: Selection,
    models: Vec<Model>,
    #[serde(default)]
    ide_baseline: Option<IdeSelectionBaseline>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Selection {
    mode: Option<i64>,
    #[serde(default)]
    display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Model {
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    base_url: String,
}

#[derive(Deserialize)]
struct MutationResult {
    verified: Option<bool>,
    added: Option<bool>,
    deleted: Option<bool>,
    reason: Option<String>,
    count: Option<usize>,
}

#[derive(Deserialize)]
struct StorageReady {
    ready: bool,
}

#[derive(Deserialize)]
struct RestoredSelection {
    restored: bool,
}

#[derive(Deserialize)]
struct BridgeResponse<T> {
    ok: Option<bool>,
    ready: Option<bool>,
    result: Option<T>,
}

struct TemporaryScript(PathBuf);

impl TemporaryScript {
    fn create() -> AppResult<Self> {
        let path = std::env::temp_dir().join(format!("at-switch-trae-{}.cjs", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(BRIDGE_SOURCE.as_bytes())?;
        file.sync_all()?;
        Ok(Self(path))
    }
}

impl Drop for TemporaryScript {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            log::warn!("unable to remove Trae bridge runtime: {error}");
        }
    }
}

struct BridgeSession {
    process: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<String>,
    pause: Option<lifecycle::DesktopAppPause>,
    released: bool,
    _script: TemporaryScript,
}

impl BridgeSession {
    fn start(kind: TraeKind, detection: &AgentDetection) -> AppResult<Self> {
        let installation = detection
            .installation
            .as_ref()
            .ok_or_else(|| CommandError::new("agent_not_installed", "Trae 安装位置不可用"))?;
        if !kind.supports_native_bridge(installation.version.as_deref()) {
            return Err(CommandError::new(
                "trae_native_version_unsupported",
                "当前 Trae 版本不支持无界面原生服务切换",
            )
            .with_recovery("请等待适配当前 Trae 版本；不会回退到可见界面操作。"));
        }
        let executable = trae_executable(&installation.path);
        if !executable.is_file() {
            return Err(CommandError::new(
                "trae_native_executable_missing",
                "Trae 原生执行文件不存在",
            ));
        }
        let script = TemporaryScript::create()?;
        let pause = lifecycle::pause_for_config_update(detection)?;
        let mut command = Command::new(&executable);
        command
            .env("ELECTRON_RUN_AS_NODE", "1")
            .arg(&script.0)
            .arg(&executable)
            .arg(kind.id())
            .arg("")
            .arg("true")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // The released pipe owner must survive AT-Switch exiting; Trae's
            // --remote-debugging-pipe instance depends on that owner.
            command.process_group(0);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut process = command.spawn().map_err(|_| {
            CommandError::new("trae_bridge_launch_failed", "无法启动 Trae 原生模型服务")
        })?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| CommandError::new("trae_bridge_pipe_missing", "Trae 控制通道不可用"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| CommandError::new("trae_bridge_pipe_missing", "Trae 控制通道不可用"))?;
        let responses = spawn_response_reader(stdout);
        let mut session = Self {
            process,
            stdin,
            responses,
            pause: Some(pause),
            released: false,
            _script: script,
        };
        let ready: BridgeResponse<Value> = session.read_response()?;
        if ready.ready == Some(true) {
            let storage: StorageReady = session.call(&json!({"operation": "bind_storage"}))?;
            if !storage.ready {
                return Err(CommandError::new(
                    "trae_native_storage_unavailable",
                    "Trae 原生模型存储未就绪",
                ));
            }
            let restored: RestoredSelection =
                session.call(&json!({"operation": "restore_selection"}))?;
            if !restored.restored {
                return Err(CommandError::new(
                    "trae_native_selection_restore_failed",
                    "Trae 无法读取已有模型选择",
                ));
            }
            Ok(session)
        } else {
            Err(CommandError::new(
                "trae_bridge_not_ready",
                "Trae 原生模型服务未就绪；版本或登录状态可能已变化",
            )
            .with_recovery("请确认 Trae 已登录，并检查当前 Trae 版本是否受支持。"))
        }
    }

    fn is_finished(&mut self) -> bool {
        self.process.try_wait().ok().flatten().is_some()
    }

    fn release(&mut self) -> AppResult<()> {
        if !self.pause.as_ref().is_some_and(|pause| pause.was_running()) {
            return Ok(());
        }
        let response: Value = self.call(&json!({"operation": "release"})).map_err(|_| {
            CommandError::new(
                "trae_bridge_release_failed",
                "Trae 切换后的进程无法保持运行",
            )
        })?;
        if response.get("released").and_then(Value::as_bool) != Some(true) {
            return Err(CommandError::new(
                "trae_bridge_release_failed",
                "Trae 切换后的进程无法保持运行",
            ));
        }
        if let Some(pause) = self.pause.as_mut() {
            pause.keep_current_running()?;
        }
        self.pause.take();
        self.released = true;
        Ok(())
    }

    fn read_response<T: for<'de> Deserialize<'de>>(&mut self) -> AppResult<BridgeResponse<T>> {
        let line = self
            .responses
            .recv_timeout(Duration::from_secs(120))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CommandError::new("trae_bridge_timeout", "Trae 原生模型服务响应超时")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CommandError::new("trae_bridge_disconnected", "Trae 原生模型服务连接中断")
                }
            })?;
        serde_json::from_str(&line).map_err(|_| {
            CommandError::new("trae_bridge_invalid_response", "Trae 原生模型服务响应无效")
        })
    }

    fn call<T: for<'de> Deserialize<'de>>(&mut self, command: &Value) -> AppResult<T> {
        let line = serde_json::to_string(command)
            .map_err(|_| CommandError::internal("Trae 控制请求无法编码"))?;
        self.call_line(&line)
    }

    fn call_line<T: for<'de> Deserialize<'de>>(&mut self, line: &str) -> AppResult<T> {
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        let response: BridgeResponse<T> = self.read_response()?;
        if response.ok != Some(true) {
            return Err(CommandError::new(
                "trae_bridge_operation_failed",
                "Trae 原生模型操作失败",
            ));
        }
        response.result.ok_or_else(|| {
            CommandError::new("trae_bridge_invalid_response", "Trae 原生模型服务缺少结果")
        })
    }
}

fn spawn_response_reader(stdout: ChildStdout) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if sender.send(line).is_err() => break,
                Ok(_) => {}
            }
        }
    });
    receiver
}

impl Drop for BridgeSession {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let _ = self
            .stdin
            .write_all(b"{\"operation\":\"shutdown\",\"terminateApp\":true}\n");
        let _ = self.stdin.flush();
        for _ in 0..70 {
            if self.process.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
        if let Some(pause) = self.pause.take() {
            if let Err(error) = pause.resume_and_wait() {
                log::warn!("unable to restore Trae runtime state: {}", error.message);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn trae_executable(installation: &Path) -> PathBuf {
    installation.join("Contents/MacOS/Electron")
}

#[cfg(target_os = "windows")]
fn trae_executable(installation: &Path) -> PathBuf {
    installation.to_path_buf()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn trae_executable(installation: &Path) -> PathBuf {
    installation.to_path_buf()
}
