use crate::domain::{ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct TraeAdapter;

impl AgentAdapter for TraeAdapter {
    fn id(&self) -> &'static str {
        "trae"
    }

    fn display_name(&self) -> &'static str {
        "TRAE"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["TRAE.app", "Trae.app"],
            &["com.trae.app"],
            &[
                "Programs/Trae/Trae.exe",
                "Programs/TRAE/TRAE.exe",
                "Trae/Trae.exe",
                "TRAE/TRAE.exe",
            ],
        );
        AgentDetection::manual(
            self.id(),
            self.display_name(),
            installation,
            "TRAE 已安装；其自定义模型由登录态内部存储管理，请在 TRAE 设置 → 模型 → 自定义模型中配置直连",
        )
    }

    fn source_protocol(
        &self,
        _desired_mode: crate::domain::AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn build_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        Err(manual_configuration_error())
    }

    fn build_native_config(
        &self,
        _detection: &AgentDetection,
        _baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        Err(manual_configuration_error())
    }

    fn verify_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        Err(manual_configuration_error())
    }
}

fn manual_configuration_error() -> CommandError {
    CommandError::new(
        "trae_login_configuration_required",
        "TRAE 没有公开稳定的本地模型配置文件",
    )
    .with_recovery("请在 TRAE 设置 → 模型 → 自定义模型中填写完整请求地址、模型 ID 和 API Key。")
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use std::fs;

    #[cfg(target_os = "macos")]
    use super::*;

    #[cfg(target_os = "macos")]
    use crate::domain::AgentInstallStatus;

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_a_closed_macos_trae_installation_by_bundle_identity() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let applications = temp.path().join("Applications");
        let app = applications.join("Renamed Trae.app");
        fs::create_dir_all(app.join("Contents")).expect("app bundle");

        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".to_owned(),
            plist::Value::String("com.trae.app".to_owned()),
        );
        plist::to_file_xml(
            app.join("Contents/Info.plist"),
            &plist::Value::Dictionary(dictionary),
        )
        .expect("plist");

        let context = DiscoveryContext {
            home: home.clone(),
            application_data_dir: home.join("Library/Application Support"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        };
        let detected = TraeAdapter.detect(&context);

        assert_eq!(detected.install_status, AgentInstallStatus::Installed);
        assert!(!detected.write_supported);
        assert_eq!(
            detected
                .installation
                .as_ref()
                .map(|item| item.path.as_path()),
            Some(app.as_path())
        );
    }
}
