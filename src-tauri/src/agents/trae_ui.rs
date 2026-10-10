use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::domain::{ApiProtocol, AppResult, CommandError};

use super::{AgentDetection, TraeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TraeUiSnapshot {
    pub selection: String,
    pub recent_selection: String,
    pub active_session_id: Option<String>,
    pub active_selection: Option<String>,
    pub custom_models: HashSet<String>,
    pub custom_models_by_id: HashMap<String, BTreeSet<String>>,
    pub custom_endpoints_by_name: HashMap<String, BTreeSet<String>>,
}

#[derive(Default)]
pub(super) struct CachedCustomModelCatalog {
    pub names: HashSet<String>,
    pub names_by_id: HashMap<String, BTreeSet<String>>,
    pub endpoints_by_name: HashMap<String, BTreeSet<String>>,
}

#[derive(Clone)]
pub(super) struct TraeModelInput {
    pub display_name: String,
    pub model_id: String,
    pub protocol: ApiProtocol,
    pub base_url: String,
    pub credential: Zeroizing<String>,
}

pub(super) trait TraeUi: Send + Sync {
    fn snapshot_interactive(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
    ) -> AppResult<TraeUiSnapshot>;
    fn add_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        input: &TraeModelInput,
    ) -> AppResult<()>;
    fn select_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()>;

    fn select_model_scoped(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
        _session_id: Option<&str>,
    ) -> AppResult<()> {
        self.select_model(kind, detection, display_name)
    }
    fn delete_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()>;

    fn restore_legacy_work_remote(
        &self,
        _detection: &AgentDetection,
        _display_name: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    fn finish_operation(&self, _kind: TraeKind) {}

    fn complete_operation(&self, kind: TraeKind) -> AppResult<()> {
        self.finish_operation(kind);
        Ok(())
    }
}

pub(super) struct SystemTraeUi {
    native: native::NativeTraeControl,
}

#[path = "trae_ui/native.rs"]
mod native;

impl SystemTraeUi {
    pub(super) fn new() -> Self {
        Self {
            native: native::NativeTraeControl::new(),
        }
    }
}

impl TraeUi for SystemTraeUi {
    fn snapshot_interactive(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
    ) -> AppResult<TraeUiSnapshot> {
        self.native.snapshot(kind, detection)
    }

    fn add_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        input: &TraeModelInput,
    ) -> AppResult<()> {
        self.native.add_model(kind, detection, input)
    }

    fn select_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.native.select_model(kind, detection, display_name)
    }

    fn select_model_scoped(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
        session_id: Option<&str>,
    ) -> AppResult<()> {
        self.native
            .select_model_scoped(kind, detection, display_name, session_id)
    }

    fn delete_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.native.delete_model(kind, detection, display_name)
    }

    fn finish_operation(&self, _kind: TraeKind) {
        self.native.finish_operation();
    }

    fn complete_operation(&self, _kind: TraeKind) -> AppResult<()> {
        self.native.complete_operation()
    }

    fn restore_legacy_work_remote(
        &self,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        self.native
            .restore_legacy_work_remote(detection, display_name)
    }
}

pub(super) fn endpoint_path(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => "chat/completions",
        ApiProtocol::OpenaiResponses => "responses",
        ApiProtocol::AnthropicMessages => "messages",
    }
}

pub(super) fn cached_custom_models(path: Option<&Path>) -> AppResult<HashSet<String>> {
    Ok(cached_custom_model_catalog(path)?.names)
}

pub(super) fn cached_recent_selection(
    path: Option<&Path>,
    label: &str,
) -> AppResult<Option<(i64, String)>> {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 模型选择"))?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key LIKE '%AI.agent.model.recent_user_selection_by_agent_label' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 模型选择格式无法识别"))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 模型选择格式无法识别"))?;
    let Some(selection) = value.get(label) else {
        return Ok(None);
    };
    let mode = selection.get("mode").and_then(Value::as_i64);
    let model_key = selection.get("modelId").and_then(Value::as_str);
    Ok(mode
        .zip(model_key)
        .map(|(mode, key)| (mode, key.to_owned())))
}

pub(super) fn cached_model_key_matches(
    path: Option<&Path>,
    label: &str,
    model_id: &str,
    selected_key: &str,
) -> AppResult<Option<bool>> {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 模型缓存"))?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key LIKE '%AI.agent.model.model_list_map' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 模型缓存格式无法识别"))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 模型缓存格式无法识别"))?;
    let Some(models) = value.get(label).and_then(Value::as_array) else {
        return Ok(None);
    };
    let candidates = models.iter().filter_map(|model| {
        let (Some(source), Some(provider), Some(name), Some(custom_id)) = (
            model.get("config_source").and_then(Value::as_i64),
            model.get("provider").and_then(Value::as_str),
            model.get("name").and_then(Value::as_str),
            model.get("custom_model_id"),
        ) else {
            return None;
        };
        if !provider.starts_with("custom_")
            || name.split_once("//").map(|(_, id)| id) != Some(model_id)
        {
            return None;
        }
        let custom_id = custom_id
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| custom_id.to_string());
        Some(format!("{label}_{source}_{provider}_{name}_{custom_id}"))
    });
    let mut found = false;
    let mut matched = false;
    for candidate in candidates {
        found = true;
        matched |= selected_key == candidate;
    }
    Ok(found.then_some(matched))
}

pub(super) fn cached_session_selection(
    path: Option<&Path>,
    session_id: &str,
    label: &str,
) -> AppResult<Option<(i64, String)>> {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 会话模型选择"))?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key LIKE '%AI.agent.model.session_selected_model' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 会话模型选择格式无法识别"))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw).map_err(|_| {
        CommandError::new("trae_profile_unreadable", "Trae 会话模型选择格式无法识别")
    })?;
    let selection = value.get(session_id).and_then(|session| session.get(label));
    let mode = selection
        .and_then(|selection| selection.get("mode"))
        .and_then(Value::as_i64);
    let key = selection
        .and_then(|selection| selection.get("modelId"))
        .and_then(Value::as_str);
    Ok(mode.zip(key).map(|(mode, key)| (mode, key.to_owned())))
}

pub(super) fn cached_custom_model_catalog(
    path: Option<&Path>,
) -> AppResult<CachedCustomModelCatalog> {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Ok(Default::default());
    };
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 模型缓存"))?;
    let mut statement = connection
        .prepare("SELECT value FROM ItemTable WHERE key LIKE '%AI.agent.model.model_list_map'")
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 模型缓存格式无法识别"))?;
    let values = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 模型缓存"))?;
    let mut catalog = CachedCustomModelCatalog::default();
    for raw in values.flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            collect_custom_model_catalog(&value, &mut catalog);
        }
    }
    Ok(catalog)
}

fn collect_custom_model_catalog(value: &Value, catalog: &mut CachedCustomModelCatalog) {
    match value {
        Value::Object(object) => {
            let custom = object
                .get("provider")
                .and_then(Value::as_str)
                .is_some_and(|provider| provider.starts_with("custom_"));
            if custom {
                if let Some(display_name) = object.get("display_name").and_then(Value::as_str) {
                    catalog.names.insert(display_name.to_owned());
                    if let Some(endpoint) = object.get("base_url").and_then(Value::as_str) {
                        catalog
                            .endpoints_by_name
                            .entry(display_name.to_owned())
                            .or_default()
                            .insert(endpoint.to_owned());
                    }
                    let model_id = ["config_name", "name"].into_iter().find_map(|field| {
                        object
                            .get(field)
                            .and_then(Value::as_str)
                            .and_then(|value| value.split_once("//"))
                            .map(|(_, model_id)| model_id)
                            .filter(|model_id| !model_id.is_empty())
                    });
                    if let Some(model_id) = model_id {
                        catalog
                            .names_by_id
                            .entry(model_id.to_owned())
                            .or_default()
                            .insert(display_name.to_owned());
                    }
                }
            }
            for child in object.values() {
                collect_custom_model_catalog(child, catalog);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_custom_model_catalog(child, catalog);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn cached_selection_must_use_key_from_restart_model_catalog() {
        let temp = tempfile::tempdir().expect("temporary profile");
        let path = temp.path().join("state.vscdb");
        let connection = Connection::open(&path).expect("state database");
        connection
            .execute("CREATE TABLE ItemTable (key TEXT, value TEXT)", [])
            .expect("item table");
        let catalog = json!({"solo_work_lite": [{
            "config_source": 3,
            "provider": "custom_openai_compatible",
            "name": "custom_openai_compatible//fictional-model",
            "custom_model_id": 123,
            "display_name": "Example provider · fictional-model"
        }]});
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                [
                    "test:AI.agent.model.model_list_map",
                    catalog.to_string().as_str(),
                ],
            )
            .expect("model catalog");
        assert_eq!(cached_model_key_matches(
            Some(&path),
            "solo_work_lite",
            "fictional-model",
            "solo_work_lite_3_custom_openai_compatible_custom_openai_compatible//fictional-model_123"
        )
        .expect("canonical model key"), Some(true));
        assert_eq!(
            cached_model_key_matches(
                Some(&path),
                "solo_work_lite",
                "fictional-model",
                "solo_work_lite_3_custom_openai_compatible_fictional-model_123"
            )
            .expect("transient model key"),
            Some(false)
        );
    }

    #[test]
    fn cached_catalog_recognizes_models_without_collecting_unrelated_labels() {
        let value = json!({
            "section": { "display_name": "Not a model" },
            "models": [
                { "provider": "official", "display_name": "Auto Mode" },
                {
                    "provider": "custom_openai_compatible",
                    "display_name": "User model",
                    "base_url": "https://provider.example.test/v1/chat/completions",
                    "name": "custom_openai_compatible//fictional/model"
                }
            ]
        });
        let mut catalog = CachedCustomModelCatalog::default();
        collect_custom_model_catalog(&value, &mut catalog);
        assert_eq!(catalog.names, HashSet::from(["User model".to_owned()]));
        assert_eq!(
            catalog.names_by_id.get("fictional/model"),
            Some(&BTreeSet::from(["User model".to_owned()]))
        );
        assert_eq!(
            catalog.endpoints_by_name.get("User model"),
            Some(&BTreeSet::from([
                "https://provider.example.test/v1/chat/completions".to_owned()
            ]))
        );
    }
}
