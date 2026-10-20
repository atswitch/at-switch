use std::{fs, path::PathBuf};

use serde_json::{json, Value};
use tempfile::tempdir;

use super::*;
use crate::domain::{AgentConfigHealth, AgentInstallStatus};

fn detection(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: "zcode",
        display_name: "ZCode",
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
    }
}

fn desired<'a>() -> DesiredAgentBinding<'a> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "GLM-5.2",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.example.com/v1/",
        credential: "test-key",
    }
}

fn managed_provider(value: &Value) -> Option<&Value> {
    value
        .pointer("/config/providerConfigRules/providerRules")
        .and_then(Value::as_array)
        .and_then(|rules| rules.iter().find(|rule| managed_id(rule)))
}

#[test]
fn build_config_writes_provider_model_and_default_selection() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);
    let desired = desired();

    let bytes = ZCodeAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let value: Value = serde_json::from_slice(&bytes).expect("valid json");

    let provider = managed_provider(&value).expect("managed provider");
    assert_eq!(
        provider.pointer("/config/api/type").and_then(Value::as_str),
        Some("openai-chat-completions")
    );
    assert_eq!(
        provider
            .pointer("/config/api/baseUrl")
            .and_then(Value::as_str),
        Some("https://api.example.com/v1")
    );
    assert_eq!(
        provider
            .pointer("/config/access/apiKey")
            .and_then(Value::as_str),
        Some("test-key")
    );
    assert_eq!(
        value
            .pointer("/config/defaultModelSelection/modelId")
            .and_then(Value::as_str),
        Some("GLM-5.2")
    );
    assert_eq!(
        value.pointer("/config/providerOrder"),
        Some(&json!(["at-switch"])),
        "managed provider must lead providerOrder"
    );
}

#[test]
fn provider_uses_the_personal_group_and_declares_its_model() {
    let temp = tempdir().expect("temp");
    let detection = detection(temp.path().join("provider_config.json"));

    let bytes = ZCodeAdapter
        .build_config(&detection, &desired())
        .expect("build config");
    let value: Value = serde_json::from_slice(&bytes).expect("valid json");

    let provider = managed_provider(&value).expect("managed provider");

    // ZCode only honours personal providers, and models belong on the provider
    // itself: `manualProviderModelRules` demands a capability block and makes
    // the whole personal file fail to load.
    assert_eq!(
        provider.pointer("/config/group").and_then(Value::as_str),
        Some("standard-personal")
    );
    assert_eq!(
        provider.pointer("/config/personalModelIds"),
        Some(&json!(["GLM-5.2"]))
    );
    assert_eq!(
        provider.pointer("/config/modelOrder"),
        Some(&json!(["GLM-5.2"]))
    );
    assert!(
        value
            .pointer("/config/modelConfigRules/manualProviderModelRules")
            .and_then(Value::as_array)
            .is_some_and(|rules| rules.iter().all(|rule| !managed_id(rule))),
        "no managed entry may be written to manualProviderModelRules"
    );
}

#[test]
fn switching_models_keeps_previously_declared_models() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);

    let first = ZCodeAdapter
        .build_config(&detection, &desired())
        .expect("first build");
    fs::write(detection.config_path.as_ref().expect("path"), &first).expect("write");

    let mut second = desired();
    second.model_id = "GLM-5.3";
    let bytes = ZCodeAdapter
        .build_config(&detection, &second)
        .expect("second build");
    let value: Value = serde_json::from_slice(&bytes).expect("valid json");
    let provider = managed_provider(&value).expect("managed provider");

    let ids = provider
        .pointer("/config/personalModelIds")
        .and_then(Value::as_array)
        .expect("personalModelIds");
    let ids: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();

    // Tasks pin their own model; dropping the old one makes it vanish from
    // ZCode and forces the user to reselect.
    assert!(
        ids.contains(&"GLM-5.2"),
        "previously switched model must stay declared"
    );
    assert!(ids.contains(&"GLM-5.3"));
    assert_eq!(
        provider
            .pointer("/config/modelOrder")
            .and_then(Value::as_array)
            .and_then(|order| order.first())
            .and_then(Value::as_str),
        Some("GLM-5.3"),
        "the newly selected model leads the order"
    );
    assert_eq!(
        ids.first().copied(),
        Some("GLM-5.3"),
        "ZCode uses personalModelIds[0] for new tasks, so the target must lead"
    );
}

#[test]
fn switching_provider_resets_declared_models() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);

    let first = ZCodeAdapter
        .build_config(&detection, &desired())
        .expect("first build");
    fs::write(detection.config_path.as_ref().expect("path"), &first).expect("write");

    let mut other_provider = desired();
    other_provider.base_url = "https://api.other.example.com/v1";
    let bytes = ZCodeAdapter
        .build_config(&detection, &other_provider)
        .expect("second build");
    let value: Value = serde_json::from_slice(&bytes).expect("valid json");
    let provider = managed_provider(&value).expect("managed provider");

    assert_eq!(
        provider.pointer("/config/personalModelIds"),
        Some(&json!(["GLM-5.2"])),
        "a different endpoint means a different provider, so stale models must not be kept"
    );
}

#[test]
fn repeated_switch_is_idempotent() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);
    let desired = desired();

    let first = ZCodeAdapter
        .build_config(&detection, &desired)
        .expect("first build");
    fs::write(detection.config_path.as_ref().expect("path"), &first).expect("write");

    let second = ZCodeAdapter
        .build_config(&detection, &desired)
        .expect("second build");
    let value: Value = serde_json::from_slice(&second).expect("valid json");

    assert_eq!(
        value
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1),
        "switching again must replace the managed provider instead of appending"
    );
    assert_eq!(
        value
            .pointer("/config/providerOrder")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1),
        "providerOrder must not accumulate duplicate entries"
    );
}

#[test]
fn unknown_fields_and_other_providers_are_preserved() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "config": {
                "providerOrder": ["custom-one"],
                "providerConfigRules": {
                    "providerRules": [
                        { "providerId": "custom-one", "providerName": "Custom" }
                    ]
                },
                "modelConfigRules": {
                    "manualProviderModelRules": [
                        { "providerId": "custom-one", "modelId": "custom-model" }
                    ]
                }
            },
            "vendorOnlyField": { "keep": true }
        }))
        .expect("serialize"),
    )
    .expect("write");
    let detection = detection(path);

    let bytes = ZCodeAdapter
        .build_config(&detection, &desired())
        .expect("build config");
    let value: Value = serde_json::from_slice(&bytes).expect("valid json");

    assert_eq!(
        value.get("vendorOnlyField").and_then(|v| v.get("keep")),
        Some(&json!(true)),
        "unknown top-level fields must survive untouched"
    );
    assert_eq!(
        value.pointer("/config/providerOrder"),
        Some(&json!(["at-switch", "custom-one"])),
        "managed provider leads while the user's ordering is preserved"
    );
    assert!(
        value
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .is_some_and(|rules| rules
                .iter()
                .any(|rule| rule.get("providerId").and_then(Value::as_str) == Some("custom-one"))),
        "user-owned providers must be preserved"
    );
    assert_eq!(
        value
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn native_config_removes_only_managed_entries() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);
    let desired = desired();

    let applied = ZCodeAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let mut with_user_provider: Value = serde_json::from_slice(&applied).expect("valid json");
    with_user_provider
        .pointer_mut("/config/providerConfigRules/providerRules")
        .and_then(Value::as_array_mut)
        .expect("provider rules")
        .push(json!({ "providerId": "custom-one", "providerName": "Custom" }));
    fs::write(
        detection.config_path.as_ref().expect("path"),
        serde_json::to_vec_pretty(&with_user_provider).expect("serialize"),
    )
    .expect("write");

    let restored = ZCodeAdapter
        .build_native_config(
            &detection,
            &BaselineSnapshot {
                existed: true,
                content: serde_json::to_vec_pretty(&with_user_provider).expect("serialize"),
            },
        )
        .expect("native config");
    let value: Value = serde_json::from_slice(&restored).expect("valid json");

    assert!(
        managed_provider(&value).is_none(),
        "managed provider must be removed on restore"
    );
    assert_eq!(
        value.get("defaultModelSelection"),
        None,
        "default selection pointing at AT-Switch must be cleared"
    );
    assert!(
        value
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .is_some_and(|rules| rules
                .iter()
                .any(|rule| rule.get("providerId").and_then(Value::as_str) == Some("custom-one"))),
        "user-owned providers must survive restore"
    );
}

#[test]
fn verify_config_accepts_the_applied_configuration() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);
    let desired = desired();

    let bytes = ZCodeAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    fs::write(detection.config_path.as_ref().expect("path"), &bytes).expect("write");

    ZCodeAdapter
        .verify_config(&detection, &desired)
        .expect("verification should pass after applying");
}

#[test]
fn verify_config_rejects_a_tampered_configuration() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("provider_config.json");
    let detection = detection(path);
    let binding = desired();

    let bytes = ZCodeAdapter
        .build_config(&detection, &binding)
        .expect("build config");
    fs::write(detection.config_path.as_ref().expect("path"), &bytes).expect("write");

    let mut other_model = desired();
    other_model.model_id = "other-model";

    ZCodeAdapter
        .verify_config(&detection, &other_model)
        .expect_err("verification must fail for another model");

    // Sanity: the applied binding still verifies against the same file.
    ZCodeAdapter
        .verify_config(&detection, &binding)
        .expect("original binding still verifies");
}

#[test]
fn all_supported_protocols_map_to_zcode_api_types() {
    assert_eq!(
        api_type_for(ApiProtocol::OpenaiChatCompletions).expect("chat"),
        "openai-chat-completions"
    );
    assert_eq!(
        api_type_for(ApiProtocol::OpenaiResponses).expect("responses"),
        "openai-responses"
    );
    assert_eq!(
        api_type_for(ApiProtocol::AnthropicMessages).expect("messages"),
        "anthropic-messages"
    );
}

#[test]
fn proxy_mode_falls_back_to_openai_chat() {
    assert_eq!(
        ZCodeAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::AnthropicMessages),
        ApiProtocol::OpenaiChatCompletions
    );
    assert_eq!(
        ZCodeAdapter.source_protocol(AgentBindingMode::Direct, ApiProtocol::AnthropicMessages),
        ApiProtocol::AnthropicMessages
    );
}

#[test]
fn empty_base_url_is_rejected_before_writing() {
    let temp = tempdir().expect("temp");
    let detection = detection(temp.path().join("provider_config.json"));
    let mut desired = desired();
    desired.base_url = "  ";

    ZCodeAdapter
        .build_config(&detection, &desired)
        .expect_err("empty base url must be rejected");
}

#[test]
fn rewrite_model_selection_rewrites_only_managed_records() {
    let value = br#"{"a":{"providerId":"at-switch","modelId":"old"},"b":{"providerId":"other","modelId":"keep"}}"#;

    let rewritten = rewrite_model_selection(value, "new").expect("rewritten");
    let text = String::from_utf8(rewritten).expect("utf8");

    assert!(text.contains(r#""providerId":"at-switch","modelId":"new""#));
    assert!(
        text.contains(r#""providerId":"other","modelId":"keep""#),
        "other providers must stay byte-identical"
    );
}

#[test]
fn rewrite_model_selection_ignores_unmanaged_values() {
    let value = br#"{"providerId":"other","modelId":"keep"}"#;

    assert!(rewrite_model_selection(value, "new").is_none());
}

#[test]
fn apply_and_restore_model_selection_round_trip() {
    let temp = tempdir().expect("temp");
    let storage = temp.path().join("leveldb");
    fs::create_dir_all(&storage).expect("leveldb dir");

    let key: &[u8] = b"_file://\x00\x01model-selection";
    {
        let options = LevelDbOptions {
            create_if_missing: true,
            ..LevelDbOptions::default()
        };
        let mut database = LevelDb::open(&storage, options).expect("open for seed");
        database
            .put(key, br#"{"providerId":"at-switch","modelId":"old"}"#)
            .expect("seed");
        database.close().expect("close");
    }

    let detection = AgentDetection {
        runtime_data_dir: Some(storage.clone()),
        ..detection(temp.path().join("provider_config.json"))
    };

    let changes = apply_model_selection(&detection, "new-model").expect("apply");
    assert_eq!(changes.len(), 1, "the managed record must be rewritten");

    let read_stored = || {
        let options = LevelDbOptions {
            create_if_missing: false,
            ..LevelDbOptions::default()
        };
        let mut database = LevelDb::open(&storage, options).expect("open for read");
        let value = database.get(key).expect("value");
        database.close().expect("close");
        String::from_utf8(value.to_vec()).expect("utf8")
    };

    assert!(read_stored().contains(r#""modelId":"new-model""#));

    restore_model_selection(&detection, &changes).expect("restore");
    assert!(
        read_stored().contains(r#""modelId":"old""#),
        "rollback must put the previous selection back"
    );
}

#[test]
fn apply_and_restore_task_model_round_trip() {
    let temp = tempdir().expect("temp");
    let database_path = temp.path().join("tasks-index.sqlite");
    {
        let connection = Connection::open(&database_path).expect("open");
        connection
            .execute_batch(
                "CREATE TABLE tasks (task_id TEXT PRIMARY KEY, model TEXT);
                 INSERT INTO tasks VALUES ('a', 'at-switch/old');
                 INSERT INTO tasks VALUES ('b', 'other/model');",
            )
            .expect("seed");
    }

    let detection = detection(temp.path().join("provider_config.json"));
    let changes = apply_task_model(&detection, "new-model").expect("apply");
    assert_eq!(changes.len(), 1, "only AT-Switch-owned tasks are repointed");

    let read = |task: &str| -> String {
        let connection = Connection::open(&database_path).expect("open");
        connection
            .query_row(
                "SELECT model FROM tasks WHERE task_id = ?1",
                [task],
                |row| row.get(0),
            )
            .expect("row")
    };

    assert_eq!(read("a"), "at-switch/new-model");
    assert_eq!(read("b"), "other/model", "other tasks must stay untouched");

    restore_task_model(&detection, &changes).expect("restore");
    assert_eq!(read("a"), "at-switch/old");
}
