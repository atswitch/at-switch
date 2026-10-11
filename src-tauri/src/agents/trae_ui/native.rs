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
    endpoint_path, AgentDetection, IdeSelectionBaseline, SelectionStorageBaseline, TraeKind,
    TraeModelInput, TraeUiSnapshot,
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

    pub(super) fn requires_initial_restart(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
    ) -> bool {
        // Passive refresh must not start/rebind a bridge or block behind an
        // operation. Only a completed, live, same-installation/account session
        // can suppress the one initial restart warning.
        let Ok(mut guard) = self.session.try_lock() else {
            return true;
        };
        !guard.as_mut().is_some_and(|session| {
            session.completed
                && session.keep_running
                && !session.is_finished()
                && session.matches_context(kind, detection)
        })
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
        if guard.as_mut().is_some_and(|session| {
            session.is_finished()
                || (session.completed && !session.matches_context(kind, detection))
        }) {
            *guard = None;
        }
        if guard.is_none() {
            *guard = Some(BridgeSession::start(kind, detection)?);
        }
        if guard.as_ref().is_some_and(|session| session.completed) {
            let expected_account = super::super::active_account(
                detection.config_path.as_deref().ok_or_else(|| {
                    CommandError::new("trae_state_missing", "Trae 账号状态文件不存在")
                })?,
                kind,
            )?;
            let session = guard.as_mut().expect("existing Trae session");
            session.completed = false;
            let ready: StorageReady = session.call(&json!({
                "operation": "begin_operation", "expectedAccount": expected_account,
            }))?;
            if !ready.ready {
                return Err(CommandError::new(
                    "trae_native_storage_unavailable",
                    "Trae 运行中的账号状态已变化",
                ));
            }
        }
        operation(guard.as_mut().expect("Trae session was just initialized"))
    }

    pub(super) fn finish_operation(&self) {
        if let Ok(mut guard) = self.session.lock() {
            if !guard.as_ref().is_some_and(|session| session.completed) {
                *guard = None;
            }
        }
    }

    pub(super) fn complete_operation(&self) -> AppResult<()> {
        let mut guard = self.session.lock().map_err(|_| {
            CommandError::new("trae_bridge_lock_failed", "Trae 原生模型服务锁不可用")
        })?;
        if let Some(session) = guard.as_mut() {
            if session
                .pause
                .as_ref()
                .is_some_and(|pause| pause.was_running())
            {
                session
                    .pause
                    .as_mut()
                    .expect("running pause")
                    .keep_current_running()?;
                session.pause.take();
                session.keep_running = true;
            }
            if session.keep_running {
                let _: Value = session.call(&json!({"operation": "retain"}))?;
                session.completed = true;
                return Ok(());
            }
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
            let command = if kind == TraeKind::Unified && label == "ide_composer" {
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
                selection_baseline: raw.selection_baseline,
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
        if kind == TraeKind::Unified && label == "ide_composer" {
            return self.with_session(kind, detection, |session| {
                let result: MutationResult = session.call(&json!({
                    "operation": "select_ide", "displayName": display_name,
                }))?;
                if result.verified != Some(true) {
                    return Err(CommandError::new(
                        "trae_native_selection_unverified",
                        "TRAE IDE 模型选择未完成写回校验",
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
        if kind != TraeKind::Unified {
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
                    "TRAE IDE 原模型恢复未完成校验",
                ));
            }
            Ok(())
        })
    }

    pub(super) fn restore_selection_baseline(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        label: &str,
        baseline: &SelectionStorageBaseline,
    ) -> AppResult<()> {
        self.with_session(kind, detection, |session| {
            let result: MutationResult = session.call(&json!({
                "operation": "restore_selection_baseline",
                "label": label,
                "baseline": baseline,
            }))?;
            if result.verified != Some(true) {
                return Err(CommandError::new(
                    "trae_native_selection_unverified",
                    "Trae 原模型选择未完成精确恢复",
                ));
            }
            Ok(())
        })
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
    #[serde(default)]
    selection_baseline: Option<SelectionStorageBaseline>,
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
    #[serde(default)]
    reason: Option<String>,
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
    #[serde(default)]
    error: Option<String>,
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
    completed: bool,
    keep_running: bool,
    executable: PathBuf,
    config_path: PathBuf,
    account: String,
    _script: TemporaryScript,
}

impl BridgeSession {
    fn start(kind: TraeKind, detection: &AgentDetection) -> AppResult<Self> {
        #[cfg(test)]
        let started = std::time::Instant::now();
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
        let expected_account = Some(super::super::active_account(
            detection.config_path.as_deref().ok_or_else(|| {
                CommandError::new("trae_state_missing", "Trae 账号状态文件不存在")
            })?,
            kind,
        )?);
        let script = TemporaryScript::create()?;
        let pause = lifecycle::pause_for_config_update(detection)?;
        #[cfg(test)]
        if std::env::var("AT_SWITCH_TRAE_LIVE_MUTATION").as_deref() == Ok("1") {
            eprintln!(
                "TRAE_BRIDGE_TIME pause_ms={}",
                started.elapsed().as_millis()
            );
        }
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
            completed: false,
            keep_running: false,
            executable,
            config_path: detection
                .config_path
                .clone()
                .expect("validated Trae profile"),
            account: expected_account.clone().unwrap_or_default(),
            _script: script,
        };
        let ready: BridgeResponse<Value> = session.read_response()?;
        #[cfg(test)]
        if std::env::var("AT_SWITCH_TRAE_LIVE_MUTATION").as_deref() == Ok("1") {
            eprintln!(
                "TRAE_BRIDGE_TIME ready_ms={}",
                started.elapsed().as_millis()
            );
        }
        if ready.ready == Some(true) {
            let storage: StorageReady = session.call(&json!({
                "operation": "bind_storage",
                "expectedAccount": expected_account,
            }))?;
            if !storage.ready {
                if let Some(reason) = storage.reason.as_deref() {
                    log::warn!("Trae native storage did not become ready: {reason}");
                    #[cfg(test)]
                    eprintln!("Trae native storage diagnostic: {reason}");
                }
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
            if let Some(diagnostic) = ready.error.as_deref() {
                log::warn!("Trae native bridge did not become ready: {diagnostic}");
                #[cfg(test)]
                eprintln!("Trae native bridge readiness diagnostic: {diagnostic}");
            }
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

    fn matches_context(&self, kind: TraeKind, detection: &AgentDetection) -> bool {
        detection
            .installation
            .as_ref()
            .is_some_and(|installation| trae_executable(&installation.path) == self.executable)
            && detection.config_path.as_deref() == Some(self.config_path.as_path())
            && super::super::active_account(&self.config_path, kind)
                .is_ok_and(|account| account == self.account)
    }

    fn release(&mut self) -> AppResult<()> {
        if !self.keep_running && !self.pause.as_ref().is_some_and(|pause| pause.was_running()) {
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
        let started = std::time::Instant::now();
        let line = serde_json::to_string(command)
            .map_err(|_| CommandError::internal("Trae 控制请求无法编码"))?;
        let result = self.call_line(&line);
        // Only protocol operation names, elapsed time and our error codes are
        // recorded. Never serialize requests, accounts, endpoints or secrets.
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self._script.0.with_extension("timing.log"))
        {
            let operation = command["operation"].as_str().unwrap_or("unknown");
            let code = result
                .as_ref()
                .err()
                .map_or("ok", |error| error.code.as_str());
            let _ = writeln!(
                file,
                "{operation} {}ms {code}",
                started.elapsed().as_millis()
            );
        }
        #[cfg(test)]
        if std::env::var("AT_SWITCH_TRAE_LIVE_MUTATION").as_deref() == Ok("1") {
            eprintln!(
                "TRAE_BRIDGE_TIME {}_ms={}",
                command["operation"].as_str().unwrap_or("unknown"),
                started.elapsed().as_millis()
            );
        }
        result
    }

    fn call_line<T: for<'de> Deserialize<'de>>(&mut self, line: &str) -> AppResult<T> {
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        let response: BridgeResponse<T> = self.read_response()?;
        if response.ok != Some(true) {
            if response.error.as_deref() == Some("trae_multiple_ide_windows") {
                return Err(CommandError::new(
                    "trae_multiple_ide_windows",
                    "TRAE 打开了多个 IDE 工作区，已阻止不完整切换",
                )
                .with_recovery("请只保留一个 TRAE IDE 工作区后重试，避免只切换部分窗口。"));
            }
            let operation = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|value| {
                    value
                        .get("operation")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "unknown".to_owned());
            if let Some(diagnostic) = response.error.as_deref() {
                log::warn!("Trae native bridge operation {operation} failed: {diagnostic}");
                #[cfg(test)]
                eprintln!("Trae native bridge operation diagnostic ({operation}): {diagnostic}");
            }
            return Err(
                CommandError::new("trae_bridge_operation_failed", "Trae 原生模型操作失败")
                    .with_recovery(format!(
                        "失败阶段：{operation}；{}。未提交成功，不会显示为使用中。",
                        bridge_failure_tag(response.error.as_deref())
                    )),
            );
        }
        response.result.ok_or_else(|| {
            CommandError::new("trae_bridge_invalid_response", "Trae 原生模型服务缺少结果")
        })
    }
}

fn bridge_failure_tag(diagnostic: Option<&str>) -> &'static str {
    let diagnostic = diagnostic.unwrap_or_default();
    if diagnostic.starts_with("cdp_timeout:") {
        "原生服务等待超时"
    } else if diagnostic.contains("trae_live_selection_mismatch")
        || diagnostic.contains("trae_ide_live_model_mismatch")
        || diagnostic.contains("trae_ide_live_mode_mismatch")
    {
        "在线模型选择校验不一致"
    } else if diagnostic.contains("trae_native_storage_invalid") {
        "原生存储格式异常"
    } else if diagnostic.contains("trae_native_account_mismatch") {
        "账号状态发生变化"
    } else {
        "原生服务未完成操作"
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
        if self.keep_running && !self.released {
            let _ = self.release();
        }
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

#[cfg(test)]
mod tests {
    use super::bridge_failure_tag;

    #[test]
    fn diagnostic_tags_do_not_expose_runtime_messages_or_credentials() {
        assert_eq!(
            bridge_failure_tag(Some("cdp_timeout:Runtime.evaluate")),
            "原生服务等待超时"
        );
        assert_eq!(
            bridge_failure_tag(Some(
                "trae_internal_evaluation_failed:trae_live_selection_mismatch"
            )),
            "在线模型选择校验不一致"
        );
        assert_eq!(
            bridge_failure_tag(Some(
                "error secret=fake-private-key https://private.invalid"
            )),
            "原生服务未完成操作"
        );
    }
}
