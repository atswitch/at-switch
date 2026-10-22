use std::{fs, path::PathBuf};

use serde_json::Value;
use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol};
use crate::services::BaselineSnapshot;

fn file_detection(path: PathBuf) -> AgentDetection {
    AgentDetection {
        id: OpenClawAdapter.id(),
        display_name: OpenClawAdapter.display_name(),
        installation: None,
        config_path: Some(path),
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
        mode: AgentBindingMode::Proxy,
        provider_name: "蒙云智算",
        model_id: "glm-test",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiResponses,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "http://127.0.0.1:54187/v1",
        credential: "local-token",
    }
}

/// 本机实证的全新安装配置（gateway/auth + plugins，无 models 段）。
const FRESH_CONFIG: &str = r#"{
  "gateway" : {
    "auth" : {
      "mode" : "token",
      "token" : "test-token"
    },
    "mode" : "local"
  },
  "meta" : {
    "lastTouchedVersion" : "2026.10.5"
  },
  "plugins" : {
    "entries" : {
      "anthropic" : {
        "config" : { "sessionCatalog" : { "enabled" : false } }
      }
    }
  }
}"#;

#[test]
fn adapter_metadata_is_stable() {
    assert_eq!(OpenClawAdapter.id(), "openclaw");
    assert_eq!(OpenClawAdapter.display_name(), "OpenClaw");
}

#[test]
fn source_protocol_mirrors_direct_and_pins_proxy() {
    assert_eq!(
        OpenClawAdapter.source_protocol(AgentBindingMode::Direct, ApiProtocol::AnthropicMessages),
        ApiProtocol::AnthropicMessages
    );
    assert_eq!(
        OpenClawAdapter.source_protocol(AgentBindingMode::Direct, ApiProtocol::OpenaiResponses),
        ApiProtocol::OpenaiResponses
    );
    assert_eq!(
        OpenClawAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::OpenaiResponses),
        ApiProtocol::OpenaiChatCompletions
    );
}

#[test]
fn build_config_adds_managed_provider_and_preserves_existing_fields() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("openclaw.json");
    fs::write(&path, FRESH_CONFIG).expect("seed config");

    let updated = OpenClawAdapter
        .build_config(&file_detection(path.clone()), &desired())
        .expect("build");
    fs::write(&path, &updated).expect("apply");

    let root: Value = serde_json::from_slice(&updated).expect("json");
    // 用户已有字段全部保留。
    assert_eq!(
        root.pointer("/gateway/auth/token"),
        Some(&Value::String("test-token".to_owned()))
    );
    assert_eq!(
        root.pointer("/plugins/entries/anthropic/config/sessionCatalog/enabled"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        root.pointer("/meta/lastTouchedVersion"),
        Some(&Value::String("2026.10.5".to_owned()))
    );
    // 受管 Provider 与默认模型就位。
    assert_eq!(
        root.pointer("/models/providers/蒙云智算/baseUrl"),
        Some(&Value::String("http://127.0.0.1:54187/v1".to_owned()))
    );
    assert_eq!(
        root.pointer("/models/providers/蒙云智算/api"),
        Some(&Value::String("openai-completions".to_owned()))
    );
    assert_eq!(
        root.pointer("/agents/defaults/model/primary"),
        Some(&Value::String("蒙云智算/glm-test".to_owned()))
    );

    OpenClawAdapter
        .verify_config(&file_detection(path.clone()), &desired())
        .expect("verify");
}

#[test]
fn repeated_switch_replaces_the_managed_provider_idempotently() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("openclaw.json");
    fs::write(&path, b"{}\n").expect("seed");

    let detection = file_detection(path.clone());
    let first = desired();
    fs::write(
        &path,
        OpenClawAdapter
            .build_config(&detection, &first)
            .expect("first"),
    )
    .expect("write first");

    let second = DesiredAgentBinding {
        model_id: "glm-next",
        ..desired()
    };
    let updated = OpenClawAdapter
        .build_config(&detection, &second)
        .expect("second");
    fs::write(&path, &updated).expect("write second");
    OpenClawAdapter
        .verify_config(&detection, &second)
        .expect("verify second");

    let root: Value = serde_json::from_slice(&updated).expect("json");
    let providers = root
        .pointer("/models/providers")
        .and_then(Value::as_object)
        .expect("providers");
    // 旧的受管条目（at-switch* 与上一轮蒙云智算模型）被整体替换，不累积。
    assert!(providers.keys().all(|key| key == "蒙云智算"));
    assert_eq!(
        root.pointer("/agents/defaults/model/primary"),
        Some(&Value::String("蒙云智算/glm-next".to_owned()))
    );
    let models = providers["蒙云智算"]["models"].as_array().expect("models");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["id"], "glm-next");
}

#[test]
fn build_native_config_restores_baseline_and_cleans_managed_fields() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("openclaw.json");
    let native = br#"{
      "agents": {"defaults": {"model": {"primary": "custom/NMauto"}}},
      "models": {"providers": {"custom": {"baseUrl": "https://x/v1"}}}
    }"#;
    fs::write(&path, native).expect("seed");
    let detection = file_detection(path.clone());

    let restored = OpenClawAdapter
        .build_native_config(
            &detection,
            &BaselineSnapshot {
                existed: true,
                content: native.to_vec(),
            },
        )
        .expect("restore");
    let root: Value = serde_json::from_slice(&restored).expect("json");
    assert_eq!(
        root.pointer("/agents/defaults/model/primary"),
        Some(&Value::String("custom/NMauto".to_owned()))
    );
    assert!(root.pointer("/models/providers/at-switch").is_none());

    // baseline 不存在（从未接管过）时移除受管 primary，其余字段保留。
    let applied = OpenClawAdapter
        .build_config(&detection, &desired())
        .expect("apply");
    fs::write(&path, applied).expect("write applied");
    let restored_without_baseline = OpenClawAdapter
        .build_native_config(
            &detection,
            &BaselineSnapshot {
                existed: false,
                content: Vec::new(),
            },
        )
        .expect("restore without baseline");
    let root: Value = serde_json::from_slice(&restored_without_baseline).expect("json");
    assert!(root.pointer("/agents/defaults/model/primary").is_none());
    assert!(root.pointer("/models/providers/at-switch").is_none());
    // 用户侧 provider 条目（蒙云智算）按内核约定保留，只清受管模型指向。
}

#[test]
fn unparseable_config_is_rejected() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join("openclaw.json");
    fs::write(&path, b"{ not valid json //").expect("seed");
    let error = OpenClawAdapter
        .build_config(&file_detection(path), &desired())
        .expect_err("must refuse");
    assert_eq!(error.code, "agent_config_unparseable");
}

#[test]
fn build_sidecar_updates_targets_the_sessions_state() {
    let temp = tempdir().expect("temp");
    let config = temp.path().join("openclaw.json");
    fs::write(&config, b"{}").expect("seed config");
    let sessions = temp.path().join("agents/main/sessions/sessions.json");
    fs::create_dir_all(sessions.parent().expect("sessions parent")).expect("sessions dir");
    fs::write(
        &sessions,
        br#"{"agent:main:thread:a": {"sessionId": "a", "model": "NMauto"}}"#,
    )
    .expect("seed sessions");

    let updates = OpenClawAdapter
        .build_sidecar_updates(&file_detection(config), &desired())
        .expect("sidecars");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].path, sessions);

    let updated: Value = serde_json::from_slice(&updates[0].new_content).expect("json");
    assert_eq!(updated["agent:main:thread:a"]["model"], "glm-test");
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn discovery_context(temp: &tempfile::TempDir, with_cli: bool) -> DiscoveryContext {
    let home = temp.path().join("home");
    if with_cli {
        let cli = home.join(".openclaw/bin/openclaw");
        fs::create_dir_all(cli.parent().expect("cli parent")).expect("cli dir");
        fs::write(&cli, b"#!/bin/sh\n").expect("cli");
    }
    #[cfg(target_os = "macos")]
    let application_dirs = {
        let root = temp.path().join("Applications");
        vec![root]
    };
    #[cfg(target_os = "windows")]
    let (application_dirs, local_app_data) = {
        let root = temp.path().join("LocalAppData");
        (Vec::new(), Some(root))
    };
    DiscoveryContext {
        home,
        application_data_dir: temp.path().join("ApplicationData"),
        application_dirs,
        path_entries: Vec::new(),
        system_application_search: false,
        custom_installation_path: None,
        #[cfg(target_os = "windows")]
        local_app_data,
        #[cfg(target_os = "windows")]
        program_files: Vec::new(),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn detect_prefers_the_desktop_installation_over_the_cli() {
    let temp = tempdir().expect("temp");
    let app = temp.path().join("Applications/OpenClaw.app");
    fs::create_dir_all(&app).expect("app");
    let config = temp.path().join("home/.openclaw/openclaw.json");
    fs::create_dir_all(config.parent().expect("config parent")).expect("openclaw home");
    fs::write(&config, b"{}\n").expect("config");

    let detection = OpenClawAdapter.detect(&discovery_context(&temp, true));
    assert_eq!(detection.id, "openclaw");
    assert_eq!(detection.install_status, AgentInstallStatus::Installed);
    assert!(detection.needs_restart);
    assert_eq!(
        detection.installation.expect("installation").kind,
        crate::agents::locator::InstallationKind::DesktopApp
    );
    assert_eq!(detection.config_path, Some(config));
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn detect_falls_back_to_the_official_cli_installation() {
    let temp = tempdir().expect("temp");
    let detection = OpenClawAdapter.detect(&discovery_context(&temp, true));
    assert_eq!(
        detection.installation.expect("installation").kind,
        crate::agents::locator::InstallationKind::Command
    );
    assert_eq!(
        detection.config_path,
        Some(temp.path().join("home/.openclaw/openclaw.json"))
    );
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn detect_without_installation_or_config_reports_not_installed() {
    let temp = tempdir().expect("temp");
    let detection = OpenClawAdapter.detect(&discovery_context(&temp, false));
    assert!(detection.installation.is_none());
    assert_eq!(detection.install_status, AgentInstallStatus::NotInstalled);
}
