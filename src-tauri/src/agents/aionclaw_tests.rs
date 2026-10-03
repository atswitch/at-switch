use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, ApiProtocol};

fn detection_at(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: "aionclaw",
        display_name: "AionClaw",
        installation: None,
        config_path: Some(config_path),
        runtime_data_dir: None,
        install_status: crate::domain::AgentInstallStatus::Installed,
        config_health: AgentConfigHealth::Healthy,
        write_supported: true,
        needs_restart: true,
        message: None,
        custom_install_path: None,
        using_custom_install_path: false,
    }
}

fn desired(model_id: &'static str) -> DesiredAgentBinding<'static> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id,
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.example.test/v1",
        credential: "test-key",
    }
}

/// 仿照真机：一个用户自建 Provider（custom_0）加一个内置 Provider。
fn seeded_config() -> Value {
    json!({
        "providers": {
            "custom_0": {
                "enabled": true,
                "apiKey": "user-key",
                "baseUrl": "https://api.g2claw.com/v1",
                "apiFormat": "openai",
                "displayName": "g2",
                "models": [{ "id": "NMauto", "name": "NMauto", "supportsImage": false }]
            },
            "deepseek": { "enabled": false }
        },
        "model": {
            "defaultModel": "NMauto",
            "defaultModelProvider": "custom_0",
            "availableModels": [
                { "id": "deepseek-v4-pro", "name": "DeepSeek-V4 Pro", "supportsImage": false, "isServerModel": true }
            ]
        },
        "theme": "dark",
        "language": "zh-CN"
    })
}

/// 建一个最小可用的 AionClaw 数据目录，返回 openclaw.json 路径（事务入口）。
fn seed(dir: &std::path::Path) -> PathBuf {
    let root = dir.join("AionClaw");
    let state = root.join("openclaw/state");
    std::fs::create_dir_all(&state).expect("state dir");
    let config_path = state.join("openclaw.json");
    std::fs::write(&config_path, b"{}").expect("config");

    let connection = Connection::open(root.join("aionclaw.sqlite")).expect("db");
    connection
        .execute_batch(
            "CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE agents (
                 id TEXT PRIMARY KEY,
                 name TEXT,
                 model TEXT,
                 is_default INTEGER NOT NULL DEFAULT 0
             );",
        )
        .expect("schema");
    connection
        .execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)",
            params![
                CONFIG_KEY,
                serde_json::to_string(&seeded_config()).expect("seed")
            ],
        )
        .expect("config row");
    // 界面显示的当前模型来自 Agent 自身。
    connection
        .execute(
            "INSERT INTO agents (id, name, model, is_default) VALUES ('main', 'OpenClaw', 'custom_0/NMauto', 1)",
            [],
        )
        .expect("agent row");
    connection.close().expect("close");

    config_path
}

fn stored_agent_model(config_path: &Path) -> Option<String> {
    let db = database_path(&detection_at(config_path.to_path_buf())).expect("db path");
    let connection = Connection::open(db).expect("open");
    connection
        .query_row(
            "SELECT model FROM agents WHERE is_default = 1 OR id = 'main' LIMIT 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("agent model")
}

fn stored(config_path: &Path) -> Value {
    let db = database_path(&detection_at(config_path.to_path_buf())).expect("db path");
    let connection = Connection::open(db).expect("open");
    let raw: String = connection
        .query_row("SELECT value FROM kv WHERE key = ?1", [CONFIG_KEY], |row| {
            row.get(0)
        })
        .expect("config row");
    serde_json::from_str(&raw).expect("parse")
}

#[test]
fn writes_the_managed_provider_into_the_database() {
    let temp = tempdir().expect("temp");
    let config_path = seed(temp.path());
    let detection = detection_at(config_path.clone());
    let binding = desired("glm-5.2");

    apply_config(&detection, &binding).expect("apply");
    verify_config(&detection, &binding).expect("verify");

    let written = stored(&config_path);
    assert_eq!(
        written
            .pointer("/model/defaultModel")
            .and_then(Value::as_str),
        Some("glm-5.2")
    );
    assert_eq!(
        written
            .pointer("/model/defaultModelProvider")
            .and_then(Value::as_str),
        Some(MANAGED_PROVIDER_ID)
    );
    assert_eq!(
        written
            .pointer("/providers/at-switch/baseUrl")
            .and_then(Value::as_str),
        Some("https://api.example.test/v1")
    );
    assert_eq!(
        written
            .pointer("/providers/at-switch/apiFormat")
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        written
            .pointer("/providers/at-switch/models/0/id")
            .and_then(Value::as_str),
        Some("glm-5.2")
    );
    // 只写目录不够：Agent 自身也必须指向受管 Provider，否则界面停在"未选中"。
    assert_eq!(
        stored_agent_model(&config_path).as_deref(),
        Some("at-switch/glm-5.2")
    );
}

#[test]
fn preserves_user_providers_and_unrelated_settings() {
    let temp = tempdir().expect("temp");
    let config_path = seed(temp.path());

    apply_config(&detection_at(config_path.clone()), &desired("glm-5.2")).expect("apply");

    let written = stored(&config_path);
    // 用户自建的 custom_0 必须原样保留，不能被替换或重命名。
    assert_eq!(
        written
            .pointer("/providers/custom_0/apiKey")
            .and_then(Value::as_str),
        Some("user-key")
    );
    assert_eq!(
        written
            .pointer("/providers/custom_0/displayName")
            .and_then(Value::as_str),
        Some("g2")
    );
    assert_eq!(
        written
            .pointer("/providers/deepseek/enabled")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        written.pointer("/theme").and_then(Value::as_str),
        Some("dark")
    );
    assert!(written
        .pointer("/model/availableModels")
        .and_then(Value::as_array)
        .is_some_and(|models| models.len() == 1));
}

#[test]
fn switching_again_replaces_instead_of_accumulating() {
    let temp = tempdir().expect("temp");
    let config_path = seed(temp.path());
    let detection = detection_at(config_path.clone());

    apply_config(&detection, &desired("glm-5.2")).expect("first");
    apply_config(&detection, &desired("glm-5.3")).expect("second");

    let written = stored(&config_path);
    let providers = written
        .pointer("/providers")
        .and_then(Value::as_object)
        .expect("providers");

    // custom_0 + deepseek + 单个受管条目，不随切换次数增长。
    assert_eq!(providers.len(), 3);
    assert_eq!(
        written
            .pointer("/model/defaultModel")
            .and_then(Value::as_str),
        Some("glm-5.3")
    );
}

#[test]
fn verification_fails_when_the_database_points_elsewhere() {
    let temp = tempdir().expect("temp");
    let config_path = seed(temp.path());
    let detection = detection_at(config_path.clone());

    apply_config(&detection, &desired("glm-5.2")).expect("apply");

    let error = verify_config(&detection, &desired("glm-5.3"))
        .expect_err("verification must fail for another model");

    assert_eq!(error.code, "aionclaw_write_verification_failed");
}

#[test]
fn restore_puts_the_original_document_back() {
    let temp = tempdir().expect("temp");
    let config_path = seed(temp.path());
    let detection = detection_at(config_path.clone());

    let change = apply_config(&detection, &desired("glm-5.2")).expect("apply");
    restore_config(&detection, &change).expect("restore");

    let written = stored(&config_path);
    assert!(written.pointer("/providers/at-switch").is_none());
    assert_eq!(
        stored_agent_model(&config_path).as_deref(),
        Some("custom_0/NMauto")
    );
    assert_eq!(
        written
            .pointer("/model/defaultModelProvider")
            .and_then(Value::as_str),
        Some("custom_0")
    );
    assert_eq!(
        written
            .pointer("/model/defaultModel")
            .and_then(Value::as_str),
        Some("NMauto")
    );
}

#[test]
fn maps_protocols_to_aionclaw_api_formats() {
    assert_eq!(
        aionclaw_api_format(ApiProtocol::OpenaiChatCompletions),
        "openai"
    );
    assert_eq!(
        aionclaw_api_format(ApiProtocol::OpenaiResponses),
        "openai-responses"
    );
    assert_eq!(
        aionclaw_api_format(ApiProtocol::AnthropicMessages),
        "anthropic"
    );
}
