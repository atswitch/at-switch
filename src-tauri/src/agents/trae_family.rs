use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{params, types::Value as SqlValue, Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use super::{
    locator::{locate_desktop_app, DiscoveryContext, DiscoveryHints},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::domain::{
    AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol, AppResult, CommandError,
};

/// Trae CN (`cn.trae.app`) and TRAE SOLO CN (`cn.trae.solo.app`) share one model
/// schema, so both are served by this parameterised adapter.
///
/// Their `state.vscdb` stores the model catalogue in `model_list_map`, which
/// repeats every entry across several agent segments, and pins the active model
/// in `recent_user_selection_by_agent_label`.
///
/// The `ak` field is an encrypted blob whose key lives in the OS keychain, so
/// AT-Switch never reads or writes credentials: it reports the models the user
/// configured, which is the prerequisite for switching the active one later.
pub struct TraeFamilyAdapter {
    id: &'static str,
    display_name: &'static str,
    macos_apps: &'static [&'static str],
    bundle_ids: &'static [&'static str],
    windows_paths: &'static [&'static str],
    /// Directory name under the per-user application data root.
    data_dir: &'static str,
}

impl TraeFamilyAdapter {
    pub const fn new(
        id: &'static str,
        display_name: &'static str,
        macos_apps: &'static [&'static str],
        bundle_ids: &'static [&'static str],
        windows_paths: &'static [&'static str],
        data_dir: &'static str,
    ) -> Self {
        Self {
            id,
            display_name,
            macos_apps,
            bundle_ids,
            windows_paths,
            data_dir,
        }
    }

    fn state_database(&self, context: &DiscoveryContext) -> PathBuf {
        context
            .application_data_dir
            .join(self.data_dir)
            .join("User")
            .join("globalStorage")
            .join("state.vscdb")
    }
}

/// A model the user configured inside Trae. Only these fields are ever touched;
/// credential fields are deliberately absent from the whitelist.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConfiguredModel {
    display_name: String,
    provider: String,
    base_url: String,
}

const CUSTOM_PROVIDER: &str = "custom_openai_compatible";

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Reads the catalogue and the active selection. Models are de-duplicated
/// across segments by `(display name, provider, base URL)`.
fn read_state(database: &PathBuf) -> AppResult<(Vec<ConfiguredModel>, Option<String>)> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            CommandError::new(
                "trae_db_locked",
                format!("无法读取 Trae 的模型配置：{error}"),
            )
            .with_recovery("请退出 Trae 后重试，或确认该应用已完成一次启动。")
        })?;

    let mut statement = connection
        .prepare("SELECT key FROM ItemTable WHERE key LIKE '%model_list_map%'")
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的配置结构无法识别：{error}"),
            )
        })?;
    let keys: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的配置结构无法识别：{error}"),
            )
        })?
        .filter_map(Result::ok)
        .collect();
    // The workspace-scoped key contains ':' and may lag behind; prefer the
    // global one.
    let catalogue_key = keys
        .iter()
        .find(|key| !key.contains(':'))
        .or_else(|| keys.first())
        .cloned();
    drop(statement);

    let Some(catalogue_key) = catalogue_key else {
        return Err(
            CommandError::new("trae_not_found", "未在 Trae 中找到模型配置")
                .with_recovery("请先在 Trae 中登录并打开一次模型设置。"),
        );
    };

    let stored: SqlValue = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [&catalogue_key],
            |row| row.get(0),
        )
        .map_err(|error| {
            CommandError::new(
                "trae_parse_error",
                format!("无法读取 Trae 的模型列表：{error}"),
            )
        })?;
    let raw = cell_to_bytes(stored);
    let value: Value = serde_json::from_slice(&raw).map_err(|error| {
        CommandError::new(
            "trae_parse_error",
            format!("Trae 的模型列表不是有效的 JSON：{error}"),
        )
    })?;

    let mut models = BTreeSet::new();
    if let Some(segments) = value.as_object() {
        for segment in segments.values() {
            let Some(entries) = segment.as_array() else {
                continue;
            };
            for entry in entries {
                if text_field(entry, "provider").as_deref() != Some(CUSTOM_PROVIDER) {
                    continue;
                }
                let display_name = text_field(entry, "display_name")
                    .or_else(|| text_field(entry, "name"))
                    .unwrap_or_else(|| "未命名模型".to_owned());
                models.insert(ConfiguredModel {
                    display_name,
                    provider: CUSTOM_PROVIDER.to_owned(),
                    base_url: text_field(entry, "base_url").unwrap_or_default(),
                });
            }
        }
    }

    let selection = read_active_selection(&connection)?;
    Ok((models.into_iter().collect(), selection))
}

/// The active model is stored per agent label, e.g.
/// `{"solo_agent":{"modelId":"solo_agent_3_custom_openai_compatible_..._glm-5.1_2765316226"}}`.
/// Only the model name inside that opaque id is surfaced.
fn read_active_selection(connection: &Connection) -> AppResult<Option<String>> {
    let Ok(stored) = connection.query_row(
        "SELECT value FROM ItemTable WHERE key LIKE '%recent_user_selection%' LIMIT 1",
        [],
        |row| row.get::<_, SqlValue>(0),
    ) else {
        return Ok(None);
    };
    let raw = cell_to_bytes(stored);
    let Ok(value) = serde_json::from_slice::<Value>(&raw) else {
        return Ok(None);
    };
    let names: Vec<String> = value
        .as_object()
        .into_iter()
        .flat_map(|labels| labels.values())
        .filter_map(|label| label.get("modelId").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    Ok(names.into_iter().next())
}

fn summarize(models: &[ConfiguredModel], active: Option<&str>) -> String {
    let installed_without_models =
        "尚未配置自定义模型；可先在 Trae 的模型设置中添加，AT-Switch 之后即可展示。";
    if models.is_empty() {
        return installed_without_models.to_owned();
    }
    let names = models
        .iter()
        .map(|model| model.display_name.as_str())
        .collect::<Vec<_>>()
        .join("、");
    // The stored selection is an opaque id; surface it only when it maps onto a
    // configured model, so internal identifiers never reach the UI.
    let active_label = active.and_then(|raw| {
        models
            .iter()
            .find(|model| raw.contains(&model.display_name))
            .map(|model| model.display_name.as_str())
    });
    match active_label {
        Some(name) => format!(
            "已配置 {} 个自定义模型：{names}；当前选中：{name}",
            models.len()
        ),
        None => format!("已配置 {} 个自定义模型：{names}", models.len()),
    }
}

/// AT-Switch-managed trace file placed beside `state.vscdb`. It gives the
/// transaction layer something to back up and roll back without ever touching
/// Trae's own database file.
const TRACE_FILE: &str = "at-switch-trae-state.json";

fn config_path(detection: &AgentDetection) -> AppResult<&PathBuf> {
    detection
        .config_path
        .as_ref()
        .ok_or_else(|| CommandError::new("trae_config_path_missing", "未找到 Trae 的配置数据库"))
}

fn require_configured_model(detection: &AgentDetection, model_id: &str) -> AppResult<ModelEntry> {
    let catalogue = read_catalogue(config_path(detection)?)?;
    find_model_entry(&catalogue, model_id).ok_or_else(|| missing_model_error(model_id, &catalogue))
}

fn trace_bytes(id: &str, model_id: Option<&str>) -> AppResult<Vec<u8>> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "agentId": id,
        "modelId": model_id,
    }))
    .map_err(|error| {
        CommandError::new(
            "trae_trace_serialize_failed",
            format!("无法生成 Trae 的切换记录：{error}"),
        )
    })
}

impl AgentAdapter for TraeFamilyAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.display_name
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: self.bundle_ids,
            windows_relative_paths: self.windows_paths,
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            self.macos_apps,
            self.bundle_ids,
            self.windows_paths,
        );
        let database = self.state_database(context);
        let installed = installation.is_some();
        let has_database = database.is_file();

        let (config_health, message) = if !installed {
            (
                AgentConfigHealth::UnsupportedVersion,
                format!("未在系统标准安装位置检测到 {}", self.display_name),
            )
        } else if !has_database {
            (
                AgentConfigHealth::UnsupportedVersion,
                format!(
                    "已检测到 {}；请先完整启动一次应用以生成模型配置。",
                    self.display_name
                ),
            )
        } else {
            match read_state(&database) {
                Ok((models, selection)) => (
                    AgentConfigHealth::Healthy,
                    summarize(&models, selection.as_deref()),
                ),
                Err(error) => {
                    let health = match error.code.as_str() {
                        "trae_parse_error" | "trae_schema_changed" => {
                            AgentConfigHealth::UnsupportedVersion
                        }
                        _ => AgentConfigHealth::Unreadable,
                    };
                    (health, error.message)
                }
            }
        };

        AgentDetection {
            id: self.id,
            display_name: self.display_name,
            installation,
            config_path: Some(database),
            runtime_data_dir: None,
            install_status: if installed {
                AgentInstallStatus::Installed
            } else {
                AgentInstallStatus::NotInstalled
            },
            config_health,
            // Switching is supported once the app has produced its model store;
            // credentials stay encrypted and app-managed either way.
            write_supported: installed && has_database,
            // 选中态写在数据库里，而运行中的 Trae 会持有会话内的旧状态，
            // 必须重启才会重新读取；因此这里声明需要重启，切换时由 AT-Switch
            // 退出应用并提示重启。
            needs_restart: true,
            custom_install_path: None,
            using_custom_install_path: false,
            message: Some(message),
        }
    }

    fn source_protocol(
        &self,
        _mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    /// Trae's truth is a row in `state.vscdb`, which must not be replaced
    /// wholesale (its WAL would be lost). The transaction therefore owns a small
    /// AT-Switch-managed trace file beside it, and the selection itself is
    /// rewritten row-by-row by `apply_selection`.
    fn config_write_target(&self, detection: &AgentDetection) -> Option<PathBuf> {
        detection
            .config_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| directory.join(TRACE_FILE))
    }

    fn validate_binding(&self, _desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        // The requested model is checked against Trae's own catalogue while the
        // configuration is built, where the detection is available.
        Ok(())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        require_configured_model(detection, desired.model_id)?;
        trace_bytes(self.id, Some(desired.model_id))
    }

    fn build_native_config(
        &self,
        _detection: &AgentDetection,
        _baseline: &crate::services::BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        // Restoring never touches Trae's database: the record only marks that
        // AT-Switch no longer manages a selection.
        trace_bytes(self.id, None)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        let entry = require_configured_model(detection, desired.model_id)?;
        let database = config_path(detection)?;
        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| {
                CommandError::new(
                    "trae_db_locked",
                    format!("无法读取 Trae 的模型配置：{error}"),
                )
            })?;
        let stored: Option<SqlValue> = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key LIKE '%recent_user_selection%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| {
                CommandError::new(
                    "trae_parse_error",
                    format!("无法读取 Trae 的选中状态：{error}"),
                )
            })?;
        let Some(stored) = stored else {
            return Err(selection_missing_error());
        };
        // Every rewritten id ends with `//{display name}_{custom model id}`.
        let expected = format!("//{}_{}", entry.display_name, entry.custom_model_id);
        if String::from_utf8_lossy(&cell_to_bytes(stored)).contains(&expected) {
            Ok(())
        } else {
            Err(CommandError::new(
                "trae_write_verification_failed",
                "Trae 的选中模型与目标不一致",
            )
            .with_recovery("请重新点击目标模型的“切换”，AT-Switch 会重新写入并校验。"))
        }
    }
}

pub const TRAEWORK_ADAPTER: TraeFamilyAdapter = TraeFamilyAdapter::new(
    "traework",
    "Trae CN",
    &["Trae CN.app"],
    &["cn.trae.app"],
    &["Programs/Trae CN/Trae CN.exe", "Trae CN/Trae CN.exe"],
    "Trae CN",
);

/// TRAE SOLO CN (bundle `cn.trae.solo.app`)。它和 Trae CN 是同一套选中态机制：
/// `AI.agent.model.recent_user_selection_by_agent_label`(扁平 `{label:{modelId}}`)
/// 与 `AI.agent.model.session_selected_model`(嵌套 `{sessionId:{label:{modelId}}}`)
/// 都明文写在 `User/globalStorage/state.vscdb`，id 形状逐字一致，因此直接复用同
/// 一个适配器实现。
///
/// 0.1.66 曾把选中态放进 `ModularData/ai-agent/database.db`(加密库，文件头不是
/// "SQLite format 3")，明文库里查不到任何选中态键，那时只能降级为只读。
/// 0.1.69 起选中态已回到明文库，故恢复为可切换。
pub const TRAECODE_ADAPTER: TraeFamilyAdapter = TraeFamilyAdapter::new(
    "traecode",
    "TRAE SOLO CN",
    &["TRAE SOLO CN.app"],
    &["cn.trae.solo.app"],
    &[
        "Programs/TRAE SOLO CN/TRAE SOLO CN.exe",
        "TRAE SOLO CN/TRAE SOLO CN.exe",
    ],
    "TRAE SOLO CN",
);

/// A model already configured inside Trae, with everything needed to rebuild
/// the opaque id the app stores for the active selection.
struct ModelEntry {
    name: String,
    provider: String,
    config_source: i64,
    custom_model_id: String,
    display_name: String,
}

/// A rewritten selection record, kept so the previous cell can be restored
/// byte-for-byte, including whether Trae stored it as TEXT or BLOB.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SelectionChange {
    pub key: String,
    pub previous: SqlValue,
    /// `None` 表示 `globalStorage` 的主库；`Some` 表示对应的 workspace 库。
    pub database: Option<PathBuf>,
}

/// `ItemTable.value` is dynamically typed: VS Code-family apps write it as TEXT
/// while other builds use BLOB. Reading either as `Vec<u8>` fails outright, so
/// every read goes through this conversion.
fn cell_to_bytes(value: SqlValue) -> Vec<u8> {
    match value {
        SqlValue::Text(text) => text.into_bytes(),
        SqlValue::Blob(bytes) => bytes,
        SqlValue::Integer(number) => number.to_string().into_bytes(),
        SqlValue::Real(number) => number.to_string().into_bytes(),
        SqlValue::Null => Vec::new(),
    }
}

/// Writes new content back using the type the cell already had, so a TEXT cell
/// never silently turns into a BLOB (which would break the app's own reader).
fn bytes_to_cell(previous: &SqlValue, bytes: Vec<u8>) -> SqlValue {
    match previous {
        SqlValue::Text(_) => SqlValue::Text(String::from_utf8_lossy(&bytes).into_owned()),
        _ => SqlValue::Blob(bytes),
    }
}

/// Trae identifies the active model with
/// `{label}_{config_source}_{provider}_{name}_{custom_model_id}` — the same
/// shape observed for a user-selected model.
fn selection_id(label: &str, entry: &ModelEntry) -> String {
    format!(
        "{label}_{}_{}_{}_{}",
        entry.config_source, entry.provider, entry.name, entry.custom_model_id
    )
}

fn json_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn find_model_entry(catalogue: &Value, model_id: &str) -> Option<ModelEntry> {
    let segments = catalogue.as_object()?;
    for segment in segments.values() {
        let Some(entries) = segment.as_array() else {
            continue;
        };
        for entry in entries {
            if text_field(entry, "provider").as_deref() != Some(CUSTOM_PROVIDER) {
                continue;
            }
            let display_name = text_field(entry, "display_name")
                .or_else(|| text_field(entry, "name"))
                .unwrap_or_default();
            // 模型名不区分大小写：Trae 里存的是 `glm-5.2`，而用户配置里可能写成
            // `GLM-5.2`，精确比较会让同一个模型判定为"不存在"。
            if !display_name.eq_ignore_ascii_case(model_id) {
                continue;
            }
            let name = entry.get("name").and_then(Value::as_str)?.to_owned();
            let custom_model_id = json_scalar(entry.get("custom_model_id")?)?;
            let config_source = entry.get("config_source").and_then(Value::as_i64)?;
            return Some(ModelEntry {
                name,
                provider: CUSTOM_PROVIDER.to_owned(),
                config_source,
                custom_model_id,
                display_name,
            });
        }
    }
    None
}

/// Names of every custom model the user configured inside Trae, used to build
/// actionable errors when the requested model is absent.
fn custom_model_names(catalogue: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Some(segments) = catalogue.as_object() else {
        return names;
    };
    for segment in segments.values() {
        let Some(entries) = segment.as_array() else {
            continue;
        };
        for entry in entries {
            if text_field(entry, "provider").as_deref() != Some(CUSTOM_PROVIDER) {
                continue;
            }
            let Some(name) =
                text_field(entry, "display_name").or_else(|| text_field(entry, "name"))
            else {
                continue;
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Builds the "model is not configured inside Trae" error. Whatever the user
/// already configured is listed, so a mismatch between two Trae apps (each has
/// its own database) is visible instead of a dead end.
/// Trae keeps its custom models on the account side — the app posts them to its
/// own `chat/add_custom_model` service — and `model_list_map` is only a cache
/// that `storeModelListMap` overwrites wholesale from that fetched list. A
/// locally injected entry is therefore never durable, and the encrypted `ak`
/// could not be minted here anyway. So AT-Switch can only repoint a model that
/// already exists; the recovery explains both the per-model manual step and the
/// one-time proxy shortcut that removes it.
fn missing_model_error(model_id: &str, catalogue: &Value) -> CommandError {
    let available = custom_model_names(catalogue);
    let listed = if available.is_empty() {
        "该应用内还没有任何自定义模型。".to_owned()
    } else {
        format!("该应用内当前可用的自定义模型：{}。", available.join("、"))
    };
    let recovery = format!(
        "{listed}Trae 的自定义模型由账号侧下发，AT-Switch 无法代为创建，只能在已存在的模型之间切换。逐个添加：在 Trae 的模型设置里添加「{model_id}」并保存，再回到 AT-Switch 切换。只配一次：在 Trae 里只添加一个自定义模型，Base URL 填 AT-Switch「本地代理」页显示的地址（形如 http://127.0.0.1:54187/v1/chat/completions），API Key 随意填占位；之后在 AT-Switch 用「高级代理路由」切换任意 Provider 与模型，Trae 无需再改。"
    );
    CommandError::new(
        "trae_model_not_configured",
        format!("Trae 中没有名为「{model_id}」的自定义模型"),
    )
    .with_recovery(recovery)
}

/// Trae writes its active-model records only after the user picks a model
/// inside the app, so a profile that has never selected one leaves AT-Switch
/// with nothing to rewrite. The id shape is Trae's own, so it is never invented.
fn selection_missing_error() -> CommandError {
    CommandError::new(
        "trae_selection_missing",
        "该应用还没有任何模型选中记录",
    )
    .with_recovery(
        "请先打开该应用新建一次对话，在模型列表里选中你要用的模型并发一条消息，让应用记录下选中状态；然后回到 AT-Switch 重新切换。",
    )
}

/// True when a recorded selection already points at the target model, which
/// makes a no-op write a success rather than a failure.
fn selection_already_targets(connection: &Connection, entry: &ModelEntry) -> AppResult<bool> {
    let expected = format!("//{}_{}", entry.display_name, entry.custom_model_id);
    let mut statement = connection
        .prepare(
            "SELECT value FROM ItemTable WHERE key LIKE '%recent_user_selection%' OR key LIKE '%session_selected_model%'",
        )
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的选中状态无法识别：{error}"),
            )
        })?;
    let values: Vec<SqlValue> = statement
        .query_map([], |row| row.get::<_, SqlValue>(0))
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的选中状态无法识别：{error}"),
            )
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(values
        .into_iter()
        .any(|value| String::from_utf8_lossy(&cell_to_bytes(value)).contains(&expected)))
}

/// Rewrites one `{label: {modelId, mode}}` entry, but only when the current
/// value already points at a user-configured provider. Built-in models are left
/// exactly as they are.
fn rewrite_label(label: &str, selection: &mut Value, entry: &ModelEntry) -> bool {
    let Some(current) = selection.get("modelId").and_then(Value::as_str) else {
        return false;
    };
    if !current.contains(CUSTOM_PROVIDER) {
        return false;
    }
    let next = selection_id(label, entry);
    if next == current {
        return false;
    }
    selection["modelId"] = Value::String(next);
    true
}

/// `globalModelMap` 的形态是 `{label: "<id>"}`：值**不带 label 前缀**，与
/// `recent_user_selection` 的 `{label}_{config_source}_...` 不同。这里只改写
/// 已经指向自定义 Provider 的条目，内置模型保持原样。
fn rewrite_global_model_map(value: &[u8], entry: &ModelEntry) -> Option<Vec<u8>> {
    let mut root: Value = serde_json::from_slice(value).ok()?;
    let map = root.as_object_mut()?;
    let next = global_model_id(entry);
    let targets: Vec<String> = map
        .iter()
        .filter(|(_, current)| {
            current
                .as_str()
                .is_some_and(|id| id.contains(CUSTOM_PROVIDER))
        })
        .map(|(label, _)| label.clone())
        .collect();
    let mut changed = false;
    for label in targets {
        if map.get(&label).and_then(Value::as_str) == Some(next.as_str()) {
            continue;
        }
        map.insert(label, Value::String(next.clone()));
        changed = true;
    }
    changed.then(|| serde_json::to_vec(&root).ok()).flatten()
}

/// Trae 在 `User/workspaceStorage/<hash>/state.vscdb` 里保存了**另一份**
/// `globalModelMap`，界面读取的很可能就是这一份：只改 `globalStorage` 会出现
/// "数据库已改、界面不变"。这里遍历所有 workspace 库并同步改写。
fn apply_workspace_model_maps(database: &Path, entry: &ModelEntry) -> Vec<SelectionChange> {
    let Some(user_dir) = database.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(user_dir.join("workspaceStorage")) else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for workspace in entries.flatten() {
        let path = workspace.path().join("state.vscdb");
        if !path.is_file() {
            continue;
        }
        match rewrite_workspace_map(&path, entry) {
            Ok(found) => changes.extend(found),
            Err(error) => log::warn!(
                "Trae workspace model map skipped ({}): {}",
                path.display(),
                error.message
            ),
        }
    }
    changes
}

/// 工作区库里有一整套会话级映射（界面显示当前会话的模型），因此按键类型逐条
/// 改写，而不是只看 `globalModelMap`。
fn rewrite_workspace_map(path: &PathBuf, entry: &ModelEntry) -> AppResult<Vec<SelectionChange>> {
    let connection = Connection::open(path).map_err(|error| {
        CommandError::new(
            "trae_db_locked",
            format!("无法写入 Trae 的工作区数据库：{error}"),
        )
    })?;
    let keys: Vec<String> = connection
        .prepare(
            "SELECT key FROM ItemTable WHERE key LIKE '%ModelMap%' OR key LIKE '%recent_user_selection%' OR key LIKE '%session_selected_model%'",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的工作区配置无法识别：{error}"),
            )
        })?;
    let mut changes = Vec::new();
    for key in keys {
        let Some(change) = rewrite_one_workspace_key(&connection, path, &key, entry)? else {
            continue;
        };
        changes.push(change);
    }
    Ok(changes)
}

fn rewrite_one_workspace_key(
    connection: &Connection,
    path: &Path,
    key: &str,
    entry: &ModelEntry,
) -> AppResult<Option<SelectionChange>> {
    let previous: SqlValue = connection
        .query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .map_err(|error| {
            CommandError::new(
                "trae_parse_error",
                format!("无法读取 Trae 的工作区模型映射：{error}"),
            )
        })?;
    let Some(updated) = rewrite_by_key(key, &cell_to_bytes(previous.clone()), entry) else {
        return Ok(None);
    };
    connection
        .execute(
            "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
            params![bytes_to_cell(&previous, updated), key],
        )
        .map_err(|error| {
            CommandError::new(
                "trae_selection_write_failed",
                format!("无法更新 Trae 的工作区模型映射：{error}"),
            )
        })?;
    Ok(Some(SelectionChange {
        key: key.to_owned(),
        previous,
        database: Some(path.to_path_buf()),
    }))
}

/// `sessionRelation:modelMap` 的形态是 `{sessionId: {label: "<id>"}}`：
/// 界面显示的是**当前活跃会话**的条目，因此必须逐会话一并改写。
/// 值同样不带 label 前缀，且只改写已指向自定义 Provider 的条目。
fn rewrite_session_model_map(value: &[u8], entry: &ModelEntry) -> Option<Vec<u8>> {
    let mut root: Value = serde_json::from_slice(value).ok()?;
    let next = global_model_id(entry);
    let sessions = root.as_object_mut()?;
    let mut changed = false;
    for labels in sessions.values_mut() {
        let Some(map) = labels.as_object_mut() else {
            continue;
        };
        let targets: Vec<String> = map
            .iter()
            .filter(|(_, current)| {
                current
                    .as_str()
                    .is_some_and(|id| id.contains(CUSTOM_PROVIDER))
            })
            .map(|(label, _)| label.clone())
            .collect();
        for label in targets {
            if map.get(&label).and_then(Value::as_str) == Some(next.as_str()) {
                continue;
            }
            map.insert(label, Value::String(next.clone()));
            changed = true;
        }
    }
    changed.then(|| serde_json::to_vec(&root).ok()).flatten()
}

/// 按键名分派到对应的改写实现：Trae 的模型状态分散在若干不同形状的键里。
fn rewrite_by_key(key: &str, value: &[u8], entry: &ModelEntry) -> Option<Vec<u8>> {
    if key.contains("globalModelMap") {
        rewrite_global_model_map(value, entry)
    } else if key.contains("modelMap") {
        rewrite_session_model_map(value, entry)
    } else {
        rewrite_selection_value(value, entry)
    }
}

/// `globalModelMap` 的值：与 `selection_id` 相同，但没有开头的 `{label}_`。
fn global_model_id(entry: &ModelEntry) -> String {
    format!(
        "{}_{}_{}_{}",
        entry.config_source, entry.provider, entry.name, entry.custom_model_id
    )
}

/// Handles both stored shapes: `{label: selection}` for the recent-selection
/// map, and `{sessionHash: {label: selection}}` for the session map.
fn rewrite_selection_value(value: &[u8], entry: &ModelEntry) -> Option<Vec<u8>> {
    let mut root: Value = serde_json::from_slice(value).ok()?;
    let mut changed = false;
    let top = root.as_object_mut()?;
    for (top_key, top_value) in top.iter_mut() {
        let Some(map) = top_value.as_object_mut() else {
            continue;
        };
        if map.contains_key("modelId") {
            if rewrite_label(top_key, top_value, entry) {
                changed = true;
            }
            continue;
        }
        for (label, selection) in map.iter_mut() {
            if rewrite_label(label, selection, entry) {
                changed = true;
            }
        }
    }
    changed.then(|| serde_json::to_vec(&root).ok()).flatten()
}

fn read_catalogue(database: &PathBuf) -> AppResult<Value> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            CommandError::new(
                "trae_db_locked",
                format!("无法读取 Trae 的模型配置：{error}"),
            )
            .with_recovery("请退出 Trae 后重试，或确认该应用已完成一次启动。")
        })?;
    let mut statement = connection
        .prepare("SELECT value FROM ItemTable WHERE key LIKE '%model_list_map%'")
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的配置结构无法识别：{error}"),
            )
        })?;
    let stored: Vec<SqlValue> = statement
        .query_map([], |row| row.get::<_, SqlValue>(0))
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的配置结构无法识别：{error}"),
            )
        })?
        .filter_map(Result::ok)
        .collect();
    drop(statement);

    // Trae keeps a parallel `model_list_map` whose key carries a colon prefix,
    // and the two copies diverge: a model added in one may be missing from the
    // other. Both are read and merged so no configured model is invisible.
    let mut merged = serde_json::Map::new();
    for value in stored {
        let raw = cell_to_bytes(value);
        let Ok(Value::Object(segments)) = serde_json::from_slice::<Value>(&raw) else {
            continue;
        };
        for (segment, entries) in segments {
            merge_segment(&mut merged, segment, entries);
        }
    }
    if merged.is_empty() {
        return Err(
            CommandError::new("trae_not_found", "未在 Trae 中找到模型配置")
                .with_recovery("请先在 Trae 中登录并打开一次模型设置。"),
        );
    }
    Ok(Value::Object(merged))
}

/// Appends one agent segment's entries, skipping the duplicates Trae repeats
/// across its parallel map copies.
fn merge_segment(merged: &mut serde_json::Map<String, Value>, segment: String, entries: Value) {
    let incoming = match entries {
        Value::Array(items) => items,
        other => {
            merged.entry(segment).or_insert(other);
            return;
        }
    };
    let slot = merged
        .entry(segment)
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(existing) = slot.as_array_mut() else {
        return;
    };
    for item in incoming {
        let name = item.get("name").and_then(Value::as_str);
        let duplicate = name.is_some()
            && existing
                .iter()
                .any(|known| known.get("name").and_then(Value::as_str) == name);
        if !duplicate {
            existing.push(item);
        }
    }
}

/// Points Trae's active-model records at the requested model. Only models the
/// user already configured inside Trae can be selected — credentials stay
/// encrypted and untouched.
pub(crate) fn apply_selection(
    detection: &AgentDetection,
    model_id: &str,
) -> AppResult<Vec<SelectionChange>> {
    let database = detection
        .config_path
        .clone()
        .ok_or_else(|| CommandError::new("trae_config_path_missing", "未找到 Trae 的配置数据库"))?;
    let catalogue = read_catalogue(&database)?;
    let entry = find_model_entry(&catalogue, model_id)
        .ok_or_else(|| missing_model_error(model_id, &catalogue))?;

    let connection = Connection::open(&database).map_err(|error| {
        CommandError::new(
            "trae_db_locked",
            format!("无法写入 Trae 的模型配置：{error}"),
        )
        .with_recovery("请完全退出 Trae 后重试。")
    })?;
    let keys: Vec<String> = connection
        .prepare(
            "SELECT key FROM ItemTable WHERE key LIKE '%recent_user_selection%' OR key LIKE '%session_selected_model%' OR key LIKE '%ModelMap%'",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| {
            CommandError::new(
                "trae_schema_changed",
                format!("Trae 的选中状态无法识别：{error}"),
            )
        })?;
    if keys.is_empty() {
        return Err(selection_missing_error());
    }

    let mut changes = Vec::new();
    for key in keys {
        let previous: SqlValue = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                [&key],
                |row| row.get(0),
            )
            .map_err(|error| {
                CommandError::new(
                    "trae_parse_error",
                    format!("无法读取 Trae 的选中状态：{error}"),
                )
            })?;
        // 各键的值形状不同，按键名分派（见 rewrite_by_key）。
        let rewritten = rewrite_by_key(&key, &cell_to_bytes(previous.clone()), &entry);
        let Some(updated) = rewritten else {
            continue;
        };
        connection
            .execute(
                "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
                params![bytes_to_cell(&previous, updated), key],
            )
            .map_err(|error| {
                CommandError::new(
                    "trae_selection_write_failed",
                    format!("无法更新 Trae 的选中模型：{error}"),
                )
            })?;
        changes.push(SelectionChange {
            key,
            previous,
            database: None,
        });
    }
    // 工作区库里还有独立的一份 globalModelMap，界面读取的很可能就是它。
    let changes = {
        let mut merged = changes;
        merged.extend(apply_workspace_model_maps(&database, &entry));
        merged
    };
    if changes.is_empty() {
        // Nothing was rewritten: either the target is already active, or every
        // recorded label still points at a built-in model, which AT-Switch
        // deliberately leaves alone.
        if selection_already_targets(&connection, &entry)? {
            return Ok(changes);
        }
        return Err(CommandError::new(
            "trae_selection_not_switchable",
            "Trae 当前记录的模型中没有可切换的自定义模型",
        )
        .with_recovery(
            "请先打开该应用，在模型列表里手动选择一次你添加的自定义模型，让应用记录下它；然后回到 AT-Switch 重新切换。",
        ));
    }
    Ok(changes)
}

/// Puts the recorded selections back exactly as they were.
///
/// Reserved for the rollback path.
#[allow(dead_code)]
pub(crate) fn restore_selection(
    detection: &AgentDetection,
    changes: &[SelectionChange],
) -> AppResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let default_database = detection
        .config_path
        .clone()
        .ok_or_else(|| CommandError::new("trae_config_path_missing", "未找到 Trae 的配置数据库"))?;
    for change in changes {
        // 改动可能落在工作区库里，需要按条目记录的库还原。
        let database = change
            .database
            .clone()
            .unwrap_or_else(|| default_database.clone());
        let connection = Connection::open(&database).map_err(|error| {
            CommandError::new(
                "trae_db_locked",
                format!("无法写入 Trae 的模型配置：{error}"),
            )
        })?;
        connection
            .execute(
                "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
                params![change.previous, change.key],
            )
            .map_err(|error| {
                CommandError::new(
                    "trae_selection_restore_failed",
                    format!("无法还原 Trae 的选中模型：{error}"),
                )
            })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "trae_family_tests.rs"]
mod tests;
