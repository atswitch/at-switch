use std::fs;

use serde_json::{json, Value};
use tempfile::tempdir;

use super::*;
use crate::domain::{AgentConfigHealth, AgentInstallStatus};

fn detection(config_path: PathBuf, runtime_data_dir: PathBuf) -> AgentDetection {
    AgentDetection {
        id: "dumate",
        display_name: "百度搭子",
        installation: None,
        config_path: Some(config_path),
        runtime_data_dir: Some(runtime_data_dir),
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
        provider_name: "蒙云",
        model_id: "GLM-5.2",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiResponses,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "http://127.0.0.1:18123/v1/",
        credential: "local-token",
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn detects_the_active_accounts_persistent_xdg_override() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let application_data_dir = temp.path().join("app-data");
    let app_data = application_data_dir.join("qianfan-desktop-app");
    let override_config = app_data.join("qianfan_desk_xdg/user-1/config/opencode/opencode.jsonc");
    fs::create_dir_all(&app_data).expect("app data");
    fs::create_dir_all(override_config.parent().expect("override parent")).expect("override");
    fs::write(
        app_data.join("auth.json"),
        serde_json::to_vec(&json!({
            "activeProfileId": "profile-1",
            "accountProfiles": [{
                "profileId": "profile-1",
                "bceUserId": "user-1"
            }]
        }))
        .expect("auth json"),
    )
    .expect("auth");
    fs::write(&override_config, b"{}\n").expect("override config");

    #[cfg(target_os = "macos")]
    let context = {
        let applications = temp.path().join("Applications");
        fs::create_dir_all(applications.join("DuMate.app")).expect("app bundle");
        DiscoveryContext {
            home,
            application_data_dir,
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        }
    };

    #[cfg(target_os = "windows")]
    let context = {
        let local_app_data = temp.path().join("local-app-data");
        let executable = local_app_data.join("Programs/DuMate/DuMate.exe");
        fs::create_dir_all(executable.parent().expect("executable parent"))
            .expect("application directory");
        fs::write(executable, b"").expect("executable");
        DiscoveryContext {
            home,
            application_data_dir,
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: Some(local_app_data),
            program_files: Vec::new(),
        }
    };

    let found = DuMateAdapter.detect(&context);
    assert_eq!(found.install_status, AgentInstallStatus::Installed);
    assert_eq!(
        found.config_path.as_deref(),
        Some(override_config.as_path())
    );
    assert_eq!(
        found.runtime_data_dir.as_deref(),
        Some(app_data.join("qianfan_desk_xdg/user-1").as_path())
    );
    assert!(found.write_supported);
    assert!(found.needs_restart);
}

#[test]
fn resolves_the_active_account_from_array_or_object_profiles() {
    for object_profiles in [false, true] {
        let temp = tempdir().expect("temp");
        let app_data = temp.path().join("qianfan-desktop-app");
        fs::create_dir_all(&app_data).expect("app data");
        let profiles = if object_profiles {
            json!({
                "profile-1": { "bceUserId": "user-1", "lastLogin": 500 },
                "profile-2": { "bceUserId": "user-2", "lastLogin": 100 }
            })
        } else {
            json!([
                { "profileId": "profile-1", "bceUserId": "user-1", "lastLogin": 500 },
                { "profileId": "profile-2", "bceUserId": "user-2", "lastLogin": 100 }
            ])
        };
        fs::write(
            app_data.join("auth.json"),
            serde_json::to_vec(&json!({
                "activeProfileId": "profile-2",
                "accountProfiles": profiles
            }))
            .expect("json"),
        )
        .expect("auth");

        assert_eq!(
            resolve_user_id(&app_data).expect("resolve").as_deref(),
            Some("user-2")
        );
    }
}

#[test]
fn invalid_active_account_does_not_silently_modify_another_account() {
    let temp = tempdir().expect("temp");
    let app_data = temp.path().join("qianfan-desktop-app");
    fs::create_dir_all(&app_data).expect("app data");
    fs::write(
        app_data.join("auth.json"),
        serde_json::to_vec(&json!({
            "activeProfileId": "missing-profile",
            "accountProfiles": [
                { "profileId": "profile-1", "bceUserId": "user-1", "lastLogin": 500 }
            ]
        }))
        .expect("json"),
    )
    .expect("auth");

    let error = resolve_user_id(&app_data).expect_err("invalid active account");
    assert_eq!(error.code, "dumate_active_account_invalid");
}

#[test]
fn scans_only_valid_xdg_account_directories_when_auth_is_absent() {
    let temp = tempdir().expect("temp");
    let app_data = temp.path().join("qianfan-desktop-app");
    let xdg = app_data.join("qianfan_desk_xdg");
    for name in ["global", "data", ".hidden", "../escape", "valid-user"] {
        if name != "../escape" {
            fs::create_dir_all(xdg.join(name)).expect("directory");
        }
    }

    assert_eq!(
        resolve_user_id(&app_data).expect("resolve").as_deref(),
        Some("valid-user")
    );
}

#[test]
fn builds_a_persistent_xdg_override_without_mutating_generated_config() {
    let temp = tempdir().expect("temp");
    let config_path = temp
        .path()
        .join("xdg/user-1/config/opencode/opencode.jsonc");
    let runtime_data_dir = temp.path().join("xdg/user-1");
    let native_path = runtime_data_dir.join("config/opencode/opencode.json");
    fs::create_dir_all(config_path.parent().expect("override")).expect("override");
    fs::create_dir_all(native_path.parent().expect("native parent")).expect("native parent");
    let original_override = json!({
        "$schema": "https://opencode.ai/config.json",
        "instructions": ["AGENTS.md"],
        "provider": { "other": { "models": {} } }
    });
    let native = json!({
        "model": "QianfanPersonalQuota/glm-5",
        "provider": {
            "QianfanPersonalQuota": {
                "models": {
                    "glm-5": { "id": "model-text" },
                    "model-artifact-validate": { "id": "model-artifact-validate" },
                    "future-native-alias": { "id": "future-model" }
                }
            }
        }
    });
    fs::write(
        &config_path,
        serde_json::to_vec(&original_override).expect("json"),
    )
    .expect("override config");
    let native_bytes = serde_json::to_vec(&native).expect("json");
    fs::write(&native_path, &native_bytes).expect("native config");
    let detection = detection(config_path.clone(), runtime_data_dir);
    let desired = desired();

    let bytes = DuMateAdapter
        .build_config(&detection, &desired)
        .expect("build");
    fs::write(&config_path, &bytes).expect("apply");
    DuMateAdapter
        .verify_config(&detection, &desired)
        .expect("verify");

    let updated: Value = serde_json::from_slice(&bytes).expect("updated json");
    assert_eq!(updated["model"], "at-switch/GLM-5.2");
    assert_eq!(updated["small_model"], "at-switch/GLM-5.2");
    assert_eq!(updated["instructions"], json!(["AGENTS.md"]));
    assert!(updated.pointer("/provider/other").is_some());
    for alias in ["glm-5", "model-artifact-validate", "future-native-alias"] {
        assert_eq!(
            updated.pointer(&format!(
                "/provider/QianfanPersonalQuota/models/{alias}/headers/{MODEL_TARGET_HEADER}"
            )),
            Some(&json!("GLM-5.2"))
        );
    }
    assert_eq!(
        fs::read(&native_path).expect("native remains"),
        native_bytes
    );
}

#[test]
fn repeated_switch_replaces_every_managed_model_without_stale_entries() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("opencode.jsonc");
    let detection = detection(config_path.clone(), temp.path().join("xdg/user-1"));
    fs::write(&config_path, b"{}\n").expect("initial config");

    let first = DuMateAdapter
        .build_config(&detection, &desired())
        .expect("first switch");
    fs::write(&config_path, first).expect("apply first switch");
    let mut second_binding = desired();
    second_binding.model_id = "second-model";
    let second = DuMateAdapter
        .build_config(&detection, &second_binding)
        .expect("second switch");
    fs::write(&config_path, &second).expect("apply second switch");
    DuMateAdapter
        .verify_config(&detection, &second_binding)
        .expect("verify second switch");

    let updated: Value = serde_json::from_slice(&second).expect("updated json");
    let managed_models = updated["provider"][MANAGED_PROVIDER]["models"]
        .as_object()
        .expect("managed models");
    assert_eq!(managed_models.len(), 1);
    assert!(managed_models.contains_key("second-model"));
    assert!(updated["provider"][NATIVE_PROVIDER]["models"]
        .as_object()
        .expect("native aliases")
        .values()
        .all(|model| model["id"] == "second-model"));
}

#[test]
fn native_restore_removes_only_managed_values_and_preserves_user_content() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("opencode.jsonc");
    let detection = detection(config_path.clone(), temp.path().join("xdg/user-1"));
    let baseline_value = json!({
        "model": "user-provider/original",
        "provider": {
            "at-switch": { "name": "User provider", "models": {} },
            "QianfanPersonalQuota": { "name": "User override", "models": {} },
            "user-provider": { "models": { "original": { "id": "original" } } }
        },
        "theme": "system"
    });
    fs::write(
        &config_path,
        serde_json::to_vec(&baseline_value).expect("json"),
    )
    .expect("baseline config");
    let managed = DuMateAdapter
        .build_config(&detection, &desired())
        .expect("managed");
    fs::write(&config_path, managed).expect("apply");
    let baseline = BaselineSnapshot {
        existed: true,
        content: serde_json::to_vec(&baseline_value).expect("baseline json"),
    };

    let restored = DuMateAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    let restored: Value = serde_json::from_slice(&restored).expect("restored json");
    assert_eq!(restored["model"], "user-provider/original");
    assert_eq!(restored["theme"], "system");
    assert_eq!(
        restored["provider"][NATIVE_PROVIDER]["name"],
        "User override"
    );
    assert_eq!(
        restored["provider"][MANAGED_PROVIDER]["name"],
        "User provider"
    );
}

#[test]
fn native_restore_removes_project_overrides_when_the_original_used_native_defaults() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("opencode.jsonc");
    let detection = detection(config_path.clone(), temp.path().join("xdg/user-1"));
    let baseline_value = json!({
        "$schema": "https://opencode.ai/config.json",
        "skills": { "paths": ["user-skill"] }
    });
    fs::write(
        &config_path,
        serde_json::to_vec(&baseline_value).expect("baseline json"),
    )
    .expect("baseline config");
    let managed = DuMateAdapter
        .build_config(&detection, &desired())
        .expect("managed");
    fs::write(&config_path, managed).expect("apply");
    let baseline = BaselineSnapshot {
        existed: true,
        content: serde_json::to_vec(&baseline_value).expect("baseline json"),
    };

    let restored = DuMateAdapter
        .build_native_config(&detection, &baseline)
        .expect("restore");
    let restored: Value = serde_json::from_slice(&restored).expect("restored json");
    assert_eq!(restored["skills"], json!({ "paths": ["user-skill"] }));
    assert!(restored.get("model").is_none());
    assert!(restored.get("small_model").is_none());
    assert!(restored.get("provider").is_none());
}

#[test]
fn native_restore_does_not_revive_a_legacy_at_switch_baseline() {
    let temp = tempdir().expect("temp");
    let config_path = temp.path().join("opencode.jsonc");
    let detection = detection(config_path.clone(), temp.path().join("xdg/user-1"));
    let legacy_baseline = json!({
        "model": "at-switch/legacy-model",
        "provider": {
            "at-switch": {
                "name": "at-switch",
                "models": { "legacy-model": { "id": "legacy-model" } }
            }
        }
    });
    fs::write(
        &config_path,
        DuMateAdapter
            .build_config(&detection, &desired())
            .expect("managed config"),
    )
    .expect("apply");

    let restored = DuMateAdapter
        .build_native_config(
            &detection,
            &BaselineSnapshot {
                existed: true,
                content: serde_json::to_vec(&legacy_baseline).expect("legacy baseline"),
            },
        )
        .expect("restore");
    let restored: Value = serde_json::from_slice(&restored).expect("restored json");
    assert!(restored.get("model").is_none());
    assert!(restored.get("provider").is_none());
}

#[test]
fn direct_mode_rejects_non_chat_protocols_but_proxy_mode_accepts_them() {
    let mut direct = desired();
    direct.mode = AgentBindingMode::Direct;
    assert!(DuMateAdapter.validate_binding(&direct).is_err());

    direct.upstream_protocol = ApiProtocol::OpenaiChatCompletions;
    assert!(DuMateAdapter.validate_binding(&direct).is_ok());
    assert_eq!(
        DuMateAdapter.source_protocol(AgentBindingMode::Proxy, ApiProtocol::AnthropicMessages),
        ApiProtocol::OpenaiChatCompletions
    );
}
