use std::{fs, path::PathBuf};

use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol};

fn detection(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: HermesAdapter.id(),
        display_name: HermesAdapter.display_name(),
        installation: None,
        config_path: Some(config_path),
        runtime_data_dir: None,
        install_status: AgentInstallStatus::Installed,
        config_health: AgentConfigHealth::Healthy,
        write_supported: true,
        needs_restart: false,
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
        base_url: "https://api.g2claw.com/v1/",
        credential: "local-token",
    }
}

fn seed(path: &PathBuf, body: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("config directory");
    fs::write(path, body).expect("seed config");
}

fn seed_env(path: &Path) {
    let env_path = path.parent().expect("config parent").join(".env");
    fs::write(env_path, "HERMES_AT_SWITCH_API_KEY=test-key\n").expect("seed env");
}

fn model_block(path: &Path) -> serde_yaml::Mapping {
    let raw = fs::read_to_string(path).expect("config");
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).expect("yaml");
    value
        .get("model")
        .and_then(serde_yaml::Value::as_mapping)
        .cloned()
        .expect("model block")
}

const LEGACY_CONFIG: &str = r#"model: "deepseek-v4-flash"
provider: "custom:at-switch:deepseek-v4-flash"
custom_providers:
  - name: at-switch
    base_url: https://api.g2claw.com/v1
    key_env: HERMES_AT_SWITCH_API_KEY
    model: deepseek-v4-flash
    api_mode: chat_completions
  - name: Api.g2claw.com
    base_url: https://api.g2claw.com/v1
    key_env: HERMES_CUSTOM_API_G2CLAW_COM_API_KEY
    model: NMauto
    api_mode: chat_completions
database:
  journal_mode: wal
"#;

#[test]
fn writes_the_v45_model_block_and_removes_legacy_entries() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");

    fs::write(&path, &updated).expect("apply");
    let model = model_block(&path);
    assert_eq!(
        model
            .get(serde_yaml::Value::from("provider"))
            .and_then(serde_yaml::Value::as_str),
        Some("custom")
    );
    assert_eq!(
        model
            .get(serde_yaml::Value::from("default"))
            .and_then(serde_yaml::Value::as_str),
        Some("GLM-5.2")
    );
    assert_eq!(
        model
            .get(serde_yaml::Value::from("base_url"))
            .and_then(serde_yaml::Value::as_str),
        Some("https://api.g2claw.com/v1")
    );
    assert_eq!(
        model
            .get(serde_yaml::Value::from("key_env"))
            .and_then(serde_yaml::Value::as_str),
        Some("HERMES_AT_SWITCH_API_KEY")
    );
    assert_eq!(
        model
            .get(serde_yaml::Value::from("api_mode"))
            .and_then(serde_yaml::Value::as_str),
        Some("chat_completions")
    );
    // 明文凭据必须移除，凭据只经 key_env 指针引用。
    assert!(model.get(serde_yaml::Value::from("api_key")).is_none());
    // 未知块与用户自有 custom_providers 条目保留。
    let raw = fs::read_to_string(&path).expect("config");
    assert!(raw.contains("journal_mode: wal"));
    assert!(raw.contains("Api.g2claw.com"));
    assert!(!raw.contains("- name: at-switch"));
    // 旧扁平 provider 键被清理。
    assert!(!raw.contains("custom:at-switch"));
}

#[test]
fn verify_accepts_the_written_config_with_env_key() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);
    seed_env(&path);

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");
    fs::write(&path, &updated).expect("apply");

    HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect("verified");
}

#[test]
fn verify_requires_the_env_file_key() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);
    // 不播种 .env：缺少凭据指针指向的键必须显式报错，而不是静默成功。

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");
    fs::write(&path, &updated).expect("apply");

    let error = HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect_err("missing env key");
    assert_eq!(error.code, "hermes_api_key_env_missing");
}

#[test]
fn verify_rejects_inline_api_key() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed_env(&path);
    seed(
        &path,
        "model:\n  provider: custom\n  default: GLM-5.2\n  base_url: https://api.g2claw.com/v1\n  api_key: plaintext\n  api_mode: chat_completions\n",
    );

    let error = HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect_err("inline api key");
    assert_eq!(error.code, "agent_config_not_applied");
}

#[test]
fn retains_user_model_fields_across_switches() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed_env(&path);
    seed(
        &path,
        "model:\n  provider: custom\n  default: old-model\n  base_url: https://api.g2claw.com/v1\n  api_mode: chat_completions\n  reasoning_effort: medium\n",
    );

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");
    fs::write(&path, &updated).expect("apply");

    let model = model_block(&path);
    // 用户在模型块里的自定义字段跨切换保留。
    assert_eq!(
        model
            .get(serde_yaml::Value::from("reasoning_effort"))
            .and_then(serde_yaml::Value::as_str),
        Some("medium")
    );
    assert_eq!(
        model
            .get(serde_yaml::Value::from("default"))
            .and_then(serde_yaml::Value::as_str),
        Some("GLM-5.2")
    );
}

#[test]
fn restore_returns_the_baseline_content() {
    let temp = tempdir().expect("temp");
    let baseline = BaselineSnapshot {
        existed: true,
        content: b"model:\n  provider: custom\n  default: official-model\n".to_vec(),
    };
    let restored = HermesAdapter
        .build_native_config(&detection(temp.path().join("config.yaml")), &baseline)
        .expect("restore");
    assert_eq!(restored, baseline.content);
}

#[test]
fn probe_accepts_v45_and_flat_shapes() {
    let temp = tempdir().expect("temp");
    let v45 = temp.path().join("v45.yaml");
    seed(&v45, "model:\n  provider: custom\n  default: m\n");
    probe_hermes(&v45).expect("v45 probe");

    let flat = temp.path().join("flat.yaml");
    seed(&flat, "model: m\nprovider: p\n");
    probe_hermes(&flat).expect("flat probe");

    let broken = temp.path().join("broken.yaml");
    seed(&broken, "model:\n  - list\n  - not a mapping\n");
    assert!(probe_hermes(&broken).is_err());
}
