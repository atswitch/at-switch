use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::domain::{ApiProtocol, AppResult, CommandError};

use super::{AgentDetection, TraeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TraeUiSnapshot {
    pub selection: String,
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
    fn delete_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()>;
}

pub(super) struct SystemTraeUi;

#[cfg(target_os = "macos")]
#[path = "trae_ui/macos.rs"]
mod platform;

#[cfg(target_os = "windows")]
#[path = "trae_ui/windows.rs"]
mod platform;

impl TraeUi for SystemTraeUi {
    fn snapshot_interactive(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
    ) -> AppResult<TraeUiSnapshot> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            platform::snapshot(kind, detection, true)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (kind, detection);
            Err(unsupported_platform())
        }
    }

    fn add_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        input: &TraeModelInput,
    ) -> AppResult<()> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            platform::add_model(kind, detection, input)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (kind, detection, input);
            Err(unsupported_platform())
        }
    }

    fn select_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            platform::select_model(kind, detection, display_name)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (kind, detection, display_name);
            Err(unsupported_platform())
        }
    }

    fn delete_model(
        &self,
        kind: TraeKind,
        detection: &AgentDetection,
        display_name: &str,
    ) -> AppResult<()> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            platform::delete_model(kind, detection, display_name)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (kind, detection, display_name);
            Err(unsupported_platform())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn unsupported_platform() -> CommandError {
    CommandError::new(
        "trae_ui_platform_unsupported",
        "Trae 界面自动化目前仅支持 macOS 和 Windows",
    )
}

pub(super) fn protocol_label(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => "OpenAI Chat Completions 格式",
        ApiProtocol::OpenaiResponses => "OpenAI Responses API 格式",
        ApiProtocol::AnthropicMessages => "Anthropic Messages 格式",
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

pub(super) fn cached_model_names(path: Option<&Path>) -> AppResult<HashSet<String>> {
    cached_model_names_by(path, false)
}

fn cached_model_names_by(path: Option<&Path>, custom_only: bool) -> AppResult<HashSet<String>> {
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
    let mut names = HashSet::new();
    for raw in values.flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            collect_model_names(&value, &mut names, custom_only);
        }
    }
    Ok(names)
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

fn collect_model_names(value: &Value, names: &mut HashSet<String>, custom_only: bool) {
    match value {
        Value::Object(object) => {
            let custom = object
                .get("provider")
                .and_then(Value::as_str)
                .is_some_and(|provider| provider.starts_with("custom_"));
            if custom || (!custom_only && object.get("provider").and_then(Value::as_str).is_some())
            {
                if let Some(name) = object.get("display_name").and_then(Value::as_str) {
                    names.insert(name.to_owned());
                }
            }
            for child in object.values() {
                collect_model_names(child, names, custom_only);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_model_names(child, names, custom_only);
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
        let mut all = HashSet::new();
        collect_model_names(&value, &mut all, false);
        assert_eq!(
            all,
            HashSet::from(["Auto Mode".to_owned(), "User model".to_owned()])
        );
        let mut custom = HashSet::new();
        collect_model_names(&value, &mut custom, true);
        assert_eq!(custom, HashSet::from(["User model".to_owned()]));

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
