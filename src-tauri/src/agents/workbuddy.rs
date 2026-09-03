use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use rusty_leveldb::{Options as LevelDbOptions, DB as LevelDb};
use serde_json::{json, Value};

use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    lifecycle,
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct WorkBuddyAdapter;
// Older AT-Switch builds wrote this synthetic ID into WorkBuddy. Keep it only
// for migration cleanup; new configurations always use the real model ID.
const LEGACY_MANAGED_MODEL_ID: &str = "at-switch";
const MANAGED_VENDOR_PREFIX: &str = "AT-Switch · ";
const NEW_TASK_SCOPE_PREFIX: &str = "new-task:";
// WorkBuddy 5.3.x keeps the welcome/new-task selector in Chromium localStorage.
// Chromium adds internal key/value encoding markers, so snapshots preserve the
// exact bytes instead of parsing and rebuilding the user's original selection.
const CHROMIUM_FILE_ORIGIN_PREFIX: &[u8] = b"_file://\0\x01";
const NEW_TASK_MODEL_KEY_PREFIX: &str = "cb-newtask:model:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionSelection {
    pub id: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionModelChange {
    pub id: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewTaskSelection {
    pub scope_id: String,
    pub user_id: String,
    pub value: Option<Vec<u8>>,
}

impl AgentAdapter for WorkBuddyAdapter {
    fn id(&self) -> &'static str {
        "workbuddy"
    }

    fn display_name(&self) -> &'static str {
        "WorkBuddy"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["WorkBuddy.app"],
            &["com.workbuddy.workbuddy"],
            &[
                "Programs/WorkBuddy/WorkBuddy.exe",
                "WorkBuddy/WorkBuddy.exe",
                "Tencent/WorkBuddy/WorkBuddy.exe",
            ],
        );
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            context.home.join(".workbuddy/models.json"),
            probe_models,
            true,
        )
    }

    fn source_protocol(
        &self,
        _desired_mode: crate::domain::AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode == AgentBindingMode::Direct
            && desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "workbuddy_direct_protocol_unsupported",
                "WorkBuddy 直连模式要求 Provider 支持 OpenAI Chat API",
            )
            .with_recovery("请改用本地代理模式，AT-Switch 会完成协议转换。"));
        }
        Ok(())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        self.validate_binding(desired)?;
        if desired.source_protocol != ApiProtocol::OpenaiChatCompletions {
            return Err(CommandError::new(
                "workbuddy_protocol_unsupported",
                "WorkBuddy 当前适配仅支持 OpenAI Chat 兼容入口",
            ));
        }
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 WorkBuddy 配置路径")
        })?;
        build_models_config(
            if path.exists() {
                Some(fs::read(path)?)
            } else {
                None
            },
            desired,
        )
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        _baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 WorkBuddy 配置路径")
        })?;
        build_native_models_config(if path.exists() {
            Some(fs::read(path)?)
        } else {
            None
        })
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 WorkBuddy 配置路径")
        })?;
        let value: Value = serde_json::from_slice(&fs::read(path)?).map_err(|_| {
            CommandError::new("agent_config_unparseable", "WorkBuddy 模型配置无法解析")
        })?;
        let expected_url = format!(
            "{}/chat/completions",
            desired.base_url.trim_end_matches('/')
        );
        let expected_model_id = managed_model_id(desired);
        let found = value.as_array().is_some_and(|models| {
            models.iter().any(|model| {
                model.get("id").and_then(Value::as_str) == Some(expected_model_id)
                    && model.get("name").is_none()
                    && model.get("url").and_then(Value::as_str) == Some(expected_url.as_str())
                    && model.get("supportsToolCall").and_then(Value::as_bool)
                        == Some(desired.supports_tools)
                    && model.get("supportsImages").and_then(Value::as_bool) == Some(true)
                    && model.get("supportsReasoning").and_then(Value::as_bool) == Some(true)
                    && model.get("onlyReasoning").and_then(Value::as_bool) == Some(false)
                    && model
                        .pointer("/reasoning/canDisableThinking")
                        .and_then(Value::as_bool)
                        == Some(true)
            })
        });
        if found {
            Ok(())
        } else {
            Err(CommandError::new(
                "agent_config_not_applied",
                "WorkBuddy 未读取到 AT-Switch 管理模型",
            ))
        }
    }

    fn activation_required(&self, detection: &AgentDetection) -> bool {
        !managed_model_selected(detection)
    }

    fn native_activation_required(&self, detection: &AgentDetection) -> bool {
        managed_model_selected(detection)
    }
}

fn managed_model_selected(detection: &AgentDetection) -> bool {
    let Ok(Some(expected)) = configured_managed_session_model(detection) else {
        return false;
    };
    latest_session_selection(detection)
        .ok()
        .flatten()
        .and_then(|selection| selection.model)
        .as_deref()
        == Some(expected.as_str())
}

pub(crate) fn pause_for_runtime_update(
    detection: &AgentDetection,
) -> AppResult<lifecycle::DesktopAppPause> {
    lifecycle::pause_for_config_update(detection)
}

pub(crate) fn read_new_task_selection(detection: &AgentDetection) -> AppResult<NewTaskSelection> {
    let user_id = workbuddy_user_id(detection)?;
    let key = new_task_model_key(&user_id);
    let mut database = open_local_storage_database(detection)?;
    let value = database.get(&key).map(|value| value.to_vec());
    close_local_storage_database(database)?;
    Ok(NewTaskSelection {
        scope_id: format!("{NEW_TASK_SCOPE_PREFIX}{user_id}"),
        user_id,
        value,
    })
}

pub(crate) fn apply_new_task_selection(
    detection: &AgentDetection,
    user_id: &str,
    value: Option<&[u8]>,
) -> AppResult<()> {
    let key = new_task_model_key(user_id);
    let mut database = open_local_storage_database(detection)?;
    let change = match value {
        Some(value) => database.put(&key, value),
        None => database.delete(&key),
    };
    if let Err(error) = change {
        log::warn!("WorkBuddy new-task model could not be updated: {error}");
        return Err(CommandError::new(
            "workbuddy_new_task_model_write_failed",
            "无法更新 WorkBuddy 的新会话模型选择",
        )
        .with_recovery("请完全退出 WorkBuddy 后重试；AT-Switch 不会直接改写已锁定的存储。"));
    }
    database.flush().map_err(|error| {
        log::warn!("WorkBuddy new-task model could not be flushed: {error}");
        CommandError::new(
            "workbuddy_new_task_model_flush_failed",
            "WorkBuddy 的新会话模型选择未能安全保存",
        )
        .with_recovery("请重试；若问题持续，请先退出 WorkBuddy 再执行切换。")
    })?;
    close_local_storage_database(database)
}

pub(crate) fn managed_session_model(detection: &AgentDetection) -> AppResult<String> {
    configured_managed_session_model(detection)?.ok_or_else(|| {
        CommandError::new(
            "workbuddy_managed_model_missing",
            "WorkBuddy 未读取到 AT-Switch 管理模型",
        )
    })
}

pub(crate) fn managed_new_task_selection(detection: &AgentDetection) -> AppResult<Vec<u8>> {
    let mut value = vec![1_u8];
    value.extend(
        serde_json::to_vec(&json!({
            "id": managed_session_model(detection)?,
            "isThinking": true
        }))
        .map_err(|_| CommandError::internal("无法生成 WorkBuddy 新会话模型选择"))?,
    );
    Ok(value)
}

pub(crate) fn is_managed_new_task_selection(
    detection: &AgentDetection,
    value: Option<&[u8]>,
) -> bool {
    let Some(value) = value.and_then(|value| value.strip_prefix(&[1_u8])) else {
        return false;
    };
    let Ok(selection) = serde_json::from_slice::<Value>(value) else {
        return false;
    };
    managed_session_model(detection)
        .ok()
        .is_some_and(|managed| selection.get("id").and_then(Value::as_str) == Some(&managed))
}

pub(crate) fn decode_runtime_selection(value: Option<&str>) -> AppResult<Option<Vec<u8>>> {
    value
        .map(|value| {
            hex::decode(value).map_err(|_| {
                CommandError::new(
                    "workbuddy_runtime_snapshot_invalid",
                    "WorkBuddy 切换前的模型选择快照无法解析",
                )
                .with_recovery("请恢复默认配置后重新应用；AT-Switch 不会猜测原选择。")
            })
        })
        .transpose()
}

pub(crate) fn encode_runtime_selection(value: Option<&[u8]>) -> Option<String> {
    value.map(hex::encode)
}

pub(crate) fn latest_session_selection(
    detection: &AgentDetection,
) -> AppResult<Option<SessionSelection>> {
    let Some(connection) = open_session_database(detection, false)? else {
        return Ok(None);
    };
    connection
        .query_row(
            r#"
            SELECT id, model
            FROM sessions
            WHERE deleted_at IS NULL
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            [],
            |row| {
                Ok(SessionSelection {
                    id: row.get(0)?,
                    model: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn managed_session_selections(
    detection: &AgentDetection,
) -> AppResult<Vec<SessionSelection>> {
    let Some(managed_session_model) = configured_managed_session_model(detection)? else {
        return Ok(Vec::new());
    };
    let Some(connection) = open_session_database(detection, false)? else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        r#"
        SELECT id, model
        FROM sessions
        WHERE deleted_at IS NULL AND model = ?1
        ORDER BY updated_at DESC
        "#,
    )?;
    let rows = statement.query_map([managed_session_model], |row| {
        Ok(SessionSelection {
            id: row.get(0)?,
            model: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn apply_session_changes(
    detection: &AgentDetection,
    changes: &[SessionModelChange],
) -> AppResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let mut connection = open_session_database(detection, true)?.ok_or_else(|| {
        CommandError::new(
            "workbuddy_session_store_missing",
            "未找到 WorkBuddy 会话数据库",
        )
        .with_recovery("请先启动 WorkBuddy 并建立一个会话，然后回到 AT-Switch 重试。")
    })?;
    let transaction = connection.transaction()?;
    for change in changes {
        let updated = transaction.execute(
            "UPDATE sessions SET model = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![change.model.as_deref(), &change.id],
        )?;
        if updated != 1 {
            return Err(CommandError::new(
                "workbuddy_session_changed",
                "WorkBuddy 会话在切换过程中发生了变化",
            )
            .with_recovery("请保持 WorkBuddy 当前会话不变，然后重试。"));
        }
    }
    transaction.commit()?;
    Ok(())
}

fn open_session_database(
    detection: &AgentDetection,
    writable: bool,
) -> AppResult<Option<Connection>> {
    let Some(path) = session_database_path(detection) else {
        return Ok(None);
    };
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    // 与 open_local_storage_database 一致：WorkBuddy 退出后 Windows 文件
    // 系统可能仍持有 SQLite 句柄几百毫秒，重试 5 次给内核释放时间。
    let mut last_error: Option<String> = None;
    for attempt in 1..=5 {
        match Connection::open_with_flags(&path, flags) {
            Ok(connection) => {
                connection.busy_timeout(Duration::from_secs(5))?;
                validate_session_schema(&connection)?;
                return Ok(Some(connection));
            }
            Err(error) => {
                let error_msg = error.to_string();
                log::warn!(
                    "WorkBuddy session database open attempt {attempt}/5 failed: {error_msg}"
                );
                last_error = Some(error_msg);
                if attempt < 5 {
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        }
    }
    let error = last_error.unwrap_or_else(|| "unknown error".to_owned());
    log::warn!("WorkBuddy session database could not be opened after 5 retries: {error}");
    Err(CommandError::new(
        "workbuddy_session_store_unavailable",
        "无法打开 WorkBuddy 会话数据库",
    )
    .with_recovery("请关闭 WorkBuddy 后重试；若仍失败，请确认当前用户拥有配置目录权限。"))
}

fn session_database_path(detection: &AgentDetection) -> Option<PathBuf> {
    let state_dir = detection.config_path.as_ref()?.parent()?;
    ["workbuddy.db", "codebuddy.db"]
        .into_iter()
        .map(|name| state_dir.join(name))
        .find(|path| path.is_file())
}

fn workbuddy_state_dir(detection: &AgentDetection) -> AppResult<&Path> {
    detection
        .config_path
        .as_deref()
        .and_then(Path::parent)
        .ok_or_else(|| {
            CommandError::new(
                "workbuddy_state_path_missing",
                "未找到 WorkBuddy 本地状态目录",
            )
        })
}

fn local_storage_database_path(detection: &AgentDetection) -> AppResult<PathBuf> {
    let path = workbuddy_state_dir(detection)?
        .join("app")
        .join("session")
        .join("Local Storage")
        .join("leveldb");
    if path.join("CURRENT").is_file() {
        Ok(path)
    } else {
        Err(CommandError::new(
            "workbuddy_local_storage_missing",
            "WorkBuddy 尚未建立本地模型选择存储",
        )
        .with_recovery("请先登录并完整启动一次 WorkBuddy，然后回到 AT-Switch 重试。"))
    }
}

fn open_local_storage_database(detection: &AgentDetection) -> AppResult<LevelDb> {
    let path = local_storage_database_path(detection)?;
    // WorkBuddy 退出后，Windows 文件系统可能仍持有 LevelDB LOCK 文件
    // 句柄几百毫秒。重试 5 次，每次间隔 300ms（总等待 1.5s），给内核
    // 足够时间释放句柄。macOS 上没有这个问题（kill -TERM 走完
    // applicationShouldTerminate 后句柄立即释放），重试也无害。
    let mut last_error: Option<String> = None;
    for attempt in 1..=5 {
        let options = LevelDbOptions {
            create_if_missing: false,
            ..LevelDbOptions::default()
        };
        match LevelDb::open(&path, options) {
            Ok(database) => return Ok(database),
            Err(error) => {
                let error_msg = error.to_string();
                log::warn!("WorkBuddy local storage open attempt {attempt}/5 failed: {error_msg}");
                last_error = Some(error_msg);
                if attempt < 5 {
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        }
    }
    let error = last_error.unwrap_or_else(|| "unknown error".to_owned());
    log::warn!("WorkBuddy local storage could not be opened after 5 retries: {error}");
    Err(CommandError::new(
        "workbuddy_local_storage_unavailable",
        "无法打开 WorkBuddy 的模型选择存储",
    )
    .with_recovery("请完全退出 WorkBuddy 后重试；AT-Switch 会在写入完成后自动重新打开。"))
}

fn close_local_storage_database(mut database: LevelDb) -> AppResult<()> {
    database.close().map_err(|error| {
        log::warn!("WorkBuddy local storage could not be closed cleanly: {error}");
        CommandError::new(
            "workbuddy_local_storage_close_failed",
            "WorkBuddy 的模型选择存储未能安全关闭",
        )
        .with_recovery("请重试；若问题持续，请先备份 WorkBuddy 数据目录。")
    })
}

fn workbuddy_user_id(detection: &AgentDetection) -> AppResult<String> {
    let state_dir = workbuddy_state_dir(detection)?;
    let sessions_path = state_dir.join("app").join("sessions.json");
    if let Ok(bytes) = fs::read(&sessions_path) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(user_id) = value
                .get("sessions")
                .and_then(Value::as_array)
                .and_then(|sessions| {
                    sessions
                        .iter()
                        .find_map(|session| session.get("userId").and_then(Value::as_str))
                })
                .filter(|user_id| !user_id.trim().is_empty())
            {
                return Ok(user_id.to_owned());
            }
        }
    }

    let settings_path = state_dir.join("settings.json");
    if let Ok(bytes) = fs::read(&settings_path) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(user_id) = value
                .pointer("/claw/legacyOwnerUid")
                .and_then(Value::as_str)
                .filter(|user_id| !user_id.trim().is_empty())
            {
                return Ok(user_id.to_owned());
            }
        }
    }

    Err(
        CommandError::new("workbuddy_user_missing", "未识别到 WorkBuddy 当前用户")
            .with_recovery("请先登录 WorkBuddy 并建立一个会话，然后回到 AT-Switch 重试。"),
    )
}

fn new_task_model_key(user_id: &str) -> Vec<u8> {
    let mut key = CHROMIUM_FILE_ORIGIN_PREFIX.to_vec();
    key.extend_from_slice(NEW_TASK_MODEL_KEY_PREFIX.as_bytes());
    key.extend_from_slice(user_id.as_bytes());
    key
}

fn validate_session_schema(connection: &Connection) -> AppResult<()> {
    let mut statement = connection.prepare("PRAGMA table_info(sessions)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<HashSet<_>, _>>()?;
    if ["id", "model", "updated_at", "deleted_at"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "workbuddy_session_schema_unsupported",
            "当前 WorkBuddy 会话数据库版本暂不兼容自动模型切换",
        )
        .with_recovery("请更新 AT-Switch，或暂时不要覆盖 WorkBuddy 的会话模型。"))
    }
}

fn build_models_config(
    existing: Option<Vec<u8>>,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<Vec<u8>> {
    let mut models = if let Some(bytes) = existing {
        serde_json::from_slice::<Value>(&bytes).map_err(|_| {
            CommandError::new("agent_config_unparseable", "WorkBuddy 模型配置无法解析")
        })?
    } else {
        Value::Array(Vec::new())
    };
    let list = models.as_array_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "WorkBuddy 模型配置不是受支持的数组结构",
        )
    })?;

    list.retain(|item| !is_managed_model(item));
    let managed_model_id = managed_model_id(desired);
    list.push(json!({
        // Use the provider's real model ID in both modes. Proxy routing already
        // stores the selected upstream model separately, so a synthetic ID is
        // unnecessary and would make direct requests invalid.
        "id": managed_model_id,
        "vendor": desired.provider_name,
        "atSwitchManaged": true,
        "apiKey": desired.credential,
        "url": format!(
            "{}/chat/completions",
            desired.base_url.trim_end_matches('/')
        ),
        "supportsToolCall": desired.supports_tools,
        "supportsImages": true,
        "supportsReasoning": true,
        "onlyReasoning": false,
        "reasoning": {
            "effort": "",
            "defaultEffort": "",
            "supportedEfforts": [],
            "summary": "",
            "canDisableThinking": true
        }
    }));
    serde_json::to_vec_pretty(&models)
        .map_err(|_| CommandError::internal("无法生成 WorkBuddy 模型配置"))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn build_native_models_config(existing: Option<Vec<u8>>) -> AppResult<Vec<u8>> {
    let mut models = if let Some(bytes) = existing {
        serde_json::from_slice::<Value>(&bytes).map_err(|_| {
            CommandError::new("agent_config_unparseable", "WorkBuddy 模型配置无法解析")
        })?
    } else {
        Value::Array(Vec::new())
    };
    let list = models.as_array_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "WorkBuddy 模型配置不是受支持的数组结构",
        )
    })?;
    list.retain(|item| !is_managed_model(item));
    serde_json::to_vec_pretty(&models)
        .map_err(|_| CommandError::internal("无法生成 WorkBuddy 原始模型配置"))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn is_managed_model(item: &Value) -> bool {
    item.get("id").and_then(Value::as_str) == Some(LEGACY_MANAGED_MODEL_ID)
        || item.get("atSwitchManaged").and_then(Value::as_bool) == Some(true)
        || item
            .get("vendor")
            .and_then(Value::as_str)
            .is_some_and(|vendor| vendor.starts_with(MANAGED_VENDOR_PREFIX))
}

fn managed_model_id<'binding>(desired: &DesiredAgentBinding<'binding>) -> &'binding str {
    desired.model_id
}

fn configured_managed_session_model(detection: &AgentDetection) -> AppResult<Option<String>> {
    let path = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new("agent_config_path_missing", "未找到 WorkBuddy 配置路径")
    })?;
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| CommandError::new("agent_config_unparseable", "WorkBuddy 模型配置无法解析"))?;
    Ok(value.as_array().and_then(|models| {
        models
            .iter()
            .find(|model| is_managed_model(model))
            .and_then(|model| model.get("id"))
            .and_then(Value::as_str)
            .map(|id| format!("custom-local:{id}"))
    }))
}

fn probe_models(path: &PathBuf) -> AppResult<()> {
    let value: Value = serde_json::from_slice(&fs::read(path)?).map_err(|_| {
        CommandError::new(
            "agent_config_unparseable",
            "WorkBuddy 模型配置不是有效 JSON",
        )
    })?;
    let models = value.as_array().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "WorkBuddy 模型配置不是受支持的数组结构",
        )
    })?;
    if models.iter().any(|model| {
        !model.is_object()
            || model.get("id").is_some_and(|value| !value.is_string())
            || model.get("url").is_some_and(|value| !value.is_string())
    }) {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "WorkBuddy 模型配置字段类型不受支持",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentConfigHealth, AgentInstallStatus};

    fn desired<'a>(
        mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
        credential: &'a str,
    ) -> DesiredAgentBinding<'a> {
        DesiredAgentBinding {
            mode,
            provider_name: "蒙云智算",
            model_id: "glm-test",
            supports_tools: true,
            upstream_protocol,
            source_protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "http://127.0.0.1:54187/v1",
            credential,
        }
    }

    fn detection_with_session_database() -> (tempfile::TempDir, AgentDetection) {
        let temp = tempfile::tempdir().expect("temp");
        let state_dir = temp.path().join(".workbuddy");
        fs::create_dir_all(&state_dir).expect("state dir");
        let config_path = state_dir.join("models.json");
        fs::write(&config_path, b"[]\n").expect("models");
        let connection = Connection::open(state_dir.join("workbuddy.db")).expect("database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    model TEXT,
                    updated_at INTEGER NOT NULL,
                    deleted_at INTEGER
                );
                INSERT INTO sessions(id, model, updated_at) VALUES
                    ('older', 'auto', 1),
                    ('current', 'custom-local:user-model', 2);
                "#,
            )
            .expect("schema");
        let detection = AgentDetection {
            id: "workbuddy",
            display_name: "WorkBuddy",
            installation: None,
            config_path: Some(config_path),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };
        fs::write(
            detection.config_path.as_ref().expect("config path"),
            build_models_config(
                None,
                &desired(
                    AgentBindingMode::Proxy,
                    ApiProtocol::OpenaiChatCompletions,
                    "local-token",
                ),
            )
            .expect("managed model"),
        )
        .expect("write managed model");
        (temp, detection)
    }

    fn seed_new_task_storage(detection: &AgentDetection, user_id: &str, value: Option<&[u8]>) {
        let state_dir = detection
            .config_path
            .as_ref()
            .and_then(|path| path.parent())
            .expect("state dir");
        let app_dir = state_dir.join("app");
        fs::create_dir_all(&app_dir).expect("app dir");
        fs::write(
            app_dir.join("sessions.json"),
            serde_json::to_vec(&json!({
                "version": 1,
                "sessions": [{
                    "conversationId": "current",
                    "userId": user_id
                }]
            }))
            .expect("sessions json"),
        )
        .expect("sessions");

        let database_path = app_dir
            .join("session")
            .join("Local Storage")
            .join("leveldb");
        fs::create_dir_all(&database_path).expect("local storage dir");
        let options = LevelDbOptions {
            create_if_missing: true,
            ..LevelDbOptions::default()
        };
        let mut database = LevelDb::open(&database_path, options).expect("leveldb");
        if let Some(value) = value {
            database
                .put(&new_task_model_key(user_id), value)
                .expect("seed selection");
        }
        database.close().expect("close");
    }

    #[test]
    fn replaces_only_managed_models_and_preserves_user_entries() {
        let existing = r#"[
          {"id":"user-model","name":"User","vendor":"Other","custom":true},
          {"id":"old","name":"Old","vendor":"AT-Switch · Old"}
        ]"#;
        let output = build_models_config(
            Some(existing.as_bytes().to_vec()),
            &desired(
                AgentBindingMode::Proxy,
                ApiProtocol::AnthropicMessages,
                "local-token",
            ),
        )
        .expect("config");
        let value: Value = serde_json::from_slice(&output).expect("json");
        let models = value.as_array().expect("array");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["custom"], true);
        assert_eq!(models[1]["id"], "glm-test");
        assert!(models[1].get("name").is_none());
        assert_eq!(
            models[1]["url"],
            "http://127.0.0.1:54187/v1/chat/completions"
        );
        assert_eq!(models[1]["apiKey"], "local-token");
    }

    #[test]
    fn proxy_and_direct_modes_both_write_the_real_model_id() {
        for mode in [AgentBindingMode::Proxy, AgentBindingMode::Direct] {
            let output = build_models_config(
                None,
                &desired(mode, ApiProtocol::OpenaiChatCompletions, "credential"),
            )
            .expect("config");
            let value: Value = serde_json::from_slice(&output).expect("json");
            let model = value
                .as_array()
                .and_then(|models| models.first())
                .expect("managed model");
            assert_eq!(model["id"], "glm-test");
            assert!(model.get("name").is_none());
            assert_ne!(model["id"], LEGACY_MANAGED_MODEL_ID);
        }
    }

    #[test]
    fn managed_model_enables_images_and_optional_reasoning_by_default() {
        let output = build_models_config(
            None,
            &desired(
                AgentBindingMode::Direct,
                ApiProtocol::OpenaiChatCompletions,
                "credential",
            ),
        )
        .expect("config");
        let value: Value = serde_json::from_slice(&output).expect("json");
        let model = value
            .as_array()
            .and_then(|models| models.first())
            .expect("managed model");

        assert_eq!(model["supportsToolCall"], true);
        assert_eq!(model["supportsImages"], true);
        assert_eq!(model["supportsReasoning"], true);
        assert_eq!(model["onlyReasoning"], false);
        assert_eq!(model["reasoning"]["canDisableThinking"], true);
        assert_eq!(model["reasoning"]["defaultEffort"], "");
        assert_eq!(
            model["reasoning"]["supportedEfforts"],
            Value::Array(Vec::new())
        );
    }

    #[test]
    fn replaces_and_verifies_the_managed_model_on_every_switch() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("models.json");
        fs::write(&path, b"[]\n").expect("seed");
        let detection = AgentDetection {
            id: "workbuddy",
            display_name: "WorkBuddy",
            installation: None,
            config_path: Some(path.clone()),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };

        fs::write(
            &path,
            WorkBuddyAdapter
                .build_config(
                    &detection,
                    &desired(
                        AgentBindingMode::Proxy,
                        ApiProtocol::OpenaiChatCompletions,
                        "local-token",
                    ),
                )
                .expect("first config"),
        )
        .expect("write first");

        let next = DesiredAgentBinding {
            model_id: "glm-next",
            ..desired(
                AgentBindingMode::Proxy,
                ApiProtocol::OpenaiChatCompletions,
                "local-token",
            )
        };
        fs::write(
            &path,
            WorkBuddyAdapter
                .build_config(&detection, &next)
                .expect("second config"),
        )
        .expect("write second");
        WorkBuddyAdapter
            .verify_config(&detection, &next)
            .expect("verify second");

        let root: Value = serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
        let managed = root
            .as_array()
            .expect("models")
            .iter()
            .filter(|entry| is_managed_model(entry))
            .collect::<Vec<_>>();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0]["id"], "glm-next");
        assert!(managed[0].get("name").is_none());
    }

    #[test]
    fn native_mode_removes_only_at_switch_models() {
        let output = build_native_models_config(Some(
            r#"[
              {"id":"user-model","vendor":"Other"},
              {"id":"at-switch","vendor":"AT-Switch · Test"},
              {"id":"legacy","vendor":"AT-Switch · Legacy"}
            ]"#
            .as_bytes()
            .to_vec(),
        ))
        .expect("native config");
        let value: Value = serde_json::from_slice(&output).expect("json");
        let models = value.as_array().expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "user-model");
    }

    #[test]
    fn direct_mode_rejects_a_non_chat_upstream() {
        let error = WorkBuddyAdapter
            .validate_binding(&desired(
                AgentBindingMode::Direct,
                ApiProtocol::OpenaiResponses,
                "upstream-key",
            ))
            .expect_err("must reject");
        assert_eq!(error.code, "workbuddy_direct_protocol_unsupported");
    }

    #[test]
    fn switches_and_restores_the_current_session_model_transactionally() {
        let (_temp, detection) = detection_with_session_database();
        let before = latest_session_selection(&detection)
            .expect("latest")
            .expect("session");
        assert_eq!(before.id, "current");
        assert_eq!(before.model.as_deref(), Some("custom-local:user-model"));
        let managed_model = managed_session_model(&detection).expect("managed model");

        apply_session_changes(
            &detection,
            &[SessionModelChange {
                id: before.id.clone(),
                model: Some(managed_model),
            }],
        )
        .expect("switch");
        assert_eq!(
            managed_session_selections(&detection)
                .expect("managed")
                .len(),
            1
        );

        apply_session_changes(
            &detection,
            &[SessionModelChange {
                id: before.id,
                model: before.model,
            }],
        )
        .expect("restore");
        assert!(managed_session_selections(&detection)
            .expect("managed")
            .is_empty());
    }

    #[test]
    fn switches_and_restores_the_new_task_model_selection() {
        let (_temp, detection) = detection_with_session_database();
        let original = b"\x01{\"id\":\"custom-local:user-model\",\"isThinking\":true}";
        seed_new_task_storage(&detection, "user-1", Some(original));

        let before = read_new_task_selection(&detection).expect("read original");
        assert_eq!(before.scope_id, "new-task:user-1");
        assert_eq!(before.value.as_deref(), Some(original.as_slice()));
        let managed_selection = managed_new_task_selection(&detection).expect("managed selection");
        let managed_json: Value =
            serde_json::from_slice(&managed_selection[1..]).expect("managed selection json");
        assert_eq!(managed_json["isThinking"], true);

        apply_new_task_selection(&detection, &before.user_id, Some(&managed_selection))
            .expect("apply managed");
        let managed = read_new_task_selection(&detection).expect("read managed");
        assert!(is_managed_new_task_selection(
            &detection,
            managed.value.as_deref()
        ));

        apply_new_task_selection(&detection, &before.user_id, before.value.as_deref())
            .expect("restore original");
        assert_eq!(
            read_new_task_selection(&detection)
                .expect("read restored")
                .value,
            before.value
        );
    }

    #[test]
    fn managed_new_task_detection_accepts_thinking_turned_off_by_the_user() {
        let (_temp, detection) = detection_with_session_database();
        let selection = [
            &[1_u8][..],
            br#"{"id":"custom-local:glm-test","isThinking":false}"#,
        ]
        .concat();

        assert!(is_managed_new_task_selection(&detection, Some(&selection)));
    }

    #[test]
    fn runtime_selection_encoding_preserves_raw_chromium_value() {
        let original = b"\x01{\"id\":\"auto\",\"isThinking\":false}";
        let encoded = encode_runtime_selection(Some(original)).expect("encoded");
        assert_eq!(
            decode_runtime_selection(Some(&encoded)).expect("decoded"),
            Some(original.to_vec())
        );
        assert_eq!(decode_runtime_selection(None).expect("none"), None);
    }
}
