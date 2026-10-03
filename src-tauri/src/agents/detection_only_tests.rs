use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::tempdir;

use super::*;
use crate::domain::{AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol};

fn desired() -> DesiredAgentBinding<'static> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "蒙云智算",
        model_id: "glm-5.2",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://api.example.test/v1",
        credential: "not-a-real-key",
    }
}

fn installed_context(home: &Path) -> DiscoveryContext {
    #[cfg(target_os = "macos")]
    {
        let applications = home.join("Applications");
        fs::create_dir_all(applications.join("QwenWorkCN.app")).expect("qwen work app");
        fs::create_dir_all(applications.join("DoubaoWork.app")).expect("doubao work app");
        fs::create_dir_all(applications.join("Coze.app")).expect("coze app");
        fs::create_dir_all(applications.join("AionClaw.app")).expect("aionclaw app");
        fs::create_dir_all(applications.join("ZCode.app")).expect("zcode app");
        fs::create_dir_all(applications.join("Accio.app")).expect("accio app");
        DiscoveryContext {
            home: home.to_path_buf(),
            application_data_dir: home.join("Library/Application Support"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            system_candidates: None,
        }
    }

    #[cfg(target_os = "windows")]
    {
        let local_app_data = home.join("AppData/Local");
        for executable in [
            "Programs/QwenWork/QwenWork.exe",
            "Programs/DoubaoWork/DoubaoWork.exe",
            "Programs/Coze/Coze.exe",
            "Programs/AionClaw/AionClaw.exe",
            "Programs/ZCode/ZCode.exe",
            "Programs/Accio/Accio.exe",
        ] {
            let path = local_app_data.join(executable);
            fs::create_dir_all(path.parent().expect("executable parent")).expect("app directory");
            fs::write(path, b"executable").expect("executable");
        }
        DiscoveryContext {
            home: home.to_path_buf(),
            application_data_dir: home.join("AppData/Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            system_candidates: None,
            local_app_data: Some(local_app_data),
            program_files: Vec::new(),
        }
    }
}

fn seed_config_candidate(home: &PathBuf, config_path: &PathBuf) {
    fs::create_dir_all(config_path).expect("config directory");
    assert!(config_path.starts_with(home));
}

#[test]
fn detection_only_agent_metadata_is_stable() {
    assert_eq!(QWEN_WORK_ADAPTER.id(), "qwenwork");
    assert_eq!(QWEN_WORK_ADAPTER.display_name(), "千问办公");
    assert_eq!(DOUBAO_WORK_ADAPTER.id(), "doubaowork");
    assert_eq!(DOUBAO_WORK_ADAPTER.display_name(), "豆包工作");
    assert_eq!(COZE_ADAPTER.id(), "coze");
    assert_eq!(COZE_ADAPTER.display_name(), "扣子");
    assert_eq!(IMA_ADAPTER.id(), "ima");
    assert_eq!(IMA_ADAPTER.display_name(), "ima");
    assert_eq!(KIMI_WORK_ADAPTER.id(), "kimiwork");
    assert_eq!(KIMI_WORK_ADAPTER.display_name(), "Kimi Work");
    assert_eq!(ACCIO_ADAPTER.id(), "accio");
    assert_eq!(ACCIO_ADAPTER.display_name(), "Accio");
}

#[test]
fn installed_detection_only_agents_stay_read_only() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    #[cfg(target_os = "macos")]
    let config_root = home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let config_root = home.join("AppData/Local");
    seed_config_candidate(&home, &config_root.join("QwenWorkCN"));
    seed_config_candidate(&home, &config_root.join("DoubaoWork"));
    seed_config_candidate(&home, &config_root.join("Coze"));
    let context = installed_context(&home);

    for adapter in [
        &QWEN_WORK_ADAPTER,
        &DOUBAO_WORK_ADAPTER,
        &COZE_ADAPTER,
        &ACCIO_ADAPTER,
    ] {
        let detection = adapter.detect(&context);
        assert_eq!(detection.id, adapter.id());
        assert_eq!(detection.install_status, AgentInstallStatus::Installed);
        assert!(matches!(
            detection.config_health,
            AgentConfigHealth::Healthy
        ));
        assert!(!detection.write_supported);
        assert!(!detection.needs_restart);
        assert!(detection.message.is_some_and(|message| {
            message.contains("不修改") || message.contains("不写入") || message.contains("待实现")
        }));
    }
}

#[test]
fn detection_only_agents_reject_writes_with_stable_errors() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let config_path = home.join("config");
    let detection = AgentDetection {
        id: QWEN_WORK_ADAPTER.id(),
        display_name: QWEN_WORK_ADAPTER.display_name(),
        installation: None,
        config_path: Some(config_path),
        runtime_data_dir: None,
        install_status: AgentInstallStatus::Installed,
        config_health: AgentConfigHealth::Healthy,
        write_supported: false,
        needs_restart: false,
        message: None,
        custom_install_path: None,
        using_custom_install_path: false,
    };

    for adapter in [
        &QWEN_WORK_ADAPTER,
        &DOUBAO_WORK_ADAPTER,
        &COZE_ADAPTER,
        &ACCIO_ADAPTER,
    ] {
        let error = adapter
            .validate_binding(&desired())
            .expect_err("unsupported");
        assert_eq!(error.code, format!("{}_write_unsupported", adapter.id()));
        let error = adapter
            .build_config(&detection, &desired())
            .expect_err("unsupported");
        assert_eq!(error.code, format!("{}_write_unsupported", adapter.id()));
        let error = adapter
            .verify_config(&detection, &desired())
            .expect_err("unsupported");
        assert_eq!(error.code, format!("{}_write_unsupported", adapter.id()));
    }
}
