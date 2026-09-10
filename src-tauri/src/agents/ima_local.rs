use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::{AppResult, CommandError};

const EXTRA_SETTINGS: &str = "kExtraSettingInfo";
const SCENES: [&str; 2] = ["modelConfig", "copilotModelConfig"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Selection {
    pub model_id: String,
    pub model_type: i64,
}

// Keep absence distinct from JSON null without ever capturing unrelated account
// data, model credentials or unknown configuration in the restoration snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "presence", content = "value", rename_all = "snake_case")]
enum SavedField<T> {
    Missing,
    Null,
    Value(T),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SceneSnapshot {
    existed: bool,
    model_id: SavedField<String>,
    model_type: SavedField<i64>,
    timestamp: SavedField<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LocalSelectionSnapshot {
    extra_settings_existed: bool,
    scenes: [SceneSnapshot; 2],
}

// A compensated remote re-creation can return a different server model ID. Only
// remap the operation snapshot; the immutable first-takeover baseline stays intact.
pub(crate) fn remap_models(
    snapshot: &LocalSelectionSnapshot,
    replacements: &[(String, Selection)],
) -> LocalSelectionSnapshot {
    let mut remapped = snapshot.clone();
    for scene in &mut remapped.scenes {
        let SavedField::Value(current) = &scene.model_id else {
            continue;
        };
        if let Some((_, replacement)) = replacements.iter().find(|(old_id, _)| old_id == current) {
            scene.model_id = SavedField::Value(replacement.model_id.clone());
            scene.model_type = SavedField::Value(replacement.model_type);
        }
    }
    remapped
}

pub(crate) fn verify_selection(preferences: &Path, selections: &[Selection; 2]) -> AppResult<()> {
    let actual = snapshot(preferences)?;
    for (scene, expected) in actual.scenes.iter().zip(selections) {
        if scene.model_id != SavedField::Value(expected.model_id.clone())
            || scene.model_type != SavedField::Value(expected.model_type)
        {
            return Err(verification_error());
        }
    }
    Ok(())
}

pub(crate) fn verify_snapshot(
    preferences: &Path,
    baseline: &LocalSelectionSnapshot,
) -> AppResult<()> {
    let actual = snapshot(preferences)?;
    if baseline.extra_settings_existed && !actual.extra_settings_existed {
        return Err(verification_error());
    }
    for (scene, saved) in actual.scenes.iter().zip(&baseline.scenes) {
        if (saved.existed && !scene.existed)
            || scene.model_id != saved.model_id
            || scene.model_type != saved.model_type
            || scene.timestamp != saved.timestamp
        {
            return Err(verification_error());
        }
    }
    Ok(())
}

pub(crate) fn snapshot(preferences: &Path) -> AppResult<LocalSelectionSnapshot> {
    let root = read_preferences(preferences)?;
    let extra = read_extra(&root)?;
    Ok(LocalSelectionSnapshot {
        extra_settings_existed: root.contains_key(EXTRA_SETTINGS),
        scenes: [
            snapshot_scene(&extra, SCENES[0])?,
            snapshot_scene(&extra, SCENES[1])?,
        ],
    })
}

pub(crate) fn build_selection(
    preferences: &Path,
    selections: &[Selection; 2],
) -> AppResult<Vec<u8>> {
    let mut root = read_preferences(preferences)?;
    let mut extra = read_extra(&root)?;
    // Validate both scenes before producing a candidate, including fields whose
    // original values are required for a subsequent exact restoration.
    for name in SCENES {
        snapshot_scene(&extra, name)?;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .ok_or_else(|| {
            CommandError::new("ima_clock_invalid", "系统时间不可用，无法更新 ima 模型选择")
        })?;
    for (name, selection) in SCENES.into_iter().zip(selections) {
        if selection.model_id.is_empty() || selection.model_type < 0 {
            return Err(
                CommandError::new("ima_selection_invalid", "ima 返回的模型选择无效")
                    .with_recovery("请在 ima 中确认模型可用后重试。"),
            );
        }
        let scene = extra
            .entry(name)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(unsupported_shape)?;
        scene.insert(
            "modelId".to_owned(),
            Value::String(selection.model_id.clone()),
        );
        scene.insert("modelType".to_owned(), Value::from(selection.model_type));
        scene.insert("timestamp".to_owned(), Value::from(timestamp));
    }
    encode_preferences(&mut root, extra, true)
}

pub(crate) fn build_restore(
    preferences: &Path,
    snapshot: &LocalSelectionSnapshot,
) -> AppResult<Vec<u8>> {
    let mut root = read_preferences(preferences)?;
    let mut extra = read_extra(&root)?;
    for (name, saved) in SCENES.into_iter().zip(&snapshot.scenes) {
        snapshot_scene(&extra, name)?;
        let scene = extra
            .entry(name)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(unsupported_shape)?;
        restore_field(scene, "modelId", &saved.model_id)?;
        restore_field(scene, "modelType", &saved.model_type)?;
        restore_field(scene, "timestamp", &saved.timestamp)?;
        if scene.is_empty() && !saved.existed {
            extra.remove(name);
        }
    }
    encode_preferences(&mut root, extra, snapshot.extra_settings_existed)
}

fn read_preferences(preferences: &Path) -> AppResult<Map<String, Value>> {
    let bytes = fs::read(preferences).map_err(|_| {
        CommandError::new("agent_config_unreadable", "无法读取 ima 本地配置")
            .with_recovery("请先打开并登录 ima，然后重试。")
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        CommandError::new("agent_config_unparseable", "ima 本地配置不是有效的 JSON")
            .with_recovery("请先在 ima 中确认设置正常；AT-Switch 未修改该配置。")
    })?;
    value.as_object().cloned().ok_or_else(unsupported_shape)
}

fn read_extra(root: &Map<String, Value>) -> AppResult<Map<String, Value>> {
    let Some(value) = root.get(EXTRA_SETTINGS) else {
        return Ok(Map::new());
    };
    let text = value.as_str().ok_or_else(unsupported_shape)?;
    let parsed: Value = serde_json::from_str(text).map_err(|_| unsupported_shape())?;
    parsed.as_object().cloned().ok_or_else(unsupported_shape)
}

fn snapshot_scene(extra: &Map<String, Value>, name: &str) -> AppResult<SceneSnapshot> {
    let empty = Map::new();
    let scene = match extra.get(name) {
        None => &empty,
        Some(value) => value.as_object().ok_or_else(unsupported_shape)?,
    };
    Ok(SceneSnapshot {
        existed: extra.contains_key(name),
        model_id: read_field(scene, "modelId", Value::as_str)?.map(str::to_owned),
        model_type: read_field(scene, "modelType", Value::as_i64)?,
        timestamp: read_field(scene, "timestamp", Value::as_i64)?,
    })
}

impl<T> SavedField<T> {
    fn map<U>(self, convert: impl FnOnce(T) -> U) -> SavedField<U> {
        match self {
            Self::Missing => SavedField::Missing,
            Self::Null => SavedField::Null,
            Self::Value(value) => SavedField::Value(convert(value)),
        }
    }
}

fn read_field<'a, T>(
    scene: &'a Map<String, Value>,
    name: &str,
    decode: impl FnOnce(&'a Value) -> Option<T>,
) -> AppResult<SavedField<T>> {
    match scene.get(name) {
        None => Ok(SavedField::Missing),
        Some(Value::Null) => Ok(SavedField::Null),
        Some(value) => decode(value)
            .map(SavedField::Value)
            .ok_or_else(unsupported_shape),
    }
}

fn restore_field<T: Serialize>(
    scene: &mut Map<String, Value>,
    name: &str,
    saved: &SavedField<T>,
) -> AppResult<()> {
    match saved {
        SavedField::Missing => {
            scene.remove(name);
        }
        SavedField::Null => {
            scene.insert(name.to_owned(), Value::Null);
        }
        SavedField::Value(value) => {
            scene.insert(
                name.to_owned(),
                serde_json::to_value(value).map_err(|_| serialization_error())?,
            );
        }
    }
    Ok(())
}

fn encode_preferences(
    root: &mut Map<String, Value>,
    extra: Map<String, Value>,
    originally_existed: bool,
) -> AppResult<Vec<u8>> {
    if extra.is_empty() && !originally_existed {
        root.remove(EXTRA_SETTINGS);
    } else {
        let encoded = serde_json::to_string(&extra).map_err(|_| serialization_error())?;
        root.insert(EXTRA_SETTINGS.to_owned(), Value::String(encoded));
    }
    serde_json::to_vec(root).map_err(|_| serialization_error())
}

fn unsupported_shape() -> CommandError {
    CommandError::new(
        "agent_config_shape_unsupported",
        "当前 ima 本地模型配置格式暂不支持",
    )
    .with_recovery("请更新 ima 和 AT-Switch 后重试；现有配置未被修改。")
}

fn serialization_error() -> CommandError {
    CommandError::new(
        "ima_config_serialization_failed",
        "无法生成 ima 本地模型配置",
    )
}

fn verification_error() -> CommandError {
    CommandError::new("ima_local_verify_failed", "ima 本地模型选择校验失败")
        .with_recovery("请重试恢复原配置；AT-Switch 将保留恢复记录。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preferences(root: Value) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
        (directory, path)
    }

    fn root_with_extra(extra: Value) -> Value {
        json!({
            "kExtraSettingInfo": extra.to_string(),
            "login_metadata": {"untouched": "example-account-secret"},
            "unknownRoot": [1, 2, 3]
        })
    }

    fn selections() -> [Selection; 2] {
        [
            Selection {
                model_id: "example-custom-qa".into(),
                model_type: 200000,
            },
            Selection {
                model_id: "example-custom-copilot".into(),
                model_type: 200000,
            },
        ]
    }

    fn decode_extra(bytes: &[u8]) -> Value {
        let root: Value = serde_json::from_slice(bytes).unwrap();
        serde_json::from_str(root[EXTRA_SETTINGS].as_str().unwrap()).unwrap()
    }

    #[test]
    fn patch_both_scenes_preserves_unknown_fields_and_does_not_write() {
        let root = root_with_extra(json!({
            "modelConfig": {"modelId":"original-qa","modelType":1,"timestamp":7,
                "enableEnhancement":false,"modelOptions":[{"id":1}],"future":{"keep":true}},
            "copilotModelConfig": {"modelId":"original-copilot","modelType":100000,
                "futureFlag":"keep"},
            "modelType":2,
            "otherSetting":{"keep":true}
        }));
        let (_directory, path) = preferences(root.clone());
        let before = fs::read(&path).unwrap();
        let candidate = build_selection(&path, &selections()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        let actual: Value = serde_json::from_slice(&candidate).unwrap();
        assert_eq!(actual["unknownRoot"], root["unknownRoot"]);
        assert_eq!(actual["login_metadata"], root["login_metadata"]);
        let extra = decode_extra(&candidate);
        assert_eq!(extra["modelConfig"]["modelId"], "example-custom-qa");
        assert_eq!(
            extra["copilotModelConfig"]["modelId"],
            "example-custom-copilot"
        );
        assert_eq!(extra["modelConfig"]["enableEnhancement"], false);
        assert_eq!(extra["modelConfig"]["future"], json!({"keep":true}));
        assert_eq!(extra["modelConfig"]["modelOptions"], json!([{"id":1}]));
        assert_eq!(extra["otherSetting"], json!({"keep":true}));
        assert_eq!(extra["modelType"], 2);
    }

    #[test]
    fn restore_exact_controlled_values_preserves_subsequent_unknown_edits() {
        let original = json!({
            "modelConfig":{"modelId":"original-qa","modelType":1,"timestamp":9},
            "copilotModelConfig":{"modelId":null,"modelType":null,"future":true}
        });
        let (_directory, path) = preferences(root_with_extra(original.clone()));
        let baseline = snapshot(&path).unwrap();
        let serialized = serde_json::to_vec(&baseline).unwrap();
        let restored_baseline: LocalSelectionSnapshot =
            serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored_baseline, baseline);
        let mut changed = decode_extra(&build_selection(&path, &selections()).unwrap());
        changed["newUserSetting"] = json!({"addedAfterSwitch":true});
        changed["modelConfig"]["newUserFlag"] = json!(true);
        fs::write(&path, root_with_extra(changed).to_string()).unwrap();
        let restored = decode_extra(&build_restore(&path, &restored_baseline).unwrap());
        assert_eq!(restored["modelConfig"]["modelId"], "original-qa");
        assert_eq!(restored["modelConfig"]["timestamp"], 9);
        assert_eq!(restored["modelConfig"]["newUserFlag"], true);
        assert_eq!(
            restored["copilotModelConfig"],
            original["copilotModelConfig"]
        );
        assert_eq!(restored["newUserSetting"], json!({"addedAfterSwitch":true}));
    }

    #[test]
    fn missing_containers_are_created_and_removed_without_replacing_preferences() {
        let original = json!({"unknownRoot":{"keep":true}});
        let (_directory, path) = preferences(original.clone());
        let baseline = snapshot(&path).unwrap();
        let candidate = build_selection(&path, &selections()).unwrap();
        assert_eq!(
            decode_extra(&candidate)["modelConfig"]["modelId"],
            "example-custom-qa"
        );
        fs::write(&path, candidate).unwrap();
        let restored: Value =
            serde_json::from_slice(&build_restore(&path, &baseline).unwrap()).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn restore_keeps_new_unknown_fields_inside_created_containers() {
        let (_directory, path) = preferences(json!({}));
        let baseline = snapshot(&path).unwrap();
        let mut changed = decode_extra(&build_selection(&path, &selections()).unwrap());
        changed["modelConfig"]["newUserFlag"] = json!(true);
        fs::write(&path, root_with_extra(changed).to_string()).unwrap();
        let restored = decode_extra(&build_restore(&path, &baseline).unwrap());
        assert_eq!(restored, json!({"modelConfig":{"newUserFlag":true}}));
    }

    #[test]
    fn snapshot_contains_only_typed_selection_values() {
        let (_directory, path) = preferences(root_with_extra(json!({
            "modelConfig":{"modelId":"original-qa","modelType":1,"apiKey":"example-model-secret"},
            "copilotModelConfig":{"modelId":"original-copilot","modelType":100000},
            "token":"example-token-secret"
        })));
        let captured = serde_json::to_string(&snapshot(&path).unwrap()).unwrap();
        for secret in [
            "example-account-secret",
            "example-model-secret",
            "example-token-secret",
        ] {
            assert!(!captured.contains(secret));
        }
    }

    #[test]
    fn malformed_or_unknown_shapes_fail_without_exposing_values() {
        for root in [
            json!({"kExtraSettingInfo":"example-private-invalid-json"}),
            json!({"kExtraSettingInfo":{}}),
            root_with_extra(json!({"modelConfig":[]})),
            root_with_extra(json!({"modelConfig":{"modelId":{"secret":"example-private"}}})),
            root_with_extra(json!({"modelConfig":{"timestamp":"example-private"}})),
        ] {
            let (_directory, path) = preferences(root);
            let original = fs::read(&path).unwrap();
            let error = build_selection(&path, &selections()).unwrap_err();
            assert_eq!(error.code, "agent_config_shape_unsupported");
            assert!(!error.message.contains("example-private"));
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn repeated_selection_and_restore_do_not_accumulate_entries() {
        let (_directory, path) = preferences(root_with_extra(json!({})));
        let baseline = snapshot(&path).unwrap();
        for _ in 0..3 {
            fs::write(&path, build_selection(&path, &selections()).unwrap()).unwrap();
        }
        let selected = read_extra(&read_preferences(&path).unwrap()).unwrap();
        assert_eq!(selected.len(), 2);
        for name in SCENES {
            assert_eq!(selected[name].as_object().unwrap().len(), 3);
        }
        fs::write(&path, build_restore(&path, &baseline).unwrap()).unwrap();
        assert_eq!(snapshot(&path).unwrap(), baseline);
    }

    #[test]
    fn missing_preferences_and_invalid_json_have_safe_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        assert_eq!(snapshot(&path).unwrap_err().code, "agent_config_unreadable");
        fs::write(&path, "example-private-invalid-json").unwrap();
        assert_eq!(
            snapshot(&path).unwrap_err().code,
            "agent_config_unparseable"
        );
    }

    #[test]
    fn verification_detects_wrong_scene_and_accepts_preserved_new_fields() {
        let (_directory, path) = preferences(json!({}));
        let baseline = snapshot(&path).unwrap();
        assert_eq!(
            verify_selection(&path, &selections()).unwrap_err().code,
            "ima_local_verify_failed"
        );
        fs::write(&path, build_selection(&path, &selections()).unwrap()).unwrap();
        verify_selection(&path, &selections()).unwrap();
        assert_eq!(
            verify_snapshot(&path, &baseline).unwrap_err().code,
            "ima_local_verify_failed"
        );
        let extra = json!({"modelConfig": {"newUserFlag": true}});
        fs::write(&path, root_with_extra(extra).to_string()).unwrap();
        verify_snapshot(&path, &baseline).unwrap();
    }

    #[test]
    fn remapping_owned_model_ids_preserves_other_scene_and_timestamp() {
        let (_directory, path) = preferences(root_with_extra(json!({
            "modelConfig":{"modelId":"owned-old","modelType":1000000,"timestamp":19},
            "copilotModelConfig":{"modelId":"user-original","modelType":100000,"timestamp":23}
        })));
        let original = snapshot(&path).unwrap();
        let remapped = remap_models(
            &original,
            &[(
                "owned-old".into(),
                Selection {
                    model_id: "owned-recreated".into(),
                    model_type: 1000000,
                },
            )],
        );
        assert_eq!(
            original.scenes[0].model_id,
            SavedField::Value("owned-old".into())
        );
        assert_eq!(
            remapped.scenes[0].model_id,
            SavedField::Value("owned-recreated".into())
        );
        assert_eq!(remapped.scenes[0].timestamp, original.scenes[0].timestamp);
        assert_eq!(remapped.scenes[1], original.scenes[1]);
    }
}
