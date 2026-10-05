use std::{fs, net::IpAddr, path::Path};

use futures_util::future::BoxFuture;

use crate::{
    domain::{
        AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol, AppResult,
        CommandError,
    },
    services::{BaselineSnapshot, ConfigTransaction},
};

use super::{
    ima::{active_account, ImaBindingManager},
    ima_local,
    locator::{locate_desktop_app, DiscoveryContext},
    service_adapter::{CommitBinding, ServiceConfigAdapter, ServiceConfigOutcome},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

#[derive(Default)]
pub(super) struct ImaAdapter {
    bindings: ImaBindingManager,
}

impl AgentAdapter for ImaAdapter {
    fn id(&self) -> &'static str {
        "ima"
    }
    fn display_name(&self) -> &'static str {
        "ima"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["ima.copilot.app"],
            &["com.tencent.imamac"],
            &["ima.copilot/Application/ima.copilot.exe"],
        );
        #[cfg(target_os = "windows")]
        let root = context
            .local_app_data
            .clone()
            .unwrap_or_else(|| context.home.join("AppData/Local"))
            .join("ima.copilot/User Data");
        #[cfg(not(target_os = "windows"))]
        let root = context.application_data_dir.join("com.tencent.imamac");
        let preferences = root.join("Default/Preferences");
        let mut detection = AgentDetection::manual(
            "ima",
            "ima",
            installation,
            "请先打开 ima 并登录，再连接模型设置。",
        );
        detection.config_path = Some(preferences.clone());
        if let Some(installation) = detection.installation.as_mut() {
            if let Ok(version) = fs::read_to_string(root.join("Last IMA Version")) {
                let version = version.trim();
                if !version.is_empty()
                    && version
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'.')
                {
                    installation.version = Some(version.to_owned());
                }
            }
        }
        detection.runtime_data_dir = Some(root);
        if detection.installation.is_none() {
            return detection;
        }
        if !cfg!(any(target_os = "macos", target_os = "windows")) {
            detection.message = Some("当前版本尚未验证此系统的 ima 登录态读取方式。".to_owned());
            return detection;
        }
        match active_account(&preferences)
            .and_then(|_| ima_local::snapshot(&preferences))
            .and_then(|_| web_version(&preferences))
        {
            Ok(_) => {
                detection.config_health = AgentConfigHealth::Healthy;
                detection.write_supported = true;
                detection.needs_restart = true;
                detection.message = Some(
                    "ima 已识别。首次切换会连接当前账号；运行中切换后自动重新打开。".to_owned(),
                );
            }
            Err(error) => {
                detection.install_status = AgentInstallStatus::InstalledUninitialized;
                detection.config_health = AgentConfigHealth::Unreadable;
                detection.message = Some(error.message);
            }
        }
        detection
    }

    fn source_protocol(&self, _: AgentBindingMode, _: ApiProtocol) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode != AgentBindingMode::Direct
            || desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "ima_protocol_unsupported",
                "ima 目前支持公网 OpenAI Chat 直连，暂不支持本地代理。",
            ));
        }
        validate_public_endpoint(desired.base_url)
    }

    fn build_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<Vec<u8>> {
        Err(service_required())
    }
    fn build_native_config(&self, _: &AgentDetection, _: &BaselineSnapshot) -> AppResult<Vec<u8>> {
        Err(service_required())
    }
    fn verify_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<()> {
        Err(service_required())
    }
    fn service_config(&self) -> Option<&dyn ServiceConfigAdapter> {
        Some(self)
    }
}

impl ServiceConfigAdapter for ImaAdapter {
    fn checkpoint_status(
        &self,
        detection: &AgentDetection,
        transaction: &ConfigTransaction,
    ) -> AppResult<Option<(bool, bool)>> {
        self.bindings.checkpoint_status(detection, transaction)
    }
    fn account_scope(&self, detection: &AgentDetection) -> AppResult<String> {
        active_account(preferences(detection)?)
    }

    fn apply<'a>(
        &'a self,
        detection: &'a AgentDetection,
        desired: &'a DesiredAgentBinding<'a>,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            let version = web_version(preferences(detection)?)?;
            let outcome = self
                .bindings
                .apply(detection, &version, desired, transaction, commit)
                .await?;
            Ok(ServiceConfigOutcome {
                needs_restart: outcome.needs_restart,
                message: outcome.message,
            })
        })
    }

    fn restore<'a>(
        &'a self,
        detection: &'a AgentDetection,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            let version = web_version(preferences(detection)?)?;
            let outcome = self
                .bindings
                .restore(detection, &version, transaction, commit)
                .await?;
            Ok(ServiceConfigOutcome {
                needs_restart: outcome.needs_restart,
                message: outcome.message,
            })
        })
    }

    fn verify_cached(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
    ) -> AppResult<()> {
        self.bindings.verify_cached(detection, desired, transaction)
    }
}

fn preferences(detection: &AgentDetection) -> AppResult<&Path> {
    detection
        .config_path
        .as_deref()
        .ok_or_else(service_required)
}

pub(super) fn web_version(preferences: &Path) -> AppResult<String> {
    let root = preferences
        .parent()
        .ok_or_else(service_required)?
        .join("Extensions/khmgfdkajnigikondkcjbaflpjflfiee");
    let mut candidates = fs::read_dir(root)
        .map_err(|_| extension_missing())?
        .flatten()
        .filter_map(|entry| {
            let raw = fs::read(entry.path().join("manifest.json")).ok()?;
            let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
            let version = json.get("version")?.as_str()?.to_owned();
            let numbers = version
                .split('.')
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            Some((numbers, version))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates
        .pop()
        .map(|(_, version)| version)
        .ok_or_else(extension_missing)
}

fn service_required() -> CommandError {
    CommandError::new(
        "agent_service_config_required",
        "ima 模型设置需要通过已登录账号同步",
    )
}
fn extension_missing() -> CommandError {
    CommandError::new(
        "ima_extension_unavailable",
        "尚未找到 ima 的模型设置组件，请打开 ima 完成初始化后重试。",
    )
}

fn validate_public_endpoint(base: &str) -> AppResult<()> {
    let url = url::Url::parse(base).map_err(|_| endpoint_error())?;
    let host = url
        .host_str()
        .ok_or_else(endpoint_error)?
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || host == "localhost"
        || !host.contains('.') && !host.contains(':')
        || [".localhost", ".local", ".internal", ".lan", ".home"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
    {
        return Err(endpoint_error());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let private = match ip {
            IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
                    || ip.is_multicast()
                    || ip.octets()[0] == 0
                    || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || (ip.segments()[0] & 0xfe00) == 0xfc00
                    || (ip.segments()[0] & 0xffc0) == 0xfe80
                    || ip.to_ipv4_mapped().is_some_and(|ip| {
                        ip.is_private()
                            || ip.is_loopback()
                            || ip.is_link_local()
                            || ip.is_unspecified()
                    })
            }
        };
        if private {
            return Err(endpoint_error());
        }
    }
    Ok(())
}

fn endpoint_error() -> CommandError {
    CommandError::new(
        "ima_endpoint_unreachable",
        "ima 云端需要可从公网访问的 API 地址，无法使用本机或内网地址。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn context(root: &Path) -> DiscoveryContext {
        DiscoveryContext {
            home: root.join("home"),
            application_data_dir: root.join("application-data"),
            application_dirs: vec![root.join("Applications")],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            #[cfg(target_os = "windows")]
            local_app_data: Some(root.join("local-app-data")),
            #[cfg(target_os = "windows")]
            program_files: vec![root.join("Program Files")],
        }
    }

    fn profile_root(context: &DiscoveryContext) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            context
                .local_app_data
                .clone()
                .unwrap_or_else(|| context.home.join("AppData/Local"))
                .join("ima.copilot/User Data")
        }
        #[cfg(not(target_os = "windows"))]
        {
            context.application_data_dir.join("com.tencent.imamac")
        }
    }

    #[cfg(target_os = "macos")]
    fn create_installation(context: &DiscoveryContext) -> PathBuf {
        let path = context.application_dirs[0].join("ima.copilot.app");
        create_mac_bundle(&path);
        path
    }

    #[cfg(target_os = "macos")]
    fn create_mac_bundle(path: &Path) {
        fs::create_dir_all(path.join("Contents")).unwrap();
        let dictionary = plist::Dictionary::from_iter([
            (
                "CFBundleIdentifier".to_owned(),
                plist::Value::String("com.tencent.imamac".into()),
            ),
            (
                "CFBundleShortVersionString".to_owned(),
                plist::Value::String("150.0.0.0".into()),
            ),
        ]);
        plist::to_file_xml(
            path.join("Contents/Info.plist"),
            &plist::Value::Dictionary(dictionary),
        )
        .unwrap();
    }

    #[cfg(target_os = "windows")]
    fn create_installation(context: &DiscoveryContext) -> PathBuf {
        let path = context
            .local_app_data
            .clone()
            .unwrap_or_else(|| context.home.join("AppData/Local"))
            .join("ima.copilot/Application/ima.copilot.exe");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"example-executable-fixture").unwrap();
        path
    }

    fn create_preferences(context: &DiscoveryContext) -> PathBuf {
        let path = profile_root(context).join("Default/Preferences");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            json!({
                "tencent":{"wxlogin":{"account_meta":json!({
                    "is_login":true,"credential_id":"example-credential",
                    "user_id":"example-user","id_type":1,"token_type":1
                }).to_string()}},
                "kExtraSettingInfo":"{}",
                "unknown":{"keep":true}
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    fn create_extension(preferences: &Path, directory: &str, version: &str) {
        let path = preferences
            .parent()
            .unwrap()
            .join("Extensions/khmgfdkajnigikondkcjbaflpjflfiee")
            .join(directory);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("manifest.json"),
            json!({"version":version}).to_string(),
        )
        .unwrap();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn discovery_uses_the_platform_installation_and_active_profile_without_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let installation = create_installation(&context);
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        fs::write(profile_root(&context).join("Last IMA Version"), "2.6.9").unwrap();
        let before = fs::read(&preferences).unwrap();

        let detection = ImaAdapter::default().detect(&context);

        assert_eq!(detection.installation.as_ref().unwrap().path, installation);
        assert_eq!(
            detection.installation.as_ref().unwrap().version.as_deref(),
            Some("2.6.9")
        );
        assert_eq!(
            detection.config_path.as_deref(),
            Some(preferences.as_path())
        );
        assert_eq!(detection.runtime_data_dir, Some(profile_root(&context)));
        assert!(matches!(
            detection.config_health,
            AgentConfigHealth::Healthy
        ));
        assert!(detection.write_supported);
        assert!(detection.needs_restart);
        assert_eq!(fs::read(&preferences).unwrap(), before);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn missing_installation_remains_unavailable_even_with_profile_data() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        let detection = ImaAdapter::default().detect(&context);
        assert!(detection.installation.is_none());
        assert_eq!(detection.install_status, AgentInstallStatus::NotInstalled);
        assert!(!detection.write_supported);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn installed_ima_requires_login_preferences_and_settings_extension() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        create_installation(&context);
        let adapter = ImaAdapter::default();
        let uninitialized = adapter.detect(&context);
        assert_eq!(
            uninitialized.install_status,
            AgentInstallStatus::InstalledUninitialized
        );
        assert!(!uninitialized.write_supported);

        let preferences = create_preferences(&context);
        let no_extension = adapter.detect(&context);
        assert!(!no_extension.write_supported);
        assert!(no_extension.message.unwrap().contains("模型设置组件"));

        create_extension(&preferences, "5.6.3_0", "5.6.3");
        fs::write(&preferences, b"invalid-example-json").unwrap();
        let invalid = adapter.detect(&context);
        assert!(!invalid.write_supported);
        assert!(!invalid.message.unwrap().contains("invalid-example-json"));
        assert_eq!(fs::read(&preferences).unwrap(), b"invalid-example-json");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn signed_out_account_cannot_enable_automatic_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        create_installation(&context);
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        let mut root: serde_json::Value =
            serde_json::from_slice(&fs::read(&preferences).unwrap()).unwrap();
        root["tencent"]["wxlogin"]["account_meta"] = json!(json!({
            "is_login":false,"credential_id":"example-signed-out-credential",
            "user_id":"example-user","id_type":1,"token_type":1
        })
        .to_string());
        fs::write(&preferences, root.to_string()).unwrap();
        let detection = ImaAdapter::default().detect(&context);
        assert_eq!(
            detection.install_status,
            AgentInstallStatus::InstalledUninitialized
        );
        assert!(!detection.write_supported);
        assert!(!detection
            .message
            .unwrap()
            .contains("example-signed-out-credential"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renamed_macos_custom_bundle_keeps_the_standard_user_profile() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context(directory.path());
        create_installation(&context);
        let custom = directory.path().join("Custom/Personal Assistant.app");
        create_mac_bundle(&custom);
        context.custom_installation_path = Some(custom.clone());
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        let detection = ImaAdapter::default().detect(&context);
        assert_eq!(detection.installation.unwrap().path, custom);
        assert_eq!(detection.config_path, Some(preferences));
        assert!(detection.write_supported);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_custom_installation_keeps_local_app_data_profile() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context(directory.path());
        create_installation(&context);
        let custom_root = directory.path().join("Custom Installation");
        let custom = custom_root.join("Application/ima.copilot.exe");
        fs::create_dir_all(custom.parent().unwrap()).unwrap();
        fs::write(&custom, b"example-custom-executable").unwrap();
        context.custom_installation_path = Some(custom_root);
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        let detection = ImaAdapter::default().detect(&context);
        assert_eq!(detection.installation.unwrap().path, custom);
        assert_eq!(detection.config_path, Some(preferences));
        assert!(detection.write_supported);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_without_local_app_data_uses_the_existing_home_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context(directory.path());
        context.local_app_data = None;
        let installation = create_installation(&context);
        let preferences = create_preferences(&context);
        create_extension(&preferences, "5.6.3_0", "5.6.3");
        let detection = ImaAdapter::default().detect(&context);
        assert_eq!(detection.installation.unwrap().path, installation);
        assert_eq!(detection.config_path, Some(preferences));
    }

    #[test]
    fn settings_manifest_uses_numeric_version_order_and_skips_invalid_entries() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let preferences = create_preferences(&context);
        create_extension(&preferences, "older", "5.9.0");
        create_extension(&preferences, "newer", "5.10.0");
        create_extension(&preferences, "invalid", "example-private-invalid-version");
        assert_eq!(web_version(&preferences).unwrap(), "5.10.0");
    }

    #[test]
    fn local_and_private_endpoints_are_rejected_before_credentials_are_read() {
        for endpoint in [
            "http://localhost:9988/v1",
            "http://127.0.0.1/v1",
            "http://10.0.1.1",
            "http://192.168.1.1",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "http://model.local",
            "https://user:secret@api.example.com",
        ] {
            assert!(validate_public_endpoint(endpoint).is_err(), "{endpoint}");
        }
        assert!(validate_public_endpoint("https://api.example.com/v1").is_ok());
    }
}
