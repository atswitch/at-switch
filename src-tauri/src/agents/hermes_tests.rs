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

fn yaml_block(path: &Path, key: &str) -> serde_yaml::Mapping {
    let raw = fs::read_to_string(path).expect("config");
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).expect("yaml");
    value
        .get(key)
        .and_then(serde_yaml::Value::as_mapping)
        .cloned()
        .unwrap_or_else(|| panic!("{key} block"))
}

fn model_block(path: &Path) -> serde_yaml::Mapping {
    yaml_block(path, "model")
}

fn managed_provider_entry(path: &Path) -> serde_yaml::Mapping {
    let providers = yaml_block(path, "providers");
    providers
        .get(serde_yaml::Value::from(MANAGED_PROVIDER))
        .and_then(serde_yaml::Value::as_mapping)
        .cloned()
        .expect("managed provider entry")
}

fn str_field(block: &serde_yaml::Mapping, key: &str) -> Option<String> {
    block
        .get(serde_yaml::Value::from(key))
        .and_then(serde_yaml::Value::as_str)
        .map(ToOwned::to_owned)
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
fn writes_named_provider_entry_and_removes_legacy_entries() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");

    fs::write(&path, &updated).expect("apply");
    let model = model_block(&path);
    assert_eq!(str_field(&model, "provider").as_deref(), Some(MANAGED_PROVIDER));
    assert_eq!(str_field(&model, "default").as_deref(), Some("GLM-5.2"));
    // 端点与凭据收敛到 provider 条目，模型块不再重复承载。
    assert!(str_field(&model, "base_url").is_none());
    assert!(str_field(&model, "key_env").is_none());
    assert!(str_field(&model, "api_mode").is_none());
    assert!(model.get(serde_yaml::Value::from("api_key")).is_none());

    let entry = managed_provider_entry(&path);
    assert_eq!(
        str_field(&entry, "base_url").as_deref(),
        Some("https://api.g2claw.com/v1")
    );
    assert_eq!(str_field(&entry, "default_model").as_deref(), Some("GLM-5.2"));
    assert_eq!(
        str_field(&entry, "key_env").as_deref(),
        Some(HERMES_API_KEY_ENV)
    );
    assert_eq!(
        str_field(&entry, "api_mode").as_deref(),
        Some("chat_completions")
    );
    assert_eq!(str_field(&entry, "name").as_deref(), Some(DISPLAY_NAME));
    // 明文凭据必须移除，凭据只经 key_env 指针引用。
    assert!(entry.get(serde_yaml::Value::from("api_key")).is_none());

    // 未知块与用户自有 custom_providers 条目保留。
    let raw = fs::read_to_string(&path).expect("config");
    assert!(raw.contains("journal_mode: wal"));
    assert!(raw.contains("Api.g2claw.com"));
    assert!(!raw.contains("- name: at-switch"));
    // 旧扁平 provider 键被清理。
    assert!(!raw.contains("custom:at-switch"));
}

#[test]
fn switching_models_updates_both_model_block_and_provider_entry() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);

    let first = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build first");
    fs::write(&path, &first).expect("apply first");

    let second = DesiredAgentBinding {
        model_id: "NMauto",
        ..desired()
    };
    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &second)
        .expect("build second");
    fs::write(&path, &updated).expect("apply second");

    assert_eq!(
        str_field(&model_block(&path), "default").as_deref(),
        Some("NMauto")
    );
    assert_eq!(
        str_field(&managed_provider_entry(&path), "default_model").as_deref(),
        Some("NMauto")
    );
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
fn ensure_env_api_key_backfills_missing_env_file_and_verification_passes() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);
    // 不播种 .env：真实切换流程中由 ensure_env_api_key 自动补建受管行。

    let updated = HermesAdapter
        .build_config(&detection(path.clone()), &desired())
        .expect("build");
    fs::write(&path, &updated).expect("apply");
    ensure_env_api_key(&detection(path.clone()), desired().credential).expect("ensure env");

    let env_path = path.parent().expect("config parent").join(".env");
    let content = fs::read_to_string(&env_path).expect("env");
    assert!(content.contains("HERMES_AT_SWITCH_API_KEY=local-token"));
    HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect("verified");
}

#[test]
fn ensure_env_api_key_rotates_value_and_keeps_user_lines() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);
    let env_path = path.parent().expect("config parent").join(".env");
    fs::write(
        &env_path,
        "HERMES_USER_KEY=keep\n  HERMES_AT_SWITCH_API_KEY=stale-token\nTAIL=1\n",
    )
    .expect("seed env");

    ensure_env_api_key(&detection(path), "rotated-token").expect("ensure env");

    let content = fs::read_to_string(&env_path).expect("env");
    assert!(content.contains("HERMES_USER_KEY=keep"));
    assert!(content.contains("HERMES_AT_SWITCH_API_KEY=rotated-token"));
    assert!(content.contains("TAIL=1"));
    assert!(!content.contains("stale-token"));
    // 受管键只保留一行，不产生重复条目。
    assert_eq!(
        content
            .lines()
            .filter(|line| line.contains("HERMES_AT_SWITCH_API_KEY"))
            .count(),
        1
    );
}

#[test]
fn ensure_env_api_key_rejects_empty_credential() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed(&path, LEGACY_CONFIG);

    let error = ensure_env_api_key(&detection(path.clone()), "  ").expect_err("empty credential");
    assert_eq!(error.code, "hermes_api_key_empty");
    assert!(!path.parent().expect("config parent").join(".env").exists());
}

#[test]
fn verify_rejects_inline_api_key() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed_env(&path);
    seed(
        &path,
        "model:\n  provider: at-switch\n  default: GLM-5.2\nproviders:\n  at-switch:\n    name: AT-Switch\n    base_url: https://api.g2claw.com/v1\n    default_model: GLM-5.2\n    key_env: HERMES_AT_SWITCH_API_KEY\n    api_key: plaintext\n    api_mode: chat_completions\n",
    );

    let error = HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect_err("inline api key");
    assert_eq!(error.code, "agent_config_not_applied");
}

#[test]
fn verify_rejects_missing_provider_entry() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("config.yaml");
    seed_env(&path);
    seed(&path, "model:\n  provider: at-switch\n  default: GLM-5.2\n");

    let error = HermesAdapter
        .verify_config(&detection(path), &desired())
        .expect_err("missing entry");
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
fn probe_accepts_v45_flat_and_named_shapes() {
    let temp = tempdir().expect("temp");
    let v45 = temp.path().join("v45.yaml");
    seed(&v45, "model:\n  provider: custom\n  default: m\n");
    probe_hermes(&v45).expect("v45 probe");

    let flat = temp.path().join("flat.yaml");
    seed(&flat, "model: m\nprovider: p\n");
    probe_hermes(&flat).expect("flat probe");

    let named = temp.path().join("named.yaml");
    seed(&named, "model:\n  provider: at-switch\n  default: m\nproviders:\n  at-switch:\n    name: AT-Switch\n    base_url: https://api.g2claw.com/v1\n");
    probe_hermes(&named).expect("named probe");

    let broken = temp.path().join("broken.yaml");
    seed(&broken, "model:\n  - list\n  - not a mapping\n");
    assert!(probe_hermes(&broken).is_err());

    let providers_sequence = temp.path().join("providers-sequence.yaml");
    seed(&providers_sequence, "providers:\n  - not a mapping\n");
    assert!(probe_hermes(&providers_sequence).is_err());
}
