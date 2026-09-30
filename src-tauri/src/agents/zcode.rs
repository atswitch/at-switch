use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use rusqlite::{params, Connection};
use rusty_leveldb::{LdbIterator, Options as LevelDbOptions, DB as LevelDb};
use serde_json::{json, Map, Value};

use super::{
    locator::{locate_desktop_app, DiscoveryContext, DiscoveryHints},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::{
    domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError},
    services::BaselineSnapshot,
};

pub struct ZCodeAdapter;

const MANAGED_PROVIDER_ID: &str = "at-switch";
const MANAGED_PROVIDER_NAME: &str = "AT-Switch";
const CONFIG_FILE_NAME: &str = "provider_config.json";

/// ZCode only honours personal providers declared with this group. Models
/// belong to the provider itself via `personalModelIds` / `modelOrder`;
/// declaring them in `manualProviderModelRules` instead demands a full
/// capability block and makes the whole personal file fail to load.
const PERSONAL_PROVIDER_GROUP: &str = "standard-personal";

/// ZCode keeps the *currently selected* model in Chromium local storage, not in
/// the provider config. Rewriting the config alone therefore never changes what
/// the app has selected, so the adapter also updates this record.
const MANAGED_SELECTION_KEY: &[u8] = b"\"providerId\":\"at-switch\",\"modelId\":\"";

/// ZCode pins a model per task in this index; local storage only mirrors it and
/// is rewritten from the index on startup. The task index is the authority.
const TASK_INDEX_FILE: &str = "tasks-index.sqlite";

/// ZCode splits its model catalogue between a bundled/server-delivered builtin
/// release and a personal file. Only the personal file is a write target, so
/// path resolution must never fall back to the bundled asset.
fn resolve_config_path(context: &DiscoveryContext) -> PathBuf {
    let candidates = [
        context
            .home
            .join(".zcode")
            .join("v2")
            .join(CONFIG_FILE_NAME),
        context
            .application_data_dir
            .join("ZCode")
            .join("v2")
            .join(CONFIG_FILE_NAME),
    ];
    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            context
                .home
                .join(".zcode")
                .join("v2")
                .join(CONFIG_FILE_NAME)
        })
}

fn probe_config(path: &PathBuf) -> AppResult<()> {
    let raw = fs::read(path).map_err(|error| {
        CommandError::new(
            "zcode_config_parse_failed",
            format!("无法读取 ZCode 配置：{error}"),
        )
    })?;
    if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    serde_json::from_slice::<Value>(&raw)
        .map(|_| ())
        .map_err(|error| {
            CommandError::new(
                "zcode_config_parse_failed",
                format!("ZCode 配置不是有效的 JSON：{error}"),
            )
            .with_recovery("请在 ZCode 中重新保存一次模型设置，或删除该文件后重启 ZCode。")
        })
}

fn default_config() -> Value {
    json!({
        "schemaVersion": 1,
        "config": {
            "providerConfigRules": { "providerRules": [] },
            "modelConfigRules": { "providerModelRules": [], "manualProviderModelRules": [] }
        }
    })
}

fn read_config(path: &PathBuf) -> AppResult<Value> {
    if !path.exists() {
        return Ok(default_config());
    }
    let raw = fs::read(path)?;
    if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(default_config());
    }
    serde_json::from_slice(&raw).map_err(|error| {
        CommandError::new(
            "zcode_config_parse_failed",
            format!("ZCode 配置解析失败：{error}"),
        )
    })
}

fn config_path(detection: &AgentDetection) -> AppResult<&PathBuf> {
    detection
        .config_path
        .as_ref()
        .ok_or_else(|| CommandError::new("zcode_config_path_missing", "未找到 ZCode 配置路径"))
}

fn parse_failure(field: &str) -> CommandError {
    CommandError::new(
        "zcode_config_parse_failed",
        format!("ZCode 配置的 {field} 结构不符合预期"),
    )
    .with_recovery("请在 ZCode 中重新保存一次模型设置后重试。")
}

fn ensure_object<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> AppResult<&'a mut Map<String, Value>> {
    parent
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| parse_failure(key))
}

fn ensure_array<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> AppResult<&'a mut Vec<Value>> {
    parent
        .entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| parse_failure(key))
}

/// ZCode validates `api.type` against a fixed enum, so the AT-Switch protocol
/// must map onto it instead of being written verbatim.
fn api_type_for(protocol: ApiProtocol) -> AppResult<&'static str> {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => Ok("openai-chat-completions"),
        ApiProtocol::OpenaiResponses => Ok("openai-responses"),
        ApiProtocol::AnthropicMessages => Ok("anthropic-messages"),
    }
}

fn managed_id(value: &Value) -> bool {
    value
        .get("providerId")
        .and_then(Value::as_str)
        .is_some_and(|id| id == MANAGED_PROVIDER_ID)
}

fn upsert_managed(rules: &mut Vec<Value>, entry: Value) {
    if let Some(existing) = rules.iter_mut().find(|rule| managed_id(rule)) {
        *existing = entry;
    } else {
        rules.push(entry);
    }
}

fn remove_managed(rules: &mut Vec<Value>) {
    rules.retain(|rule| !managed_id(rule));
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_owned()
}

impl AgentAdapter for ZCodeAdapter {
    fn id(&self) -> &'static str {
        "zcode"
    }

    fn display_name(&self) -> &'static str {
        "ZCode"
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["dev.zcode.app"],
            windows_relative_paths: &["Programs/ZCode/ZCode.exe", "ZCode/ZCode.exe"],
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["ZCode.app"],
            &["dev.zcode.app"],
            &["Programs/ZCode/ZCode.exe", "ZCode/ZCode.exe"],
        );
        let mut detection = AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            resolve_config_path(context),
            probe_config,
            true,
        );
        // The currently selected model lives in Chromium local storage rather
        // than the config file, so the sync step needs that directory too.
        detection.runtime_data_dir = Some(local_storage_dir(context));
        detection
    }

    fn source_protocol(
        &self,
        desired_mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        match desired_mode {
            AgentBindingMode::Direct => upstream_protocol,
            AgentBindingMode::Proxy => ApiProtocol::OpenaiChatCompletions,
        }
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.base_url.trim().is_empty() {
            return Err(CommandError::new(
                "zcode_base_url_missing",
                "Provider 缺少 Base URL，无法写入 ZCode",
            ));
        }
        api_type_for(desired.source_protocol).map(|_| ())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        self.validate_binding(desired)?;
        let path = config_path(detection)?;
        let mut root = read_config(path)?;
        let api_type = api_type_for(desired.source_protocol)?;
        let base_url = normalize_base_url(desired.base_url);

        let root_map = root
            .as_object_mut()
            .ok_or_else(|| parse_failure("根节点"))?;
        {
            let config = ensure_object(root_map, "config")?;
            {
                let provider_rules = ensure_object(config, "providerConfigRules")?;
                let rules = ensure_array(provider_rules, "providerRules")?;
                // ZCode locks the model per task. Replacing the declared model
                // list on every switch would make previously switched models
                // vanish from the app and break tasks still pinned to them, so
                // keep models already declared for this provider and only reset
                // when the upstream endpoint actually changes.
                let mut declared_models: Vec<String> = Vec::new();
                if let Some(existing) = rules.iter().find(|rule| managed_id(rule)) {
                    let same_endpoint = existing
                        .pointer("/config/api/baseUrl")
                        .and_then(Value::as_str)
                        == Some(base_url.as_str());
                    if same_endpoint {
                        if let Some(ids) = existing
                            .pointer("/config/personalModelIds")
                            .and_then(Value::as_array)
                        {
                            declared_models = ids
                                .iter()
                                .filter_map(|id| id.as_str().map(str::to_owned))
                                .collect();
                        }
                    }
                }
                // ZCode picks `personalModelIds[0]` as the model for a new
                // task — not `modelOrder[0]` and not `defaultModelSelection`.
                // The target must lead the list, while previously declared
                // models stay so existing tasks keep resolving theirs.
                declared_models.retain(|id| id != desired.model_id);
                declared_models.insert(0, desired.model_id.to_owned());
                let model_order = declared_models.clone();
                upsert_managed(
                    rules,
                    json!({
                        "providerId": MANAGED_PROVIDER_ID,
                        "providerName": MANAGED_PROVIDER_NAME,
                        "enabled": true,
                        "config": {
                            "group": PERSONAL_PROVIDER_GROUP,
                            "access": {
                                "type": "api-key",
                                "apiKey": desired.credential
                            },
                            "api": {
                                "type": api_type,
                                "baseUrl": base_url
                            },
                            "personalModelIds": declared_models,
                            "modelOrder": model_order,
                            "visibility": "visible"
                        }
                    }),
                );
            }
            {
                // ZCode renders providers in `providerOrder` sequence; putting
                // the managed one first makes it the default pick in the UI.
                let order = ensure_array(config, "providerOrder")?;
                order.retain(|id| id.as_str() != Some(MANAGED_PROVIDER_ID));
                order.insert(0, json!(MANAGED_PROVIDER_ID));
            }
            config.insert(
                "defaultModelSelection".to_owned(),
                json!({
                    "providerId": MANAGED_PROVIDER_ID,
                    "modelId": desired.model_id
                }),
            );
        }

        serde_json::to_vec_pretty(&root).map_err(|error| {
            CommandError::new(
                "zcode_config_serialize_failed",
                format!("无法生成 ZCode 配置：{error}"),
            )
        })
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        _baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        let path = config_path(detection)?;
        let mut root = read_config(path)?;

        {
            let root_map = root
                .as_object_mut()
                .ok_or_else(|| parse_failure("根节点"))?;
            let config = ensure_object(root_map, "config")?;
            if let Some(provider_rules) = config
                .get_mut("providerConfigRules")
                .and_then(Value::as_object_mut)
            {
                if let Some(rules) = provider_rules
                    .get_mut("providerRules")
                    .and_then(Value::as_array_mut)
                {
                    remove_managed(rules);
                }
            }
            if let Some(order) = config
                .get_mut("providerOrder")
                .and_then(Value::as_array_mut)
            {
                order.retain(|id| id.as_str() != Some(MANAGED_PROVIDER_ID));
            }
            let clear_selection = config.get("defaultModelSelection").is_some_and(managed_id);
            if clear_selection {
                config.remove("defaultModelSelection");
            }
        }

        serde_json::to_vec_pretty(&root).map_err(|error| {
            CommandError::new(
                "zcode_config_serialize_failed",
                format!("无法生成 ZCode 配置：{error}"),
            )
        })
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        self.validate_binding(desired)?;
        let path = config_path(detection)?;
        let root = read_config(path)?;
        let config = root
            .get("config")
            .and_then(Value::as_object)
            .ok_or_else(|| parse_failure("config"))?;

        let provider = config
            .get("providerConfigRules")
            .and_then(Value::as_object)
            .and_then(|rules| rules.get("providerRules"))
            .and_then(Value::as_array)
            .and_then(|rules| rules.iter().find(|rule| managed_id(rule)));

        let model_declared = provider.is_some_and(|provider| {
            provider
                .pointer("/config/personalModelIds")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(desired.model_id)))
        });

        let selection = config
            .get("defaultModelSelection")
            .is_some_and(|selection| {
                managed_id(selection)
                    && selection.get("modelId").and_then(Value::as_str) == Some(desired.model_id)
            });

        let applied = provider.is_some_and(|provider| {
            provider
                .pointer("/config/access/apiKey")
                .and_then(Value::as_str)
                == Some(desired.credential)
                && provider
                    .pointer("/config/api/baseUrl")
                    .and_then(Value::as_str)
                    == Some(normalize_base_url(desired.base_url).as_str())
        }) && model_declared
            && selection;

        if applied {
            Ok(())
        } else {
            Err(CommandError::new(
                "zcode_write_verification_failed",
                "ZCode 配置与目标模型不一致",
            )
            .with_recovery("请重新点击目标模型的“切换”，AT-Switch 会重新写入并校验。"))
        }
    }
}

/// A single local-storage record AT-Switch overwrote, kept so the previous
/// selection can be restored when the apply pipeline rolls back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSelectionChange {
    pub key: Vec<u8>,
    pub previous: Vec<u8>,
}

fn local_storage_dir(context: &DiscoveryContext) -> PathBuf {
    context
        .application_data_dir
        .join("ZCode")
        .join("session")
        .join("Local Storage")
        .join("leveldb")
}

fn local_storage_dir_from_detection(detection: &AgentDetection) -> AppResult<&Path> {
    detection.runtime_data_dir.as_deref().ok_or_else(|| {
        CommandError::new("zcode_local_storage_missing", "未找到 ZCode 的模型选择存储")
            .with_recovery("请完整启动一次 ZCode 后回到 AT-Switch 重试。")
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Rewrites the selected model for AT-Switch-owned selections only, leaving
/// every other local-storage value byte-identical.
fn rewrite_model_selection(value: &[u8], model_id: &str) -> Option<Vec<u8>> {
    let mut rewritten = Vec::with_capacity(value.len() + model_id.len());
    let mut cursor = 0;
    let mut changed = false;
    while let Some(offset) = find_subslice(&value[cursor..], MANAGED_SELECTION_KEY) {
        let start = cursor + offset + MANAGED_SELECTION_KEY.len();
        let end = value[start..].iter().position(|byte| *byte == b'"')?;
        rewritten.extend_from_slice(&value[cursor..start]);
        rewritten.extend_from_slice(model_id.as_bytes());
        cursor = start + end;
        changed = true;
    }
    rewritten.extend_from_slice(&value[cursor..]);
    changed.then_some(rewritten)
}

fn open_local_storage(directory: &Path) -> AppResult<LevelDb> {
    // ZCode releases the LevelDB LOCK shortly after its process exits, so retry
    // briefly instead of failing the whole switch.
    let mut last_error = String::new();
    for attempt in 1..=5 {
        let options = LevelDbOptions {
            create_if_missing: false,
            ..LevelDbOptions::default()
        };
        match LevelDb::open(directory, options) {
            Ok(database) => return Ok(database),
            Err(error) => {
                last_error = error.to_string();
                log::warn!("ZCode local storage open attempt {attempt}/5 failed: {last_error}");
                thread::sleep(Duration::from_millis(300));
            }
        }
    }
    Err(CommandError::new(
        "zcode_local_storage_open_failed",
        format!("无法打开 ZCode 的模型选择存储：{last_error}"),
    )
    .with_recovery("请完全退出 ZCode 后重试；AT-Switch 会在写入完成后自动重新打开。"))
}

fn close_local_storage(mut database: LevelDb) -> AppResult<()> {
    database.close().map_err(|error| {
        CommandError::new(
            "zcode_local_storage_close_failed",
            format!("ZCode 的模型选择存储未能安全关闭：{error}"),
        )
    })
}

/// Points ZCode's stored selection at the model AT-Switch just applied, and
/// returns the previous values so a failed apply can be rolled back.
pub(crate) fn apply_model_selection(
    detection: &AgentDetection,
    model_id: &str,
) -> AppResult<Vec<ModelSelectionChange>> {
    let directory = local_storage_dir_from_detection(detection)?;
    let mut database = open_local_storage(directory)?;

    let pending: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = {
        let mut iterator = database.new_iter().map_err(|error| {
            CommandError::new(
                "zcode_local_storage_read_failed",
                format!("无法读取 ZCode 的模型选择存储：{error}"),
            )
        })?;
        let mut collected = Vec::new();
        while let Some((key, value)) = iterator.next() {
            if let Some(rewritten) = rewrite_model_selection(&value, model_id) {
                collected.push((key, value, rewritten));
            }
        }
        collected
    };

    let mut changes = Vec::with_capacity(pending.len());
    for (key, previous, rewritten) in pending {
        database.put(&key, &rewritten).map_err(|error| {
            CommandError::new(
                "zcode_local_storage_write_failed",
                format!("无法更新 ZCode 的模型选择：{error}"),
            )
        })?;
        changes.push(ModelSelectionChange { key, previous });
    }

    close_local_storage(database)?;
    Ok(changes)
}

/// Puts the recorded selections back exactly as they were.
///
/// Reserved for the rollback path: restoring the native configuration keeps the
/// previous selection available so a failed apply can be undone byte for byte.
#[allow(dead_code)]
pub(crate) fn restore_model_selection(
    detection: &AgentDetection,
    changes: &[ModelSelectionChange],
) -> AppResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let directory = local_storage_dir_from_detection(detection)?;
    let mut database = open_local_storage(directory)?;
    for change in changes {
        database
            .put(&change.key, &change.previous)
            .map_err(|error| {
                CommandError::new(
                    "zcode_local_storage_restore_failed",
                    format!("无法还原 ZCode 的模型选择：{error}"),
                )
            })?;
    }
    close_local_storage(database)
}

/// A task whose pinned model AT-Switch rewrote, kept for rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskModelChange {
    pub task_id: String,
    pub previous: Option<String>,
}

fn task_index_path(detection: &AgentDetection) -> AppResult<PathBuf> {
    config_path(detection)?
        .parent()
        .map(|directory| directory.join(TASK_INDEX_FILE))
        .ok_or_else(|| CommandError::new("zcode_task_index_missing", "未找到 ZCode 的任务索引路径"))
}

fn open_task_index(detection: &AgentDetection) -> AppResult<Option<Connection>> {
    let path = task_index_path(detection)?;
    if !path.is_file() {
        return Ok(None);
    }
    let connection = Connection::open(&path).map_err(|error| {
        CommandError::new(
            "zcode_task_index_open_failed",
            format!("无法打开 ZCode 的任务索引：{error}"),
        )
        .with_recovery("请完全退出 ZCode 后重试。")
    })?;
    Ok(Some(connection))
}

fn task_index_error(error: rusqlite::Error) -> CommandError {
    CommandError::new(
        "zcode_task_index_failed",
        format!("无法更新 ZCode 的任务索引：{error}"),
    )
    .with_recovery("请完全退出 ZCode 后重试。")
}

/// Repoints every AT-Switch-owned task at the model just applied, and returns
/// the previous values so the change can be rolled back.
pub(crate) fn apply_task_model(
    detection: &AgentDetection,
    model_id: &str,
) -> AppResult<Vec<TaskModelChange>> {
    let Some(connection) = open_task_index(detection)? else {
        return Ok(Vec::new());
    };
    let managed = format!("{MANAGED_PROVIDER_ID}/%");
    let previous: Vec<TaskModelChange> = {
        let mut statement = connection
            .prepare("SELECT task_id, model FROM tasks WHERE model LIKE ?1")
            .map_err(task_index_error)?;
        let rows = statement
            .query_map([&managed], |row| {
                Ok(TaskModelChange {
                    task_id: row.get(0)?,
                    previous: row.get(1)?,
                })
            })
            .map_err(task_index_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(task_index_error)?
    };
    let updated = format!("{MANAGED_PROVIDER_ID}/{model_id}");
    connection
        .execute(
            "UPDATE tasks SET model = ?1 WHERE model LIKE ?2",
            params![updated, managed],
        )
        .map_err(task_index_error)?;
    Ok(previous)
}

/// Puts the recorded task models back exactly as they were.
///
/// Reserved for the rollback path, matching `restore_model_selection`.
#[allow(dead_code)]
pub(crate) fn restore_task_model(
    detection: &AgentDetection,
    changes: &[TaskModelChange],
) -> AppResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let Some(connection) = open_task_index(detection)? else {
        return Ok(());
    };
    for change in changes {
        connection
            .execute(
                "UPDATE tasks SET model = ?1 WHERE task_id = ?2",
                params![change.previous, change.task_id],
            )
            .map_err(task_index_error)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "zcode_tests.rs"]
mod tests;
