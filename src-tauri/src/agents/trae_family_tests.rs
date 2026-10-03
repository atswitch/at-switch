use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth};

fn seed_database(path: &Path, catalogue: Value, selection: Option<Value>) {
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    let connection = Connection::open(path).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_vec(&catalogue).expect("encode")
            ],
        )
        .expect("catalogue");
    if let Some(selection) = selection {
        connection
            .execute(
                "INSERT INTO ItemTable VALUES (?1, ?2)",
                params![
                    "3129680676791939:AI.agent.model.recent_user_selection_by_agent_label",
                    serde_json::to_vec(&selection).expect("encode")
                ],
            )
            .expect("selection");
    }
    connection.close().expect("close");
}

fn custom_model(display_name: &str) -> Value {
    json!({
        "name": format!("custom_openai_compatible//{display_name}"),
        "display_name": display_name,
        "provider": "custom_openai_compatible",
        "base_url": "https://api.example.com/v1/chat/completions",
        "ak": "<encrypted>",
    })
}

#[test]
fn custom_models_are_deduplicated_across_segments() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    seed_database(
        &database,
        json!({
            "builder": [custom_model("glm-5.1")],
            "builder_v3": [custom_model("glm-5.1")],
            "solo_agent": [custom_model("glm-5.1"), custom_model("NMauto")]
        }),
        None,
    );

    let (models, selection) = read_state(&database).expect("read state");

    let names: Vec<&str> = models
        .iter()
        .map(|model| model.display_name.as_str())
        .collect();
    assert_eq!(names, vec!["NMauto", "glm-5.1"]);
    assert!(selection.is_none());
}

#[test]
fn builtin_models_are_ignored() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    seed_database(
        &database,
        json!({
            "solo_agent": [
                { "name": "Doubao-Seed", "provider": "", "base_url": "" },
                custom_model("glm-5.1")
            ]
        }),
        None,
    );

    let (models, _) = read_state(&database).expect("read state");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].display_name, "glm-5.1");
}

#[test]
fn active_selection_is_surfaced_in_the_summary() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    seed_database(
        &database,
        json!({ "solo_agent": [custom_model("glm-5.1")] }),
        Some(json!({
            "solo_agent": { "modelId": "solo_agent_3_custom_openai_compatible_glm-5.1_2765316226", "mode": 0 }
        })),
    );

    let (models, selection) = read_state(&database).expect("read state");
    let summary = summarize(&models, selection.as_deref());

    assert!(summary.contains("glm-5.1"));
    assert!(summary.contains("当前选中"));
    assert!(
        !summary.contains("2765316226"),
        "the opaque id must not leak into the summary"
    );
}

#[test]
fn a_missing_catalogue_is_reported_as_not_found() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    connection.close().expect("close");

    let error = read_state(&database).expect_err("no catalogue");

    assert_eq!(error.code, "trae_not_found");
}

#[test]
fn bindings_are_validated_against_traes_own_catalogue() {
    let detection = AgentDetection {
        id: TRAEWORK_ADAPTER.id(),
        display_name: TRAEWORK_ADAPTER.display_name(),
        installation: None,
        config_path: None,
        runtime_data_dir: None,
        install_status: crate::domain::AgentInstallStatus::Installed,
        config_health: AgentConfigHealth::Healthy,
        write_supported: false,
        needs_restart: false,
        message: None,
        custom_install_path: None,
        using_custom_install_path: false,
    };
    let desired = DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "GLM-5.2",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.example.com/v1",
        credential: "test-key",
    };

    // Trae CN supports switching: the requested model is checked against its
    // own catalogue, and credentials are never touched.
    TRAEWORK_ADAPTER
        .validate_binding(&desired)
        .expect("Trae no longer rejects bindings outright");
    assert!(
        TRAEWORK_ADAPTER.config_write_target(&detection).is_none(),
        "without a database there is nothing to reconcile"
    );
}

#[test]
fn adapter_metadata_is_stable() {
    assert_eq!(TRAEWORK_ADAPTER.id(), "traework");
    assert_eq!(TRAEWORK_ADAPTER.display_name(), "Trae CN");
    assert_eq!(TRAECODE_ADAPTER.id(), "traecode");
    assert_eq!(TRAECODE_ADAPTER.display_name(), "TRAE SOLO CN");
}

fn detection_at(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: TRAEWORK_ADAPTER.id(),
        display_name: TRAEWORK_ADAPTER.display_name(),
        installation: None,
        config_path: Some(config_path),
        runtime_data_dir: None,
        install_status: crate::domain::AgentInstallStatus::Installed,
        config_health: AgentConfigHealth::Healthy,
        write_supported: true,
        needs_restart: false,
        message: None,
        custom_install_path: None,
        using_custom_install_path: false,
    }
}

#[test]
fn the_transaction_target_is_the_trace_file_not_the_database() {
    let detection = detection_at(PathBuf::from("/tmp/trae/User/globalStorage/state.vscdb"));

    let target = TRAEWORK_ADAPTER
        .config_write_target(&detection)
        .expect("trace target");

    assert_eq!(
        target,
        PathBuf::from("/tmp/trae/User/globalStorage/at-switch-trae-state.json")
    );
    assert_ne!(
        detection.config_path.as_ref(),
        Some(&target),
        "the SQLite database must never be the transaction target"
    );
}

#[test]
fn building_config_rejects_models_trae_does_not_have() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    seed_database(&database, nmauto_catalogue(), None);
    let detection = detection_at(database);
    let desired = DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "not-configured",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.example.com/v1",
        credential: "test-key",
    };

    let error = TRAEWORK_ADAPTER
        .build_config(&detection, &desired)
        .expect_err("model is not configured inside Trae");

    assert_eq!(error.code, "trae_model_not_configured");
}

#[test]
fn reads_catalogues_stored_as_text() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    // VS Code-family apps commonly write this JSON as TEXT, and sqlite reports
    // it as such; reading it as a blob fails outright.
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_string(&nmauto_catalogue()).expect("encode")
            ],
        )
        .expect("catalogue");
    connection.close().expect("close");

    let detection = detection_at(database);
    let desired = DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "NMauto",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.g2claw.com/v1",
        credential: "test-key",
    };

    let trace = TRAEWORK_ADAPTER
        .build_config(&detection, &desired)
        .expect("a TEXT-stored catalogue must be readable");

    assert!(String::from_utf8_lossy(&trace).contains("NMauto"));
}

/// Trae 在 `User/workspaceStorage/<hash>/state.vscdb` 里另存了一份
/// `globalModelMap`，界面读取的很可能就是它；只改 globalStorage 是不够的。
#[test]
fn rewrites_the_global_model_map_inside_workspace_databases() {
    let temp = tempdir().expect("temp");
    let user_dir = temp.path().join("User");
    let global = user_dir.join("globalStorage/state.vscdb");
    let workspace = user_dir.join("workspaceStorage/abc123/state.vscdb");
    for path in [&global, &workspace] {
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    }
    // 主库：完整的模型清单 + 选中态。
    let connection = Connection::open(&global).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_string(&nmauto_catalogue()).expect("encode")
            ],
        )
        .expect("catalogue");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939:AI.agent.model.recent_user_selection_by_agent_label",
                serde_json::to_string(&json!({
                    "solo_agent": { "modelId": "solo_agent_3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226", "mode": 0 }
                }))
                .expect("encode")
            ],
        )
        .expect("selection");
    connection.close().expect("close");
    // 工作区库：只有一份 globalModelMap，且仍指向旧模型。
    let connection = Connection::open(&workspace).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_ai-chat:sessionRelation:globalModelMap",
                serde_json::to_string(&json!({
                    "dev_builder": "3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226"
                }))
                .expect("encode")
            ],
        )
        .expect("workspace map");
    connection.close().expect("close");

    let detection = detection_at(global.clone());
    let changes = apply_selection(&detection, "NMauto").expect("apply");

    // 工作区库也必须被改写，并且改动要记录所属库以便回滚。
    assert!(
        changes.iter().any(|change| change.database.is_some()),
        "workspace database must be part of the change set"
    );
    let connection = Connection::open(&workspace).expect("open");
    let stored: String = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key LIKE '%globalModelMap%'",
            [],
            |row| row.get(0),
        )
        .expect("workspace value");
    connection.close().expect("close");
    assert!(
        stored.contains("//NMauto_2751100546"),
        "workspace map should point at the target model, got: {stored}"
    );

    // 回滚后工作区库应恢复原值。
    restore_selection(&detection, &changes).expect("restore");
    let connection = Connection::open(&workspace).expect("open");
    let restored: String = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key LIKE '%globalModelMap%'",
            [],
            |row| row.get(0),
        )
        .expect("workspace value");
    connection.close().expect("close");
    assert!(restored.contains("//glm-5.1_2765316226"), "got: {restored}");
}

/// `globalModelMap` 是界面读取的另一套存储：值不带 label 前缀，
/// 且只应改写已经指向自定义 Provider 的条目。
#[test]
fn rewrites_the_global_model_map_without_a_label_prefix() {
    let raw = serde_json::to_vec(&json!({
        "solo_coder": "3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226",
        "dev_builder": "3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546",
        "builtin_agent": "1__Doubao_1_6"
    }))
    .expect("encode");
    let entry = find_model_entry(&nmauto_catalogue(), "NMauto").expect("entry");

    let updated = rewrite_global_model_map(&raw, &entry).expect("should change");
    let value: Value = serde_json::from_slice(&updated).expect("parse");

    // 值不带 label 前缀，形如 `{config_source}_{provider}_{name}_{id}`。
    let expected = "3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546";
    assert_eq!(
        value.get("solo_coder").and_then(Value::as_str),
        Some(expected)
    );
    assert_eq!(
        value.get("dev_builder").and_then(Value::as_str),
        Some(expected)
    );
    // 指向内置模型的条目保持原样。
    assert_eq!(
        value.get("builtin_agent").and_then(Value::as_str),
        Some("1__Doubao_1_6")
    );
}

/// 已经指向目标模型时不应产生写入。
#[test]
fn an_already_targeted_global_model_map_is_a_no_op() {
    let entry = find_model_entry(&nmauto_catalogue(), "NMauto").expect("entry");
    let expected = global_model_id(&entry);
    let raw = serde_json::to_vec(&json!({ "solo_coder": expected })).expect("encode");

    assert!(rewrite_global_model_map(&raw, &entry).is_none());
}

/// `read_state` 是切换面板列模型时走的路径，必须同样容忍 TEXT 存储。
#[test]
fn reads_the_configured_models_from_a_text_stored_catalogue() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    // VS Code 系应用常把 JSON 存成 TEXT；按 blob 读取会直接失败。
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_string(&nmauto_catalogue()).expect("encode")
            ],
        )
        .expect("catalogue");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939:AI.agent.model.recent_user_selection_by_agent_label",
                serde_json::to_string(&serde_json::json!({
                    "solo_agent": { "modelId": "solo_agent_3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546", "mode": 0 }
                }))
                .expect("encode")
            ],
        )
        .expect("selection");
    connection.close().expect("close");

    let (models, active) = read_state(&database).expect("a TEXT catalogue must be readable");

    assert!(models.iter().any(|model| model.display_name == "NMauto"));
    assert!(active.is_some());
}

#[test]
fn merges_the_parallel_model_list_map_copies() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    // TRAE SOLO CN keeps a second map under a colon-prefixed key, and the two
    // copies diverge — a model configured in one is missing from the other.
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939:AI.agent.model.model_list_map",
                serde_json::to_string(&serde_json::json!({})).expect("encode")
            ],
        )
        .expect("colon copy");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_string(&nmauto_catalogue()).expect("encode")
            ],
        )
        .expect("underscore copy");
    connection.close().expect("close");

    let detection = detection_at(database);
    let desired = DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "NMauto",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.g2claw.com/v1",
        credential: "test-key",
    };

    TRAEWORK_ADAPTER
        .build_config(&detection, &desired)
        .expect("a model present in either copy must still be found");
}

#[test]
fn selection_cells_keep_their_original_storage_type() {
    let text = SqlValue::Text("{\"a\":1}".to_owned());
    let blob = SqlValue::Blob(br#"{"a":1}"#.to_vec());
    let next = br#"{"a":2}"#.to_vec();

    assert!(
        matches!(bytes_to_cell(&text, next.clone()), SqlValue::Text(_)),
        "a TEXT cell must stay TEXT or Trae's own reader breaks"
    );
    assert!(
        matches!(bytes_to_cell(&blob, next), SqlValue::Blob(_)),
        "a BLOB cell must stay BLOB"
    );
}

#[test]
fn a_profile_without_any_selection_is_reported_clearly() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            params![
                "3129680676791939_AI.agent.model.model_list_map",
                serde_json::to_string(&nmauto_catalogue()).expect("encode")
            ],
        )
        .expect("catalogue");
    connection.close().expect("close");

    let detection = detection_at(database);

    // TRAE SOLO CN records nothing until the user picks a model inside the app,
    // so the failure must explain that instead of leaking a raw sqlite error.
    let error = apply_selection(&detection, "NMauto").expect_err("no selection recorded yet");

    assert_eq!(error.code, "trae_selection_missing");
}

fn nmauto_catalogue() -> Value {
    json!({
        "solo_agent": [{
            "name": "custom_openai_compatible//NMauto",
            "display_name": "NMauto",
            "provider": "custom_openai_compatible",
            "base_url": "https://api.g2claw.com/v1/chat/completions",
            "custom_model_id": 2751100546_i64,
            "config_source": 3,
        }]
    })
}

#[test]
fn selection_id_matches_the_shape_trae_writes() {
    let entry = find_model_entry(&nmauto_catalogue(), "NMauto").expect("entry");

    assert_eq!(
        selection_id("solo_agent", &entry),
        "solo_agent_3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546"
    );
}

/// Trae 里存的是 `NMauto`，但用户侧常会写成 `NMAUTO` / `nmauto`，
/// 大小写不同不应被判定为"模型不存在"。
#[test]
fn model_names_match_case_insensitively() {
    let catalogue = nmauto_catalogue();

    for query in ["NMauto", "NMAUTO", "nmauto", "NmAuto"] {
        let entry = find_model_entry(&catalogue, query)
            .unwrap_or_else(|| panic!("{query} should resolve to NMauto"));
        // 解析出的身份字段必须始终来自 catalogue 的原始写法。
        assert_eq!(entry.display_name, "NMauto");
        assert_eq!(entry.custom_model_id, "2751100546");
    }

    // 大小写不敏感不等于模糊匹配，无关名称仍须被拒绝。
    assert!(find_model_entry(&catalogue, "NMauto-2").is_none());
}

#[test]
fn unknown_models_are_rejected_before_any_write() {
    assert!(find_model_entry(&nmauto_catalogue(), "not-configured").is_none());
}

#[test]
fn rewrite_targets_custom_labels_and_leaves_builtins_alone() {
    let entry = find_model_entry(&nmauto_catalogue(), "NMauto").expect("entry");

    // recent-user-selection shape: {label: selection}
    let recent = json!({
        "solo_agent": {
            "modelId": "solo_agent_3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226",
            "mode": 0
        },
        "solo_coder": { "modelId": "solo_coder_1__Doubao_1_6", "mode": 0 }
    });
    let raw = serde_json::to_vec(&recent).expect("encode");
    let updated = rewrite_selection_value(&raw, &entry).expect("rewritten");
    let value: Value = serde_json::from_slice(&updated).expect("decode");

    assert_eq!(
        value["solo_agent"]["modelId"],
        json!("solo_agent_3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546")
    );
    assert_eq!(
        value["solo_coder"]["modelId"],
        json!("solo_coder_1__Doubao_1_6"),
        "built-in labels must stay untouched"
    );

    // session shape: {sessionHash: {label: selection}}
    let session = json!({
        "6ab14bdc6e2656c4995b4c0f": {
            "solo_agent": {
                "modelId": "solo_agent_3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226",
                "mode": 0
            },
            "agent": { "modelId": "agent_1__Doubao-Seed-Code_null", "mode": 1 }
        }
    });
    let raw = serde_json::to_vec(&session).expect("encode");
    let updated = rewrite_selection_value(&raw, &entry).expect("rewritten");
    let value: Value = serde_json::from_slice(&updated).expect("decode");

    assert_eq!(
        value["6ab14bdc6e2656c4995b4c0f"]["solo_agent"]["modelId"],
        json!("solo_agent_3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546")
    );
    assert_eq!(
        value["6ab14bdc6e2656c4995b4c0f"]["agent"]["modelId"],
        json!("agent_1__Doubao-Seed-Code_null")
    );
}

#[test]
fn rewriting_an_already_selected_model_is_a_no_op() {
    let entry = find_model_entry(&nmauto_catalogue(), "NMauto").expect("entry");
    let recent = json!({
        "solo_agent": {
            "modelId": "solo_agent_3_custom_openai_compatible_custom_openai_compatible//NMauto_2751100546",
            "mode": 0
        }
    });
    let raw = serde_json::to_vec(&recent).expect("encode");

    assert!(
        rewrite_selection_value(&raw, &entry).is_none(),
        "no change means the transaction stays untouched"
    );
}

/// TRAE SOLO CN 0.1.69 把选中态放回了明文库，键名带 `3129680676791939:` 冒号前缀，
/// 且 `session_selected_model` 按会话 id 再套一层。键名与两层形状都取自真机，
/// 确保它和 Trae CN 共用同一条写入路径时不会漏改。
#[test]
fn switches_the_keys_solo_writes_to_the_plain_database() {
    let temp = tempdir().expect("temp");
    let database = temp.path().join("state.vscdb");
    fs::create_dir_all(database.parent().expect("parent")).expect("dir");
    let connection = Connection::open(&database).expect("open");
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);")
        .expect("schema");
    let catalogue = serde_json::to_string(&json!({
        "solo_work_lite": [{
            "name": "custom_openai_compatible//glm-5.2",
            "display_name": "glm-5.2",
            "provider": "custom_openai_compatible",
            "base_url": "https://api.example.com/v1/chat/completions",
            "custom_model_id": 2776505986_i64,
            "config_source": 3,
        }]
    }))
    .expect("encode");
    let previous =
        "solo_work_lite_3_custom_openai_compatible_custom_openai_compatible//glm-5.1_2765316226";
    for (key, value) in [
        (
            "3129680676791939:AI.agent.model.model_list_map",
            catalogue,
        ),
        (
            "3129680676791939:AI.agent.model.recent_user_selection_by_agent_label",
            format!("{{\"solo_work_lite\":{{\"modelId\":\"{previous}\",\"mode\":0}}}}"),
        ),
        (
            "3129680676791939:AI.agent.model.session_selected_model",
            format!(
                "{{\"6ab22d8e971728d01f95acbf\":{{\"solo_work_lite\":{{\"modelId\":\"{previous}\",\"mode\":0}}}}}}"
            ),
        ),
    ] {
        connection
            .execute("INSERT INTO ItemTable VALUES (?1, ?2)", params![key, value])
            .expect("seed");
    }
    connection.close().expect("close");

    let detection = detection_at(database.clone());
    let changes = apply_selection(&detection, "glm-5.2").expect("apply");
    assert!(
        !changes.is_empty(),
        "SOLO's plaintext selection keys must be writable"
    );

    let connection = Connection::open(&database).expect("open");
    for key in [
        "3129680676791939:AI.agent.model.recent_user_selection_by_agent_label",
        "3129680676791939:AI.agent.model.session_selected_model",
    ] {
        let stored: String = connection
            .query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .expect("stored value");
        assert!(
            stored.contains("//glm-5.2_2776505986"),
            "{key} should point at the target model, got: {stored}"
        );
    }
    connection.close().expect("close");
}
