use std::{fs, path::PathBuf};

use serde_json::Value;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol};
use crate::services::BaselineSnapshot;

fn detection(config_path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: OpenCodeAdapter.id(),
        display_name: OpenCodeAdapter.display_name(),
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

fn desired(
    mode: AgentBindingMode,
    upstream_protocol: ApiProtocol,
    credential: &'static str,
) -> DesiredAgentBinding<'static> {
    DesiredAgentBinding {
        mode,
        provider_name: "蒙云智算",
        model_id: "glm-5.2",
        supports_tools: true,
        upstream_protocol,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "http://127.0.0.1:18123/v1/",
        credential,
    }
}

fn seed(path: &PathBuf, body: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("config directory");
    fs::write(path, body).expect("baseline");
}

#[test]
fn adapter_metadata_is_stable() {
    assert_eq!(OpenCodeAdapter.id(), "opencode");
    assert_eq!(OpenCodeAdapter.display_name(), "OpenCode");
    assert_eq!(
        OpenCodeAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::AnthropicMessages,),
        ApiProtocol::OpenaiChatCompletions
    );
}

#[test]
fn accepts_jsonc_comments_and_trailing_commas() {
    let parsed = parse_config(
        br#"{
  // comment
  "model": "copilot/model",
  "provider": {
    "copilot": {},
  },
}"#,
    )
    .expect("jsonc");

    assert_eq!(parsed["model"], "copilot/model");
    assert!(parsed["provider"]["copilot"].is_object());
}

#[test]
fn writes_managed_provider_without_losing_user_configuration() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("opencode.jsonc");
    let baseline = r#"{
  "small_model": "user/small",
  "provider": {
    "copilot": { "name": "Copilot" }
  }
}
"#;
    seed(&path, baseline);
    let binding = desired(
        AgentBindingMode::Proxy,
        ApiProtocol::OpenaiChatCompletions,
        "local-token",
    );

    let output = OpenCodeAdapter
        .build_config(&detection(path.clone()), &binding)
        .expect("config");
    let value: Value = serde_json::from_slice(&output).expect("written json");

    assert_eq!(value["model"], "at-switch/glm-5.2");
    assert_eq!(value["small_model"], "user/small");
    assert_eq!(value["provider"]["copilot"]["name"], "Copilot");
    assert_eq!(value["provider"]["at-switch"]["name"], "AT-Switch");
    assert_eq!(
        value["provider"]["at-switch"]["options"]["baseURL"],
        "http://127.0.0.1:18123/v1"
    );
    assert_eq!(
        value["provider"]["at-switch"]["options"]["apiKey"],
        "local-token"
    );
    assert!(value["provider"]["at-switch"]["models"]["glm-5.2"].is_object());
}

#[test]
fn rejects_direct_protocols_outside_openai_chat() {
    let error = OpenCodeAdapter
        .validate_binding(&desired(
            AgentBindingMode::Direct,
            ApiProtocol::OpenaiResponses,
            "upstream-key",
        ))
        .expect_err("unsupported protocol");

    assert_eq!(error.code, "opencode_direct_protocol_unsupported");
    assert!(error.recovery.is_some());
}

#[test]
fn verifies_and_natively_restores_the_managed_model() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("opencode.jsonc");
    let baseline = r#"{
  "small_model": "user/small",
  "provider": {
    "copilot": { "name": "Copilot" }
  }
}
"#;
    seed(&path, baseline);
    let binding = desired(
        AgentBindingMode::Proxy,
        ApiProtocol::OpenaiChatCompletions,
        "local-token",
    );
    let target = detection(path.clone());

    let generated = OpenCodeAdapter
        .build_config(&target, &binding)
        .expect("generated");
    fs::write(&path, generated).expect("apply generated");
    OpenCodeAdapter
        .verify_config(&target, &binding)
        .expect("verify");

    let restored = OpenCodeAdapter
        .build_native_config(
            &target,
            &BaselineSnapshot {
                existed: true,
                content: baseline.as_bytes().to_vec(),
            },
        )
        .expect("native");
    let value: Value = serde_json::from_slice(&restored).expect("restored json");

    assert_eq!(value["small_model"], "user/small");
    assert_eq!(value["provider"]["copilot"]["name"], "Copilot");
    assert!(value["provider"]["at-switch"].is_null());
    assert!(value["model"].is_null());
}

#[test]
fn rejects_opencode_config_that_is_not_a_json_object() {
    let parsed = parse_config(br#"[]"#);

    assert!(parsed.is_err());
}

#[test]
fn preserves_jsonc_comments_and_trailing_commas_across_native_restore() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("opencode.jsonc");
    let baseline = r#"{
  // top-level comment
  "small_model": "user/small", // model comment
  "provider": {
    "copilot": {
      // provider comment
      "name": "Copilot",
    },
  },
}
"#;
    seed(&path, baseline);
    let binding = desired(
        AgentBindingMode::Proxy,
        ApiProtocol::OpenaiChatCompletions,
        "local-token",
    );
    let target = detection(path.clone());

    let generated = OpenCodeAdapter
        .build_config(&target, &binding)
        .expect("generated");
    let generated_text = String::from_utf8(generated).expect("utf8");
    assert!(generated_text.contains("// top-level comment"));
    assert!(generated_text.contains("// provider comment"));
    assert!(generated_text.contains("\"small_model\": \"user/small\", // model comment"));
    let generated_value = parse_config(generated_text.as_bytes()).expect("jsonc");
    assert_eq!(generated_value["model"], "at-switch/glm-5.2");

    let restored = OpenCodeAdapter
        .build_native_config(
            &target,
            &BaselineSnapshot {
                existed: true,
                content: baseline.as_bytes().to_vec(),
            },
        )
        .expect("restored");
    let restored_text = String::from_utf8(restored).expect("utf8");
    assert!(restored_text.contains("// top-level comment"));
    assert!(restored_text.contains("// provider comment"));
    assert!(!restored_text.contains("at-switch"));
    let restored_value = parse_config(restored_text.as_bytes()).expect("jsonc");
    assert_eq!(restored_value["small_model"], "user/small");
    assert_eq!(restored_value["provider"]["copilot"]["name"], "Copilot");
    assert!(restored_value["provider"]["at-switch"].is_null());
    assert!(restored_value["model"].is_null());
}
