use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};
use crate::services::{write_atomic, BaselineSnapshot};

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct CodeBuddyAdapter;

const MANAGED_VENDOR_PREFIX: &str = "AT-Switch · ";
const WORKSPACE_SCOPE_PREFIX: &str = "workspace:";
const HISTORY_SCOPE_PREFIX: &str = "history:";
const CODEBUDDY_CN_DATA_DIR: &str = "CodeBuddy CN";
const CODEBUDDY_EXTENSION_DATA_DIR: &str = "CodeBuddyExtension/Data";
const CODEBUDDY_CN_EXTENSION_KEY: &str = "Tencent-Cloud.coding-copilot";
const SELECTED_MODE_KEY: &str = "chatSelectedMode";
const SELECTED_MODEL_MAP_KEY: &str = "chatSelectedModelMapV2";

/// 全局 state.vscdb 里的字段名。CodeBuddy UI 优先读全局级别的
/// chatSelectedModelGlobalMap 来决定默认勾选，workspace 级别的
/// chatSelectedModelMapV2 只是工作区覆盖。如果只写 workspace 不写
/// 全局，UI 会读全局的旧值并勾选旧模型。macOS 上可能因为启动时序
/// 不同而碰巧用了 workspace 值，Windows 上 UI 严格读全局值。
const GLOBAL_SELECTED_MODEL_MAP_KEY: &str = "chatSelectedModelGlobalMap";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorkspaceSelection {
    pub scope_id: String,
    database_path: PathBuf,
    extension_key: String,
    pub selected_mode: String,
    model_map: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConversationSelection {
    pub scope_id: String,
    index_path: PathBuf,
    conversation_id: String,
    selected_mode: String,
    model_map: Option<Value>,
}

impl AgentAdapter for CodeBuddyAdapter {
    fn id(&self) -> &'static str {
        "codebuddy"
    }

    fn display_name(&self) -> &'static str {
        "CodeBuddy"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["CodeBuddy CN.app"],
            &["com.tencent.codebuddycn"],
            &[
                "Programs/CodeBuddy CN/CodeBuddy CN.exe",
                "Programs/CodeBuddy CN/CodeBuddy.exe",
                "CodeBuddy CN/CodeBuddy CN.exe",
                "CodeBuddy CN/CodeBuddy.exe",
                "Tencent/CodeBuddy CN/CodeBuddy CN.exe",
                "Tencent/CodeBuddy CN/CodeBuddy.exe",
            ],
        );
        let mut detection = AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            context.home.join(".codebuddy/models.json"),
            probe_models,
            true,
        );
        detection.runtime_data_dir = Some(context.application_data_dir.join(CODEBUDDY_CN_DATA_DIR));
        detection
    }

    fn source_protocol(
        &self,
        _desired_mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode == AgentBindingMode::Direct
            && desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "codebuddy_direct_protocol_unsupported",
                "CodeBuddy 直连模式要求 Provider 支持 OpenAI Chat API",
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
        let path = config_path(detection)?;
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
        let path = config_path(detection)?;
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
        let path = config_path(detection)?;
        let root = parse_root(&fs::read(path)?)?;
        let expected_url = chat_completions_url(desired.base_url);
        let found = root
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| {
                models.iter().any(|model| {
                    is_managed_model(model)
                        && model.get("id").and_then(Value::as_str) == Some(desired.model_id)
                        && model.get("url").and_then(Value::as_str) == Some(expected_url.as_str())
                        && model.get("supportsToolCall").and_then(Value::as_bool)
                            == Some(desired.supports_tools)
                })
            });
        if found {
            Ok(())
        } else {
            Err(CommandError::new(
                "agent_config_not_applied",
                "CodeBuddy 未读取到目标 AT-Switch 模型配置",
            ))
        }
    }
}

fn config_path(detection: &AgentDetection) -> AppResult<&PathBuf> {
    detection
        .config_path
        .as_ref()
        .ok_or_else(|| CommandError::new("agent_config_path_missing", "未找到 CodeBuddy 配置路径"))
}

fn build_models_config(
    existing: Option<Vec<u8>>,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<Vec<u8>> {
    let mut root = existing
        .as_deref()
        .map(parse_root)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let managed_model_ids = {
        let models = models_array_mut(&mut root)?;
        let managed_model_ids = models
            .iter()
            .filter(|model| is_managed_model(model))
            .filter_map(|model| {
                model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();
        models.retain(|model| !is_managed_model(model));
        for model in models.iter_mut() {
            if model
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            {
                if let Some(id) = model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                {
                    if let Some(obj) = model.as_object_mut() {
                        obj.insert("name".to_owned(), Value::String(id));
                    }
                }
            }
        }
        models.push(json!({
            "id": desired.model_id,
            "name": desired.provider_name,
            "vendor": desired.provider_name,
            "url": chat_completions_url(desired.base_url),
            "apiKey": desired.credential,
            "supportsToolCall": desired.supports_tools,
            "supportsImages": true,
            "supportsReasoning": true,
            "atSwitchManaged": true
        }));
        managed_model_ids
    };
    update_available_models(&mut root, &managed_model_ids, Some(desired.model_id))?;
    serialize_root(&root, "无法生成 CodeBuddy 模型配置")
}

pub(crate) fn current_conversation_selections(
    detection: &AgentDetection,
) -> AppResult<Vec<ConversationSelection>> {
    conversation_selections(detection, true)
}

pub(crate) fn desired_conversation_selections(
    previous: &[ConversationSelection],
    model_id: &str,
) -> AppResult<Vec<ConversationSelection>> {
    previous
        .iter()
        .map(|selection| {
            let mut model_map = parse_conversation_model_map(selection.model_map.as_ref())?;
            model_map.insert(
                selection.selected_mode.clone(),
                Value::String(model_id.to_owned()),
            );
            let mut desired = selection.clone();
            desired.model_map = Some(Value::Object(model_map));
            Ok(desired)
        })
        .collect()
}

pub(crate) fn apply_conversation_selections(changes: &[ConversationSelection]) -> AppResult<()> {
    for change in changes {
        let bytes = fs::read(&change.index_path)?;
        let mut index = parse_history_index(&bytes)?;
        let conversation = history_conversation_mut(&mut index, &change.conversation_id)
            .ok_or_else(|| {
                CommandError::new(
                    "codebuddy_conversation_state_changed",
                    "CodeBuddy 当前会话在切换过程中发生了变化",
                )
                .with_recovery("请保持 CodeBuddy CN 完全退出，然后重试。")
            })?;
        let conversation = conversation.as_object_mut().ok_or_else(|| {
            CommandError::new(
                "codebuddy_conversation_state_invalid",
                "CodeBuddy 会话状态不是有效对象",
            )
        })?;
        match &change.model_map {
            Some(model_map) => {
                conversation.insert("modelMap".to_owned(), model_map.clone());
            }
            None => {
                conversation.remove("modelMap");
            }
        }
        let mut serialized = serde_json::to_vec_pretty(&index)
            .map_err(|_| CommandError::internal("无法保存 CodeBuddy 当前会话模型选择"))?;
        serialized.push(b'\n');
        write_atomic(&change.index_path, &serialized)?;
    }
    Ok(())
}

pub(crate) fn verify_current_conversation_model(
    detection: &AgentDetection,
    expected_model_id: &str,
) -> AppResult<()> {
    let selections = current_conversation_selections(detection)?;
    if selections.iter().all(|selection| {
        selected_conversation_model(selection).as_deref() == Some(expected_model_id)
    }) {
        Ok(())
    } else {
        Err(CommandError::new(
            "codebuddy_conversation_model_not_applied",
            "CodeBuddy CN 的当前会话仍在使用其他模型",
        )
        .with_recovery("请重新点击目标模型的“切换”，AT-Switch 会同步当前会话后再重启。"))
    }
}

pub(crate) fn managed_conversation_selections(
    detection: &AgentDetection,
) -> AppResult<Vec<ConversationSelection>> {
    let Some(managed_model_id) = configured_managed_model_id(detection)? else {
        return Ok(Vec::new());
    };
    Ok(conversation_selections(detection, false)?
        .into_iter()
        .filter(|selection| {
            selected_conversation_model(selection).as_deref() == Some(managed_model_id.as_str())
        })
        .collect())
}

pub(crate) fn encode_conversation_selection(
    selection: &ConversationSelection,
) -> AppResult<String> {
    serde_json::to_string(&selection.model_map)
        .map_err(|_| CommandError::internal("无法保存 CodeBuddy 原会话模型选择快照"))
}

pub(crate) fn restore_conversation_selection(
    current: &ConversationSelection,
    encoded: Option<&str>,
) -> AppResult<ConversationSelection> {
    let model_map = decode_model_map_snapshot(encoded)?;
    let mut restored = current.clone();
    restored.model_map = model_map;
    Ok(restored)
}

fn conversation_selections(
    detection: &AgentDetection,
    current_only: bool,
) -> AppResult<Vec<ConversationSelection>> {
    let history_data_root = codebuddy_extension_data_root(detection)?;
    if !history_data_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut index_paths = Vec::new();
    for account in sorted_directories(&history_data_root)? {
        let ide_root = account.join("CodeBuddyIDE");
        if !ide_root.is_dir() {
            continue;
        }
        for ide_user in sorted_directories(&ide_root)? {
            let history_root = ide_user.join("history");
            if !history_root.is_dir() {
                continue;
            }
            for workspace in sorted_directories(&history_root)? {
                let index_path = workspace.join("index.json");
                if index_path.is_file() {
                    index_paths.push((history_data_root.clone(), index_path));
                }
            }
        }
    }

    let mut selections = Vec::new();
    for (history_root, index_path) in index_paths {
        let index = parse_history_index(&fs::read(&index_path)?)?;
        let current_id = index
            .get("current")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let conversations = index
            .get("conversations")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CommandError::new(
                    "codebuddy_history_index_invalid",
                    "CodeBuddy 会话历史索引缺少 conversations 数组",
                )
            })?;
        for conversation in conversations {
            let Some(conversation_id) = conversation.get("id").and_then(Value::as_str) else {
                continue;
            };
            if current_only && current_id != Some(conversation_id) {
                continue;
            }
            let relative_index = index_path
                .strip_prefix(&history_root)
                .map_err(|_| {
                    CommandError::new(
                        "codebuddy_history_identity_invalid",
                        "CodeBuddy 会话历史目录无法识别",
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            let selected_mode = conversation
                .get("type")
                .and_then(Value::as_str)
                .filter(|mode| !mode.trim().is_empty())
                .unwrap_or("craft")
                .to_owned();
            selections.push(ConversationSelection {
                scope_id: format!("{HISTORY_SCOPE_PREFIX}{relative_index}:{conversation_id}"),
                index_path: index_path.clone(),
                conversation_id: conversation_id.to_owned(),
                selected_mode,
                model_map: conversation.get("modelMap").cloned(),
            });
        }
    }
    Ok(selections)
}

fn codebuddy_extension_data_root(detection: &AgentDetection) -> AppResult<PathBuf> {
    let application_data_dir = codebuddy_data_dir(detection)?.parent().ok_or_else(|| {
        CommandError::new(
            "codebuddy_runtime_data_path_invalid",
            "CodeBuddy CN 数据目录无法识别",
        )
    })?;
    Ok(application_data_dir.join(CODEBUDDY_EXTENSION_DATA_DIR))
}

fn sorted_directories(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut directories = fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    Ok(directories)
}

fn parse_history_index(bytes: &[u8]) -> AppResult<Value> {
    let index: Value = serde_json::from_slice(bytes).map_err(|_| {
        CommandError::new(
            "codebuddy_history_index_invalid",
            "CodeBuddy 会话历史索引无法解析",
        )
        .with_recovery("请保持 CodeBuddy CN 完全退出，然后重试。")
    })?;
    if index.is_object() && index.get("conversations").is_some_and(Value::is_array) {
        Ok(index)
    } else {
        Err(CommandError::new(
            "codebuddy_history_index_invalid",
            "CodeBuddy 会话历史索引格式不受支持",
        ))
    }
}

fn history_conversation_mut<'a>(
    index: &'a mut Value,
    conversation_id: &str,
) -> Option<&'a mut Value> {
    index
        .get_mut("conversations")?
        .as_array_mut()?
        .iter_mut()
        .find(|conversation| {
            conversation.get("id").and_then(Value::as_str) == Some(conversation_id)
        })
}

fn parse_conversation_model_map(
    value: Option<&Value>,
) -> AppResult<serde_json::Map<String, Value>> {
    match value {
        None | Some(Value::Null) => Ok(serde_json::Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => Err(CommandError::new(
            "codebuddy_conversation_model_map_invalid",
            "CodeBuddy 当前会话的模型选择格式不受支持",
        )
        .with_recovery("请在 CodeBuddy CN 中重新选择一次模型后重试。")),
    }
}

fn selected_conversation_model(selection: &ConversationSelection) -> Option<String> {
    parse_conversation_model_map(selection.model_map.as_ref())
        .ok()?
        .get(&selection.selected_mode)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn build_native_models_config(existing: Option<Vec<u8>>) -> AppResult<Vec<u8>> {
    let mut root = existing
        .as_deref()
        .map(parse_root)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let managed_model_ids = {
        let models = models_array_mut(&mut root)?;
        let managed_model_ids = models
            .iter()
            .filter(|model| is_managed_model(model))
            .filter_map(|model| {
                model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();
        models.retain(|model| !is_managed_model(model));
        managed_model_ids
    };
    update_available_models(&mut root, &managed_model_ids, None)?;
    serialize_root(&root, "无法生成 CodeBuddy 原始模型配置")
}

fn update_available_models(
    root: &mut Value,
    retired_model_ids: &[String],
    next_model_id: Option<&str>,
) -> AppResult<()> {
    let Some(available_models) = root.get_mut("availableModels") else {
        return Ok(());
    };
    let available_models = available_models.as_array_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "CodeBuddy 模型配置的 availableModels 字段不是数组",
        )
    })?;
    available_models.retain(|value| {
        value
            .as_str()
            .is_none_or(|model_id| !retired_model_ids.iter().any(|retired| retired == model_id))
    });
    if let Some(next_model_id) = next_model_id {
        if !available_models
            .iter()
            .any(|value| value.as_str() == Some(next_model_id))
        {
            available_models.push(Value::String(next_model_id.to_owned()));
        }
    }
    Ok(())
}

pub(crate) fn workspace_selections(
    detection: &AgentDetection,
) -> AppResult<Vec<WorkspaceSelection>> {
    let workspace_root = codebuddy_data_dir(detection)?.join("User/workspaceStorage");
    if !workspace_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut state_databases = fs::read_dir(&workspace_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("state.vscdb"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    state_databases.sort();

    let mut selections = Vec::new();
    for database_path in state_databases {
        let connection = open_workspace_database(&database_path, false)?;
        let Some(serialized) = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                [CODEBUDDY_CN_EXTENSION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            continue;
        };
        let state = parse_workspace_state(&serialized)?;
        let selected_mode = state
            .get(SELECTED_MODE_KEY)
            .and_then(Value::as_str)
            .filter(|mode| !mode.trim().is_empty())
            .unwrap_or("craft")
            .to_owned();
        let workspace_id = database_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                CommandError::new(
                    "codebuddy_workspace_identity_invalid",
                    "CodeBuddy 工作区存储目录无法识别",
                )
            })?;
        selections.push(WorkspaceSelection {
            scope_id: format!("{WORKSPACE_SCOPE_PREFIX}{workspace_id}"),
            database_path,
            extension_key: CODEBUDDY_CN_EXTENSION_KEY.to_owned(),
            selected_mode,
            model_map: state.get(SELECTED_MODEL_MAP_KEY).cloned(),
        });
    }
    Ok(selections)
}

pub(crate) fn desired_workspace_selections(
    previous: &[WorkspaceSelection],
    model_id: &str,
) -> AppResult<Vec<WorkspaceSelection>> {
    previous
        .iter()
        .map(|selection| {
            let mut model_map = parse_model_map(selection.model_map.as_ref())?;
            model_map.insert(
                selection.selected_mode.clone(),
                Value::String(model_id.to_owned()),
            );
            let mut desired = selection.clone();
            desired.model_map = Some(Value::String(
                serde_json::to_string(&Value::Object(model_map))
                    .map_err(|_| CommandError::internal("无法生成 CodeBuddy 工作区模型选择"))?,
            ));
            Ok(desired)
        })
        .collect()
}

pub(crate) fn apply_workspace_selections(changes: &[WorkspaceSelection]) -> AppResult<()> {
    for change in changes {
        let mut connection = open_workspace_database(&change.database_path, true)?;
        let transaction = connection.transaction()?;
        let serialized: String = transaction
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                [&change.extension_key],
                |row| row.get(0),
            )
            .map_err(|error| {
                log::warn!("CodeBuddy workspace state could not be read before update: {error}");
                CommandError::new(
                    "codebuddy_workspace_state_changed",
                    "CodeBuddy 工作区状态在切换过程中发生了变化",
                )
                .with_recovery("请保持 CodeBuddy 完全退出，然后重试。")
            })?;
        let mut state = parse_workspace_state(&serialized)?;
        let object = state.as_object_mut().ok_or_else(|| {
            CommandError::new(
                "codebuddy_workspace_state_invalid",
                "CodeBuddy 工作区状态不是有效对象",
            )
        })?;
        match &change.model_map {
            Some(model_map) => {
                object.insert(SELECTED_MODEL_MAP_KEY.to_owned(), model_map.clone());
            }
            None => {
                object.remove(SELECTED_MODEL_MAP_KEY);
            }
        }
        let updated = transaction.execute(
            "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
            params![
                serde_json::to_string(&state).map_err(|_| {
                    CommandError::internal("无法保存 CodeBuddy 工作区模型选择")
                })?,
                &change.extension_key
            ],
        )?;
        if updated != 1 {
            return Err(CommandError::new(
                "codebuddy_workspace_state_changed",
                "CodeBuddy 工作区状态在切换过程中发生了变化",
            )
            .with_recovery("请保持 CodeBuddy 完全退出，然后重试。"));
        }
        transaction.commit()?;
    }
    Ok(())
}

/// 写入全局 state.vscdb 的 `chatSelectedModelGlobalMap` 字段。
/// CodeBuddy UI 启动时优先读全局级别的这个字段来决定默认勾选，
/// 如果只写 workspace 级别的 `chatSelectedModelMapV2`，UI 仍会
/// 读全局的旧值并勾选旧模型。
pub(crate) fn apply_global_model_selection(
    detection: &AgentDetection,
    selected_mode: &str,
    model_id: &str,
) -> AppResult<()> {
    let global_db = codebuddy_data_dir(detection)?.join("User/globalStorage/state.vscdb");
    if !global_db.exists() {
        // 全局 state.vscdb 不存在说明 CodeBuddy 从未启动过，跳过即可。
        return Ok(());
    }
    let mut connection = open_workspace_database(&global_db, true)?;
    let transaction = connection.transaction()?;
    let serialized: String = transaction
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [CODEBUDDY_CN_EXTENSION_KEY],
            |row| row.get(0),
        )
        .map_err(|error| {
            log::warn!("CodeBuddy global state could not be read: {error}");
            CommandError::new(
                "codebuddy_global_state_changed",
                "CodeBuddy 全局状态在切换过程中发生了变化",
            )
            .with_recovery("请保持 CodeBuddy 完全退出，然后重试。")
        })?;
    let mut state: Value = serde_json::from_str(&serialized).map_err(|_| {
        CommandError::new(
            "codebuddy_global_state_invalid",
            "CodeBuddy 全局状态不是有效对象",
        )
    })?;
    let object = state.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "codebuddy_global_state_invalid",
            "CodeBuddy 全局状态不是有效对象",
        )
    })?;
    // 解析现有的 chatSelectedModelGlobalMap（是个 JSON 字符串），更新
    // selected_mode 对应的 model_id，再序列化回字符串存入。
    let current_map_str = object
        .get(GLOBAL_SELECTED_MODEL_MAP_KEY)
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let mut model_map: serde_json::Map<String, Value> =
        serde_json::from_str(current_map_str).unwrap_or_default();
    model_map.insert(selected_mode.to_owned(), Value::String(model_id.to_owned()));
    let updated_map = serde_json::to_string(&Value::Object(model_map))
        .map_err(|_| CommandError::internal("无法序列化全局模型选择"))?;
    object.insert(
        GLOBAL_SELECTED_MODEL_MAP_KEY.to_owned(),
        Value::String(updated_map),
    );
    let updated = transaction.execute(
        "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
        params![
            serde_json::to_string(&state)
                .map_err(|_| CommandError::internal("无法保存 CodeBuddy 全局模型选择"))?,
            CODEBUDDY_CN_EXTENSION_KEY
        ],
    )?;
    if updated != 1 {
        return Err(CommandError::new(
            "codebuddy_global_state_changed",
            "CodeBuddy 全局状态在切换过程中发生了变化",
        )
        .with_recovery("请保持 CodeBuddy 完全退出，然后重试。"));
    }
    transaction.commit()?;
    Ok(())
}

/// 清除全局 state.vscdb 里 AT-Switch 写入的模型选择，让 CodeBuddy
/// 回到自己的默认选中逻辑。
pub(crate) fn clear_global_model_selection(
    detection: &AgentDetection,
    selected_mode: &str,
) -> AppResult<()> {
    let global_db = codebuddy_data_dir(detection)?.join("User/globalStorage/state.vscdb");
    if !global_db.exists() {
        return Ok(());
    }
    let mut connection = open_workspace_database(&global_db, true)?;
    let transaction = connection.transaction()?;
    let serialized: String = transaction
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [CODEBUDDY_CN_EXTENSION_KEY],
            |row| row.get(0),
        )
        .map_err(|error| {
            log::warn!("CodeBuddy global state could not be read: {error}");
            CommandError::new(
                "codebuddy_global_state_changed",
                "CodeBuddy 全局状态在恢复过程中发生了变化",
            )
            .with_recovery("请保持 CodeBuddy 完全退出，然后重试。")
        })?;
    let mut state: Value = serde_json::from_str(&serialized).map_err(|_| {
        CommandError::new(
            "codebuddy_global_state_invalid",
            "CodeBuddy 全局状态不是有效对象",
        )
    })?;
    let object = state.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "codebuddy_global_state_invalid",
            "CodeBuddy 全局状态不是有效对象",
        )
    })?;
    if let Some(map_str) = object
        .get(GLOBAL_SELECTED_MODEL_MAP_KEY)
        .and_then(Value::as_str)
    {
        let mut model_map: serde_json::Map<String, Value> =
            serde_json::from_str(map_str).unwrap_or_default();
        model_map.remove(selected_mode);
        let updated_map = serde_json::to_string(&Value::Object(model_map))
            .map_err(|_| CommandError::internal("无法序列化全局模型选择"))?;
        object.insert(
            GLOBAL_SELECTED_MODEL_MAP_KEY.to_owned(),
            Value::String(updated_map),
        );
    }
    let updated = transaction.execute(
        "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
        params![
            serde_json::to_string(&state)
                .map_err(|_| CommandError::internal("无法保存 CodeBuddy 全局模型选择"))?,
            CODEBUDDY_CN_EXTENSION_KEY
        ],
    )?;
    if updated != 1 {
        return Err(CommandError::new(
            "codebuddy_global_state_changed",
            "CodeBuddy 全局状态在恢复过程中发生了变化",
        )
        .with_recovery("请保持 CodeBuddy 完全退出，然后重试。"));
    }
    transaction.commit()?;
    Ok(())
}

pub(crate) fn verify_workspace_model(
    detection: &AgentDetection,
    expected_model_id: &str,
) -> AppResult<()> {
    let selections = workspace_selections(detection)?;
    if selections.is_empty() {
        return Err(CommandError::new(
            "codebuddy_workspace_state_missing",
            "CodeBuddy 尚未生成可同步的工作区模型状态",
        )
        .with_recovery("请先打开 CodeBuddy CN 的任意项目，完全退出后再重试切换。"));
    }
    if selections
        .iter()
        .all(|selection| selected_workspace_model(selection).as_deref() == Some(expected_model_id))
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "codebuddy_workspace_model_not_applied",
            "CodeBuddy CN 的工作区仍在使用其他模型",
        )
        .with_recovery("请重新点击目标模型的“切换”，AT-Switch 会同步所有已有工作区。"))
    }
}

pub(crate) fn managed_workspace_selections(
    detection: &AgentDetection,
) -> AppResult<Vec<WorkspaceSelection>> {
    let Some(managed_model_id) = configured_managed_model_id(detection)? else {
        return Ok(Vec::new());
    };
    Ok(workspace_selections(detection)?
        .into_iter()
        .filter(|selection| {
            selected_workspace_model(selection).as_deref() == Some(managed_model_id.as_str())
        })
        .collect())
}

pub(crate) fn encode_workspace_selection(selection: &WorkspaceSelection) -> AppResult<String> {
    serde_json::to_string(&selection.model_map)
        .map_err(|_| CommandError::internal("无法保存 CodeBuddy 原模型选择快照"))
}

pub(crate) fn restore_workspace_selection(
    current: &WorkspaceSelection,
    encoded: Option<&str>,
) -> AppResult<WorkspaceSelection> {
    let model_map = decode_model_map_snapshot(encoded)?;
    let mut restored = current.clone();
    restored.model_map = model_map;
    Ok(restored)
}

fn decode_model_map_snapshot(encoded: Option<&str>) -> AppResult<Option<Value>> {
    Ok(encoded
        .map(|value| {
            serde_json::from_str::<Option<Value>>(value).map_err(|_| {
                CommandError::new(
                    "codebuddy_runtime_snapshot_invalid",
                    "CodeBuddy 切换前的模型选择快照无法解析",
                )
                .with_recovery("请重新应用目标模型后再恢复原始配置。")
            })
        })
        .transpose()?
        .flatten())
}

fn selected_workspace_model(selection: &WorkspaceSelection) -> Option<String> {
    parse_model_map(selection.model_map.as_ref())
        .ok()?
        .get(&selection.selected_mode)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn parse_model_map(value: Option<&Value>) -> AppResult<serde_json::Map<String, Value>> {
    let parsed = match value {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(Value::String(serialized)) if serialized.trim().is_empty() => {
            Value::Object(serde_json::Map::new())
        }
        Some(Value::String(serialized)) => serde_json::from_str(serialized).map_err(|_| {
            CommandError::new(
                "codebuddy_workspace_model_map_invalid",
                "CodeBuddy 工作区模型选择无法解析",
            )
            .with_recovery("请在 CodeBuddy CN 中重新选择一次模型后重试。")
        })?,
        Some(Value::Object(map)) => Value::Object(map.clone()),
        Some(_) => {
            return Err(CommandError::new(
                "codebuddy_workspace_model_map_invalid",
                "CodeBuddy 工作区模型选择格式不受支持",
            )
            .with_recovery("请在 CodeBuddy CN 中重新选择一次模型后重试。"));
        }
    };
    parsed.as_object().cloned().ok_or_else(|| {
        CommandError::new(
            "codebuddy_workspace_model_map_invalid",
            "CodeBuddy 工作区模型选择不是有效对象",
        )
    })
}

fn configured_managed_model_id(detection: &AgentDetection) -> AppResult<Option<String>> {
    let path = config_path(detection)?;
    if !path.is_file() {
        return Ok(None);
    }
    let root = parse_root(&fs::read(path)?)?;
    Ok(root
        .get("models")
        .and_then(Value::as_array)
        .and_then(|models| {
            models.iter().find_map(|model| {
                is_managed_model(model)
                    .then(|| managed_model_selection_id(model))
                    .flatten()
            })
        }))
}

fn managed_model_selection_id(model: &Value) -> Option<String> {
    let model_id = model.get("id").and_then(Value::as_str)?;
    let display_name = model.get("name").and_then(Value::as_str);
    Some(model_selection_id(display_name, model_id))
}

pub(crate) fn model_selection_id(display_name: Option<&str>, model_id: &str) -> String {
    let display_name = display_name.map(str::trim).filter(|name| !name.is_empty());
    match display_name {
        Some(name) if name != model_id => format!("{name}:{model_id}"),
        _ => model_id.to_owned(),
    }
}

fn codebuddy_data_dir(detection: &AgentDetection) -> AppResult<&Path> {
    detection.runtime_data_dir.as_deref().ok_or_else(|| {
        CommandError::new(
            "codebuddy_runtime_data_path_missing",
            "未找到 CodeBuddy CN 工作区数据目录",
        )
    })
}

fn open_workspace_database(path: &Path, writable: bool) -> AppResult<Connection> {
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    // CodeBuddy 退出后，Windows 文件系统可能仍持有 SQLite 文件句柄
    // 几百毫秒（taskkill /F 是强制终止，不给应用优雅清理资源的机会）。
    // 重试 5 次，每次间隔 300ms（总等待 1.5s），给内核时间释放句柄。
    // macOS 上 kill -TERM 走完 applicationShouldTerminate 后句柄立即
    // 释放，重试无害。
    let mut last_error: Option<String> = None;
    for attempt in 1..=5 {
        match Connection::open_with_flags(path, flags) {
            Ok(connection) => {
                connection.busy_timeout(Duration::from_secs(5))?;
                return Ok(connection);
            }
            Err(error) => {
                let error_msg = error.to_string();
                log::warn!(
                    "CodeBuddy workspace database open attempt {attempt}/5 failed: {error_msg}"
                );
                last_error = Some(error_msg);
                if attempt < 5 {
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        }
    }
    let error = last_error.unwrap_or_else(|| "unknown error".to_owned());
    log::warn!("CodeBuddy workspace database could not be opened after 5 retries: {error}");
    Err(CommandError::new(
        "codebuddy_workspace_store_unavailable",
        "无法打开 CodeBuddy 工作区模型状态",
    )
    .with_recovery("请完全退出 CodeBuddy CN 后重试。"))
}

fn parse_workspace_state(serialized: &str) -> AppResult<Value> {
    let state: Value = serde_json::from_str(serialized).map_err(|_| {
        CommandError::new(
            "codebuddy_workspace_state_invalid",
            "CodeBuddy 工作区状态无法解析",
        )
        .with_recovery("请在 CodeBuddy CN 中重新打开该项目后重试。")
    })?;
    if state.is_object() {
        Ok(state)
    } else {
        Err(CommandError::new(
            "codebuddy_workspace_state_invalid",
            "CodeBuddy 工作区状态不是有效对象",
        ))
    }
}

fn parse_root(bytes: &[u8]) -> AppResult<Value> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        CommandError::new(
            "agent_config_unparseable",
            "CodeBuddy 模型配置不是有效 JSON",
        )
    })?;
    if !value.is_object() {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "CodeBuddy 模型配置根节点不是对象",
        ));
    }
    if value.get("models").is_some_and(|models| !models.is_array()) {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "CodeBuddy 模型配置的 models 字段不是数组",
        ));
    }
    Ok(value)
}

fn models_array_mut(root: &mut Value) -> AppResult<&mut Vec<Value>> {
    let object = root.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "CodeBuddy 模型配置根节点不是对象",
        )
    })?;
    object
        .entry("models")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            CommandError::new(
                "agent_config_shape_unsupported",
                "CodeBuddy 模型配置的 models 字段不是数组",
            )
        })
}

fn is_managed_model(model: &Value) -> bool {
    model.get("atSwitchManaged").and_then(Value::as_bool) == Some(true)
        || model
            .get("vendor")
            .and_then(Value::as_str)
            .is_some_and(|vendor| vendor.starts_with(MANAGED_VENDOR_PREFIX))
}

fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

fn serialize_root(root: &Value, message: &str) -> AppResult<Vec<u8>> {
    serde_json::to_vec_pretty(root)
        .map_err(|_| CommandError::internal(message))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn probe_models(path: &PathBuf) -> AppResult<()> {
    let root = parse_root(&fs::read(path)?)?;
    if root
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                !model.is_object()
                    || model.get("id").is_some_and(|value| !value.is_string())
                    || model.get("url").is_some_and(|value| !value.is_string())
            })
        })
    {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "CodeBuddy 模型配置字段类型不受支持",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentConfigHealth, AgentInstallStatus};

    fn desired(mode: AgentBindingMode, credential: &str) -> DesiredAgentBinding<'_> {
        DesiredAgentBinding {
            mode,
            provider_name: "蒙云智算",
            model_id: "glm-test",
            supports_tools: true,
            upstream_protocol: ApiProtocol::OpenaiChatCompletions,
            source_protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "http://127.0.0.1:54187/v1",
            credential,
        }
    }

    fn detection(path: PathBuf) -> AgentDetection {
        AgentDetection {
            id: "codebuddy",
            display_name: "CodeBuddy",
            installation: None,
            config_path: Some(path),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        }
    }

    #[test]
    fn preserves_root_metadata_and_user_models() {
        let existing = br#"{
          "schemaVersion": 2,
          "availableModels": ["user-model"],
          "models": [
            {"id":"user-model","name":"User","vendor":"Other","custom":true},
            {"id":"old","name":"Old","vendor":"AT-Switch \u00b7 Old"}
          ]
        }"#;
        let output = build_models_config(
            Some(existing.to_vec()),
            &desired(AgentBindingMode::Proxy, "local-token"),
        )
        .expect("config");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(root["schemaVersion"], 2);
        assert_eq!(root["availableModels"][0], "user-model");
        assert_eq!(root["availableModels"][1], "glm-test");
        let models = root["models"].as_array().expect("models");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["custom"], true);
        assert_eq!(models[1]["id"], "glm-test");
        assert_eq!(
            models[1]["url"],
            "http://127.0.0.1:54187/v1/chat/completions"
        );
    }

    #[test]
    fn direct_and_proxy_modes_use_the_real_model_id() {
        for mode in [AgentBindingMode::Proxy, AgentBindingMode::Direct] {
            let output = build_models_config(None, &desired(mode, "credential")).expect("config");
            let root: Value = serde_json::from_slice(&output).expect("json");
            assert_eq!(root["models"][0]["id"], "glm-test");
            assert_eq!(root["models"][0]["name"], "蒙云智算");
            assert_eq!(root["models"][0]["vendor"], "蒙云智算");
        }
    }

    #[test]
    fn custom_model_selection_uses_the_display_name_and_real_model_id() {
        assert_eq!(
            model_selection_id(Some("蒙云智算"), "deepseek-v4-flash"),
            "蒙云智算:deepseek-v4-flash"
        );
        assert_eq!(
            model_selection_id(Some("deepseek-v4-flash"), "deepseek-v4-flash"),
            "deepseek-v4-flash"
        );
        assert_eq!(
            model_selection_id(None, "deepseek-v4-flash"),
            "deepseek-v4-flash"
        );
    }

    #[test]
    fn repeated_switch_replaces_the_managed_model_and_allowlist_entry() {
        let first = build_models_config(
            Some(
                br#"{
                  "availableModels": ["user-model"],
                  "models": [{"id":"user-model","vendor":"Other"}]
                }"#
                .to_vec(),
            ),
            &desired(AgentBindingMode::Direct, "first-key"),
        )
        .expect("first config");
        let second_desired = DesiredAgentBinding {
            model_id: "glm-next",
            ..desired(AgentBindingMode::Direct, "second-key")
        };
        let second = build_models_config(Some(first), &second_desired).expect("second config");
        let root: Value = serde_json::from_slice(&second).expect("json");
        assert_eq!(root["models"].as_array().expect("models").len(), 2);
        assert_eq!(root["models"][1]["id"], "glm-next");
        assert_eq!(root["availableModels"], json!(["user-model", "glm-next"]));
    }

    #[test]
    fn verifies_the_written_model() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("models.json");
        let desired = desired(AgentBindingMode::Proxy, "local-token");
        fs::write(&path, build_models_config(None, &desired).expect("config")).expect("write");
        CodeBuddyAdapter
            .verify_config(&detection(path), &desired)
            .expect("verified");
    }

    #[test]
    fn native_restore_removes_only_managed_models() {
        let output = build_native_models_config(Some(
            br#"{
              "availableModels": ["user-model", "managed"],
              "models": [
                {"id":"user-model","vendor":"Other"},
                {"id":"managed","vendor":"AT-Switch \u00b7 Test"}
              ]
            }"#
            .to_vec(),
        ))
        .expect("native");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(root["models"].as_array().expect("models").len(), 1);
        assert_eq!(root["models"][0]["id"], "user-model");
        assert_eq!(root["availableModels"], json!(["user-model"]));
    }

    fn seed_workspace_state(
        runtime_data_dir: &Path,
        workspace_id: &str,
        selected_mode: &str,
        model_map: Option<Value>,
    ) -> PathBuf {
        let workspace_dir = runtime_data_dir
            .join("User/workspaceStorage")
            .join(workspace_id);
        fs::create_dir_all(&workspace_dir).expect("workspace directory");
        let database_path = workspace_dir.join("state.vscdb");
        let connection = Connection::open(&database_path).expect("workspace database");
        connection
            .execute_batch(
                "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
            )
            .expect("schema");
        let mut state = json!({
            SELECTED_MODE_KEY: selected_mode,
            "userSetting": {"preserved": true}
        });
        if let Some(model_map) = model_map {
            state
                .as_object_mut()
                .expect("object")
                .insert(SELECTED_MODEL_MAP_KEY.to_owned(), model_map);
        }
        connection
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params![CODEBUDDY_CN_EXTENSION_KEY, state.to_string()],
            )
            .expect("state");
        database_path
    }

    fn workspace_detection(config_path: PathBuf, runtime_data_dir: PathBuf) -> AgentDetection {
        let mut detection = detection(config_path);
        detection.runtime_data_dir = Some(runtime_data_dir);
        detection
    }

    fn seed_conversation_history(
        runtime_data_dir: &Path,
        workspace_id: &str,
        conversation_id: &str,
        model_map: Option<Value>,
    ) -> PathBuf {
        let history_dir = runtime_data_dir
            .parent()
            .expect("application data")
            .join(CODEBUDDY_EXTENSION_DATA_DIR)
            .join("account/CodeBuddyIDE/user/history")
            .join(workspace_id);
        fs::create_dir_all(&history_dir).expect("history directory");
        let index_path = history_dir.join("index.json");
        let mut conversation = json!({
            "id": conversation_id,
            "type": "craft",
            "name": "preserved title"
        });
        if let Some(model_map) = model_map {
            conversation
                .as_object_mut()
                .expect("conversation")
                .insert("modelMap".to_owned(), model_map);
        }
        fs::write(
            &index_path,
            serde_json::to_vec_pretty(&json!({
                "current": conversation_id,
                "conversations": [conversation],
                "preserved": true
            }))
            .expect("history json"),
        )
        .expect("history index");
        index_path
    }

    #[test]
    fn synchronizes_each_workspace_mode_and_restores_the_exact_previous_map() {
        let temp = tempfile::tempdir().expect("temp");
        let runtime_data_dir = temp.path().join(CODEBUDDY_CN_DATA_DIR);
        let first_database = seed_workspace_state(
            &runtime_data_dir,
            "workspace-a",
            "craft",
            Some(Value::String(
                json!({"craft":"glm-old","ask":"ask-old"}).to_string(),
            )),
        );
        seed_workspace_state(&runtime_data_dir, "workspace-b", "ask", None);
        let config_path = temp.path().join("models.json");
        fs::write(
            &config_path,
            build_models_config(None, &desired(AgentBindingMode::Direct, "test-key"))
                .expect("models"),
        )
        .expect("models file");
        let detection = workspace_detection(config_path, runtime_data_dir);

        let previous = workspace_selections(&detection).expect("previous");
        let changes =
            desired_workspace_selections(&previous, "glm-test").expect("desired selections");
        apply_workspace_selections(&changes).expect("apply");
        verify_workspace_model(&detection, "glm-test").expect("verified");

        let connection = Connection::open(first_database).expect("workspace database");
        let serialized: String = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                [CODEBUDDY_CN_EXTENSION_KEY],
                |row| row.get(0),
            )
            .expect("state");
        let state: Value = serde_json::from_str(&serialized).expect("json");
        assert_eq!(state["userSetting"]["preserved"], true);
        let model_map: Value =
            serde_json::from_str(state[SELECTED_MODEL_MAP_KEY].as_str().expect("map"))
                .expect("model map");
        assert_eq!(model_map["craft"], "glm-test");
        assert_eq!(model_map["ask"], "ask-old");

        apply_workspace_selections(&previous).expect("restore");
        let restored = workspace_selections(&detection).expect("restored selections");
        assert_eq!(restored, previous);
    }

    #[test]
    fn runtime_snapshot_preserves_an_absent_workspace_model_map() {
        let selection = WorkspaceSelection {
            scope_id: "workspace:test".to_owned(),
            database_path: PathBuf::from("state.vscdb"),
            extension_key: CODEBUDDY_CN_EXTENSION_KEY.to_owned(),
            selected_mode: "craft".to_owned(),
            model_map: None,
        };
        let encoded = encode_workspace_selection(&selection).expect("encoded");
        let mut changed = selection.clone();
        changed.model_map = Some(Value::String("{\"craft\":\"managed\"}".to_owned()));
        let restored =
            restore_workspace_selection(&changed, Some(&encoded)).expect("restored selection");
        assert_eq!(restored.model_map, None);
    }

    #[test]
    fn synchronizes_current_conversation_and_restores_its_exact_model_map() {
        let temp = tempfile::tempdir().expect("temp");
        let runtime_data_dir = temp.path().join("ApplicationData/CodeBuddy CN");
        fs::create_dir_all(&runtime_data_dir).expect("runtime data");
        let index_path = seed_conversation_history(
            &runtime_data_dir,
            "workspace-a",
            "conversation-a",
            Some(json!({"craft":"kimi-k3-1","ask":"ask-old"})),
        );
        let detection = workspace_detection(temp.path().join("models.json"), runtime_data_dir);

        let previous = current_conversation_selections(&detection).expect("previous");
        assert_eq!(previous.len(), 1);
        let desired =
            desired_conversation_selections(&previous, "deepseek-v4-pro").expect("desired");
        apply_conversation_selections(&desired).expect("apply");
        verify_current_conversation_model(&detection, "deepseek-v4-pro").expect("verified");

        let index: Value =
            serde_json::from_slice(&fs::read(&index_path).expect("history")).expect("json");
        assert_eq!(index["preserved"], true);
        assert_eq!(index["conversations"][0]["name"], "preserved title");
        assert_eq!(
            index["conversations"][0]["modelMap"]["craft"],
            "deepseek-v4-pro"
        );
        assert_eq!(index["conversations"][0]["modelMap"]["ask"], "ask-old");

        let encoded = encode_conversation_selection(&previous[0]).expect("snapshot");
        let restored = restore_conversation_selection(&desired[0], Some(&encoded))
            .expect("restored selection");
        apply_conversation_selections(&[restored]).expect("restore");
        let restored = current_conversation_selections(&detection).expect("restored");
        assert_eq!(restored, previous);
    }

    #[test]
    fn direct_mode_rejects_non_chat_provider() {
        let desired = DesiredAgentBinding {
            upstream_protocol: ApiProtocol::OpenaiResponses,
            ..desired(AgentBindingMode::Direct, "upstream-key")
        };
        let error = CodeBuddyAdapter
            .validate_binding(&desired)
            .expect_err("must reject");
        assert_eq!(error.code, "codebuddy_direct_protocol_unsupported");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_codebuddy_cn_and_uses_its_own_configuration_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let applications = temp.path().join("Applications");
        let app = applications.join("CodeBuddy CN.app");
        fs::create_dir_all(app.join("Contents")).expect("app bundle");
        fs::create_dir_all(home.join(".codebuddy")).expect("state directory");
        fs::write(home.join(".codebuddy/models.json"), b"{\"models\":[]}\n").expect("models");

        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".to_owned(),
            plist::Value::String("com.tencent.codebuddycn".to_owned()),
        );
        dictionary.insert(
            "CFBundleShortVersionString".to_owned(),
            plist::Value::String("4.10.4".to_owned()),
        );
        plist::to_file_xml(
            app.join("Contents/Info.plist"),
            &plist::Value::Dictionary(dictionary),
        )
        .expect("plist");

        let context = DiscoveryContext {
            home: home.clone(),
            application_data_dir: home.join("Library/Application Support"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        };
        let detected = CodeBuddyAdapter.detect(&context);

        assert_eq!(detected.install_status, AgentInstallStatus::Installed);
        assert!(matches!(detected.config_health, AgentConfigHealth::Healthy));
        assert_eq!(
            detected.config_path.as_deref(),
            Some(home.join(".codebuddy/models.json").as_path())
        );
        assert_eq!(
            detected
                .installation
                .as_ref()
                .and_then(|installation| installation.version.as_deref()),
            Some("4.10.4")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn detects_codebuddy_cn_in_the_per_user_windows_installation_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let local_app_data = temp.path().join("LocalAppData");
        let executable = local_app_data.join("Programs/CodeBuddy CN/CodeBuddy CN.exe");
        fs::create_dir_all(executable.parent().expect("parent")).expect("app directory");
        fs::write(&executable, b"test executable").expect("executable");
        fs::create_dir_all(home.join(".codebuddy")).expect("state directory");
        fs::write(home.join(".codebuddy/models.json"), b"{\"models\":[]}\n").expect("models");
        let context = DiscoveryContext {
            home: home.clone(),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: Some(local_app_data),
            program_files: Vec::new(),
        };

        let detected = CodeBuddyAdapter.detect(&context);

        assert_eq!(detected.install_status, AgentInstallStatus::Installed);
        assert_eq!(
            detected
                .installation
                .as_ref()
                .map(|installation| installation.path.as_path()),
            Some(executable.as_path())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn does_not_merge_the_international_codebuddy_bundle_into_the_cn_agent() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let applications = temp.path().join("Applications");
        let app = applications.join("CodeBuddy.app");
        fs::create_dir_all(app.join("Contents")).expect("app bundle");

        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".to_owned(),
            plist::Value::String("com.s.codebuddy".to_owned()),
        );
        plist::to_file_xml(
            app.join("Contents/Info.plist"),
            &plist::Value::Dictionary(dictionary),
        )
        .expect("plist");

        let context = DiscoveryContext {
            home: home.clone(),
            application_data_dir: home.join("Library/Application Support"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        };

        let detected = CodeBuddyAdapter.detect(&context);
        assert_eq!(detected.install_status, AgentInstallStatus::NotInstalled);
        assert!(detected.installation.is_none());
    }
}
