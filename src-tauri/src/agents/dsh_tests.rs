use std::{fs, path::PathBuf};

use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus};

fn detection(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: DshAdapter.id(),
        display_name: DshAdapter.display_name(),
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

/// Direct binding against an OpenAI-Chat provider — the happy path for dsh.
fn desired_direct<'a>() -> DesiredAgentBinding<'a> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "DeepSeek",
        model_id: "deepseek-chat",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.deepseek.com/v1/",
        credential: "sk-test",
    }
}

/// Proxy binding — dsh still speaks OpenAI Chat on its side, but the upstream
/// provider is Responses, so the local proxy converts.
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

fn write_patch(path: &PathBuf, body: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("config dir");
    fs::write(path, body).expect("patch write");
}

#[test]
fn dsh_adapter_metadata_is_stable() {
    assert_eq!(DshAdapter.id(), "dsh");
    assert_eq!(DshAdapter.display_name(), "DeepSeek Harness");
    // dsh only speaks OpenAI Chat on its wire, regardless of mode or upstream.
    assert_eq!(
        DshAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::OpenaiResponses),
        ApiProtocol::OpenaiChatCompletions
    );
    assert_eq!(
        DshAdapter.source_protocol(AgentBindingMode::Direct, ApiProtocol::AnthropicMessages),
        ApiProtocol::OpenaiChatCompletions
    );
}

#[test]
fn dsh_validate_binding_rejects_non_chat_direct() {
    let desired = DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "Anthropic",
        model_id: "claude-sonnet-5",
        supports_tools: true,
        upstream_protocol: ApiProtocol::AnthropicMessages,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.anthropic.com",
        credential: "sk-test",
    };
    let error = DshAdapter.validate_binding(&desired).expect_err("rejected");
    assert_eq!(error.code, "dsh_direct_protocol_unsupported");
    assert!(error.recovery.is_some());
}

#[test]
fn dsh_validate_binding_accepts_chat_direct() {
    DshAdapter
        .validate_binding(&desired_direct())
        .expect("accepted");
}

#[test]
fn dsh_probe_accepts_empty_patch() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(&path, "[]\n");
    probe_patch(&path).expect("probe");
}

#[test]
fn dsh_probe_accepts_populated_patch() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(
        &path,
        "- id: agent-default-model\n  config:\n    provider: deepseek-account\n    model: deepseek-flash\n",
    );
    probe_patch(&path).expect("probe");
}

#[test]
fn dsh_probe_rejects_unparseable_yaml() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(&path, "- id: broken\n  config: [\n  invalid: yaml");
    let error = probe_patch(&path).expect_err("probe error");
    assert_eq!(error.code, "dsh_config_unparseable");
}

#[test]
fn dsh_probe_rejects_non_list_root() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(&path, "agent:\n  model: deepseek-flash\n");
    let error = probe_patch(&path).expect_err("probe error");
    assert_eq!(error.code, "dsh_config_shape_unsupported");
}

#[test]
fn dsh_build_config_creates_entries_from_empty_array() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(&path, "[]\n");
    let detection = detection(path.clone());
    let desired = desired_direct();

    let updated = DshAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let updated = std::str::from_utf8(&updated).expect("utf8");

    assert!(updated.contains("id: agent-default-model"));
    assert!(updated.contains("provider: at-switch"));
    assert!(updated.contains("model: \"deepseek-chat\""));
    assert!(updated.contains("id: llm-pi-ai"));
    assert!(updated.contains("at-switch:"));
    assert!(updated.contains("api: openai-completions"));
    assert!(updated.contains("baseURL: \"https://api.deepseek.com/v1\""));
    assert!(updated.contains("apiKeyEnv: DSH_AT_SWITCH_API_KEY"));
    assert!(!updated.contains("apiKey:"));
    assert!(!updated.contains("[]")); // placeholder dropped

    fs::write(&path, updated).expect("write updated");
    DshAdapter
        .verify_config(&detection, &desired)
        .expect("verify");
}

#[test]
fn dsh_build_config_strips_reasoning_effort_preserves_other_tuning() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(
        &path,
        "# user comment that must survive\n\
         - id: agent-default-model\n\
         \x20 name: \"@deepseek-ai/dsh-agent-default-model\"\n\
         \x20 config:\n\
         \x20   provider: deepseek-account\n\
         \x20   model: deepseek-flash\n\
         \x20   reasoningEffort: high\n\
         \x20   temperature: 0.8\n\
         - id: ui-settings-account\n\
         \x20 name: \"@deepseek-ai/dsh-client-ui-settings-account\"\n\
         \x20 config:\n\
         \x20   version: 1\n\
         \x20   step: done\n",
    );
    let detection = detection(path.clone());
    let desired = desired_proxy();

    let updated = DshAdapter
        .build_config(&detection, &desired)
        .expect("build config");
    let updated_str = std::str::from_utf8(&updated).expect("utf8");

    // User comment preserved.
    assert!(updated_str.contains("# user comment that must survive"));
    // Other user entries preserved untouched.
    assert!(updated_str.contains("id: ui-settings-account"));
    assert!(updated_str.contains("step: done"));
    // reasoningEffort is incompatible with third-party providers — stripped.
    assert!(!updated_str.contains("reasoningEffort"));
    // temperature (unrelated tuning) is still preserved.
    assert!(updated_str.contains("temperature: 0.8"));
    // Managed fields overwritten.
    assert!(updated_str.contains("provider: at-switch"));
    assert!(updated_str.contains("model: \"GLM-5.2\""));
    // Managed provider block present.
    assert!(updated_str.contains("id: llm-pi-ai"));
    assert!(updated_str.contains("baseURL: \"http://127.0.0.1:18123/v1\""));

    fs::write(&path, updated).expect("write updated");
    DshAdapter
        .verify_config(&detection, &desired)
        .expect("verify");
}

#[test]
fn dsh_build_config_replaces_previous_managed_entries_idempotently() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    // Start from a file AT-Switch already manages for an older model/provider.
    let desired_old = desired_direct();
    let first = DshAdapter
        .build_config(&detection(path.clone()), &desired_old)
        .expect("first build");
    write_patch(&path, std::str::from_utf8(&first).expect("utf8"));

    // Switch to a different model via proxy.
    let desired_new = desired_proxy();
    let detection = detection(path.clone());
    let updated = DshAdapter
        .build_config(&detection, &desired_new)
        .expect("second build");
    let updated = std::str::from_utf8(&updated).expect("utf8");

    // Exactly one managed default-model entry and one provider entry.
    assert_eq!(
        updated.matches("id: agent-default-model").count(),
        1,
        "default-model entry count: {updated}"
    );
    assert_eq!(
        updated.matches("id: llm-pi-ai").count(),
        1,
        "provider entry count: {updated}"
    );
    assert_eq!(
        updated.match_indices("at-switch:").count(),
        1,
        "at-switch: count: {updated}"
    );
    // Old model id gone, new one present.
    assert!(
        !updated.contains("deepseek-chat"),
        "old model id leaked: {updated}"
    );
    assert!(updated.contains("model: \"GLM-5.2\""));
    assert!(updated.contains("baseURL: \"http://127.0.0.1:18123/v1\""));
    assert_eq!(updated.matches("id: llm-pi-ai").count(), 1);
    assert_eq!(updated.matches("at-switch:").count(), 1);
    // Old model id gone, new one present.
    assert!(!updated.contains("deepseek-chat"));
    assert!(updated.contains("model: \"GLM-5.2\""));
    assert!(updated.contains("baseURL: \"http://127.0.0.1:18123/v1\""));

    fs::write(&path, updated).expect("write updated");
    DshAdapter
        .verify_config(&detection, &desired_new)
        .expect("verify");
}

#[test]
fn dsh_verify_config_detects_provider_mismatch() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    // Managed entry present but provider name tampered.
    write_patch(
        &path,
        "- id: agent-default-model\n\
         \x20 config:\n\
         \x20   provider: deepseek-account\n\
         \x20   model: deepseek-chat\n\
         - id: llm-pi-ai\n\
         \x20 config:\n\
         \x20   providers:\n\
         \x20     at-switch:\n\
         \x20       api: openai-completions\n\
         \x20       baseURL: \"https://api.deepseek.com/v1\"\n\
         \x20       apiKeyEnv: DSH_AT_SWITCH_API_KEY\n\
         \x20       models:\n\
         \x20         - id: deepseek-chat\n",
    );
    let detection = detection(path.clone());
    let desired = desired_direct();
    let error = DshAdapter
        .verify_config(&detection, &desired)
        .expect_err("verify error");
    assert_eq!(error.code, "dsh_config_not_applied");
}

#[test]
fn dsh_verify_config_rejects_plaintext_key() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(
        &path,
        "- id: agent-default-model\n\
         \x20 config:\n\
         \x20   provider: at-switch\n\
         \x20   model: deepseek-chat\n\
         - id: llm-pi-ai\n\
         \x20 config:\n\
         \x20   providers:\n\
         \x20     at-switch:\n\
         \x20       api: openai-completions\n\
         \x20       baseURL: \"https://api.deepseek.com/v1\"\n\
         \x20       apiKeyEnv: DSH_AT_SWITCH_API_KEY\n\
         \x20       apiKey: sk-leaked-secret\n\
         \x20       models:\n\
         \x20         - id: deepseek-chat\n",
    );
    let detection = detection(path.clone());
    let desired = desired_direct();
    let error = DshAdapter
        .verify_config(&detection, &desired)
        .expect_err("verify error");
    assert_eq!(error.code, "dsh_config_not_applied");
}

#[test]
fn dsh_build_native_config_restores_baseline_when_present() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    let original = "- id: agent-default-model\n  config:\n    provider: deepseek-account\n    model: deepseek-flash\n";
    write_patch(&path, original);
    let detection = detection(path);
    let baseline = BaselineSnapshot {
        existed: true,
        content: original.as_bytes().to_vec(),
    };
    let restored = DshAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    assert_eq!(restored, baseline.content);
}

#[test]
fn dsh_build_native_config_emits_empty_list_when_no_baseline() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(PATCH_FILE);
    write_patch(&path, "[]\n");
    let detection = detection(path);
    let baseline = BaselineSnapshot {
        existed: false,
        content: Vec::new(),
    };
    let restored = DshAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    // No baseline → AT-Switch never wrote, so revert to dsh's first-run empty
    // patch list rather than pinning a vendor provider.
    assert_eq!(std::str::from_utf8(&restored).expect("utf8"), "[]\n");
}

#[test]
fn dsh_launch_env_returns_managed_key_variable() {
    let desired = desired_direct();
    let env = DshAdapter.launch_env(&desired);
    assert_eq!(env.len(), 1);
    let (name, value) = env[0];
    // 凭据必须走 launchd 注入，而不是写进 profile 的 .env —— dsh 把
    // DSH_AT_SWITCH_API_KEY 归为「启动环境变量」，从 .env 读到就拒绝启动。
    assert_eq!(name, DSH_API_KEY_ENV);
    assert_eq!(value, "sk-test");
}

#[test]
fn dsh_launch_env_carries_proxy_token_when_proxying() {
    let desired = desired_proxy();
    let env = DshAdapter.launch_env(&desired);
    assert_eq!(env.len(), 1);
    // 代理模式注入的是本地代理 token，而不是上游 Key。
    assert_eq!(env[0], (DSH_API_KEY_ENV, "local-token"));
}
