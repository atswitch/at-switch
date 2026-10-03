use std::{fs, path::PathBuf};

use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus};

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

fn desired_proxy<'a>() -> DesiredAgentBinding<'a> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Proxy,
        provider_name: "蒙云",
        model_id: "GLM-5.2",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiResponses,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "http://127.0.0.1:18123/v1/",
        credential: "local-token",
    }
}

fn write_baseline(path: &PathBuf, body: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("config dir");
    fs::write(path, body).expect("baseline write");
}

#[test]
fn hermes_adapter_metadata_is_stable() {
    assert_eq!(HermesAdapter.id(), "hermes");
    assert_eq!(HermesAdapter.display_name(), "Hermes Agent");
    assert_eq!(
        HermesAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::OpenaiResponses),
        ApiProtocol::OpenaiChatCompletions
    );
    assert_eq!(
        HermesAdapter.source_protocol(AgentBindingMode::Direct, ApiProtocol::AnthropicMessages),
        ApiProtocol::AnthropicMessages
    );
}

#[test]
fn probe_accepts_minimal_hermes_configuration() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(
        &config_path,
        "model: existing-model\nprovider: openrouter\nfallback_providers: []\n",
    );
    probe_hermes(&config_path).expect("probe");
}

#[test]
fn probe_rejects_unparseable_yaml() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(&config_path, "model: [\nbroken: yaml");
    let error = probe_hermes(&config_path).expect_err("probe error");
    assert_eq!(error.code, "agent_config_unparseable");
}

#[test]
fn build_config_inserts_provider_block_preserves_comments() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(
        &config_path,
        "# 用户自定义的注释，应该被保留\nmodel: previous-model\nprovider: openrouter\n\ntheme: dark\n",
    );

    let detection = detection(config_path.clone());
    let desired = desired_proxy();
    let updated = HermesAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let updated = std::str::from_utf8(&updated).expect("utf8");

    assert!(updated.contains("# 用户自定义的注释，应该被保留"));
    assert!(updated.contains("theme: dark"));
    assert!(updated.contains("model: \"GLM-5.2\""));
    assert!(updated.contains("provider: \"custom:at-switch:GLM-5.2\""));
    assert!(updated.contains("providers:"));
    assert!(updated.contains("  at-switch:"));
    assert!(updated.contains("    api: \"http://127.0.0.1:18123/v1\""));
    assert!(updated.contains("    key_env: HERMES_AT_SWITCH_API_KEY"));
    assert!(updated.contains("    transport: chat_completions"));
    assert!(updated.contains("    default_model: \"GLM-5.2\""));
    assert!(!updated.contains("api_key"));
    assert!(!updated.contains("key_cmd"));
    fs::write(&config_path, updated).expect("write updated");

    HermesAdapter
        .verify_config(&detection, &desired)
        .expect("verify");
}

#[test]
fn build_config_replaces_previous_at_switch_provider_block() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(
        &config_path,
        "model: previous-model\nprovider: \"custom:at-switch:old-model\"\nproviders:\n  at-switch:\n    api: \"https://old.example.com/v1\"\n    api_key: \"old-secret\"\n    transport: anthropic_messages\n    default_model: \"old-model\"\n  openrouter: {}\n",
    );

    let detection = detection(config_path.clone());
    let desired = desired_proxy();
    let updated = HermesAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let updated = std::str::from_utf8(&updated).expect("utf8");
    let occurrences = updated.matches("  at-switch:").count();
    assert_eq!(occurrences, 1);
    fs::write(&config_path, updated).expect("write updated");
    assert!(!updated.contains("old-secret"));
    assert!(!updated.contains("old.example.com"));
    assert!(updated.contains("    transport: chat_completions"));
    assert!(updated.contains("    default_model: \"GLM-5.2\""));

    HermesAdapter
        .verify_config(&detection, &desired)
        .expect("verify");
}

#[test]
fn build_config_appends_providers_when_missing() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(
        &config_path,
        "model: fallback-model\nprovider: openrouter\n",
    );
    let detection = detection(config_path.clone());
    let desired = desired_proxy();
    let updated = HermesAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let updated = std::str::from_utf8(&updated).expect("utf8");
    assert!(updated.contains("providers:"));
    assert!(updated.contains("  at-switch:"));
    assert!(updated.contains("    key_env: HERMES_AT_SWITCH_API_KEY"));
}

#[test]
fn verify_config_detects_unexpected_provider_layout() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(
        &config_path,
        "model: GLM-5.2\nprovider: \"custom:at-switch:GLM-5.2\"\nproviders:\n  at-switch:\n    api: \"http://127.0.0.1:18123/v1\"\n    key_env: HERMES_AT_SWITCH_API_KEY\n    transport: anthropic_messages\n    default_model: \"GLM-5.2\"\n",
    );
    let detection = detection(config_path.clone());
    let desired = desired_proxy();
    let error = HermesAdapter
        .verify_config(&detection, &desired)
        .expect_err("verify error");
    assert_eq!(error.code, "agent_config_not_applied");
}

#[test]
fn build_native_config_restores_baseline_when_present() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(&config_path, "model: user-model\nprovider: openrouter\n");
    let detection = detection(config_path.clone());
    let original = fs::read(&config_path).expect("read baseline");
    let baseline = BaselineSnapshot {
        existed: true,
        content: original,
    };
    let restored = HermesAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    assert_eq!(restored, baseline.content);
}

#[test]
fn build_native_config_leaves_disk_untouched_when_no_baseline() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("config.yaml");
    write_baseline(&config_path, "model: placeholder\nprovider: placeholder\n");
    let detection = detection(config_path);
    let baseline = BaselineSnapshot {
        existed: false,
        content: Vec::new(),
    };
    let restored = HermesAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    // No baseline means AT-Switch never wrote to this Agent. Emit an empty
    // payload so the disk reverts to its first-run / factory state instead of
    // being pinned to a hardcoded provider.
    assert!(restored.is_empty());
}
