use std::{fs, path::PathBuf};

use toml_edit::{value, DocumentMut, Item, Table};

use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    locator::{locate_command, locate_desktop_app, DiscoveryContext, Installation},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = merge_installations(
            merge_installations(
                locate_desktop_app(
                    context,
                    // Current macOS releases may ship Codex as a standalone app or
                    // as the Codex surface inside ChatGPT.app.
                    &["Codex.app", "ChatGPT.app"],
                    &["com.openai.codex"],
                    &[
                        "Programs/Codex/Codex.exe",
                        "Codex/Codex.exe",
                        "OpenAI/Codex/Codex.exe",
                        "Programs/ChatGPT/ChatGPT.exe",
                        "ChatGPT/ChatGPT.exe",
                        "OpenAI/ChatGPT/ChatGPT.exe",
                    ],
                ),
                locate_command(
                    context,
                    if cfg!(target_os = "windows") {
                        &["codex.exe", "codex.cmd"]
                    } else {
                        &["codex"]
                    },
                ),
            ),
            locate_codex_openai_install(context),
        );
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| context.home.join(".codex"));
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            codex_home.join("config.toml"),
            probe_codex,
            true,
        )
    }

    fn source_protocol(
        &self,
        _mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiResponses
    }

    fn validate_binding(&self, _desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        Ok(())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        self.validate_binding(desired)?;
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 Codex 配置路径")
        })?;
        let source = if path.exists() {
            fs::read_to_string(path)?
        } else {
            String::new()
        };
        build_codex_config(&source, desired)
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 Codex 配置路径")
        })?;
        let current = if path.exists() {
            fs::read_to_string(path)?
        } else {
            String::new()
        };
        let original = if baseline.existed {
            String::from_utf8(baseline.content.clone()).map_err(|_| {
                CommandError::new("baseline_payload_invalid", "Codex 原始配置备份不是 UTF-8")
            })?
        } else {
            String::new()
        };
        build_native_codex_config(&current, &original)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        let path = detection.config_path.as_ref().ok_or_else(|| {
            CommandError::new("agent_config_path_missing", "未找到 Codex 配置路径")
        })?;
        let document = fs::read_to_string(path)?
            .parse::<DocumentMut>()
            .map_err(|_| CommandError::new("agent_config_unparseable", "Codex 配置无法解析"))?;
        let provider = document
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get("at_switch"))
            .and_then(Item::as_table);
        let applied = document.get("model").and_then(Item::as_str) == Some(desired.model_id)
            && document.get("model_provider").and_then(Item::as_str) == Some("at_switch")
            && provider
                .and_then(|table| table.get("base_url"))
                .and_then(Item::as_str)
                == Some(desired.base_url.trim_end_matches('/'));
        if applied {
            Ok(())
        } else {
            Err(CommandError::new(
                "agent_config_not_applied",
                "Codex 未读取到目标 AT-Switch 路由",
            ))
        }
    }
}

fn build_codex_config(source: &str, desired: &DesiredAgentBinding<'_>) -> AppResult<Vec<u8>> {
    let mut document = source
        .parse::<DocumentMut>()
        .map_err(|_| CommandError::new("agent_config_unparseable", "Codex config.toml 无法解析"))?;
    document["model"] = value(desired.model_id);
    document["model_provider"] = value("at_switch");

    match document.get("model_providers") {
        None => {
            document
                .as_table_mut()
                .insert("model_providers", Item::Table(Table::new()));
        }
        Some(item) if item.is_table() => {}
        Some(_) => {
            return Err(CommandError::new(
                "agent_config_shape_unsupported",
                "Codex 的 model_providers 字段不是表",
            ));
        }
    }
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            CommandError::new(
                "agent_config_shape_unsupported",
                "Codex 的 model_providers 字段不是表",
            )
        })?;
    if providers
        .get("at_switch")
        .is_some_and(|item| !item.is_table())
    {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "Codex 的 model_providers.at_switch 字段不是表",
        ));
    }
    if !providers.contains_key("at_switch") {
        providers["at_switch"] = Item::Table(Table::new());
    }
    let provider = providers
        .get_mut("at_switch")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            CommandError::new(
                "agent_config_shape_unsupported",
                "Codex 的 model_providers.at_switch 字段不是表",
            )
        })?;
    provider["name"] = value(desired.provider_name);
    provider["base_url"] = value(desired.base_url.trim_end_matches('/'));
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(false);
    // For proxy mode this is a high-entropy localhost routing token, not
    // the upstream API key. Direct mode is explicit and necessarily uses
    // Codex's supported static-token field.
    provider["experimental_bearer_token"] = value(desired.credential);

    Ok(document.to_string().into_bytes())
}

fn build_native_codex_config(current: &str, original: &str) -> AppResult<Vec<u8>> {
    let mut document = current
        .parse::<DocumentMut>()
        .map_err(|_| CommandError::new("agent_config_unparseable", "Codex config.toml 无法解析"))?;
    let baseline = original
        .parse::<DocumentMut>()
        .map_err(|_| CommandError::new("baseline_payload_invalid", "Codex 原始配置备份无法解析"))?;

    if let Some(item) = baseline.get("model") {
        document.as_table_mut().insert("model", item.clone());
    } else {
        document.as_table_mut().remove("model");
    }

    if let Some(item) = baseline.get("model_provider") {
        document
            .as_table_mut()
            .insert("model_provider", item.clone());
    } else {
        // 显式写入官方默认 "openai"，避免在保留 [model_providers.at_switch] 的情况下
        // Codex 隐式回退到自定义 provider，确保官方通道与自定义通道历史会话精确隔离。
        document["model_provider"] = value("openai");
    }

    // 保留 model_providers.at_switch 及用户已有的 Provider 定义。
    // 类似于 cc-switch 的规范，切回原生只恢复顶层 model/model_provider 指针，
    // 不物理删除历史 Provider 表。这样在 ChatGPT / Codex 桌面端打开历史工作空间会话时，
    // 不会因找不到历史 Provider 定义而触发 "Model provider 'at_switch' not found" 错误。
    Ok(document.to_string().into_bytes())
}

fn probe_codex(path: &PathBuf) -> AppResult<()> {
    fs::read_to_string(path)?
        .parse::<DocumentMut>()
        .map(|_| ())
        .map_err(|_| CommandError::new("agent_config_unparseable", "Codex config.toml 无法解析"))
}

fn merge_installations(
    desktop: Option<Installation>,
    command: Option<Installation>,
) -> Option<Installation> {
    match (desktop, command) {
        (Some(mut desktop), Some(command)) => {
            if desktop.version.is_none() {
                desktop.version = command.version;
            }
            Some(desktop)
        }
        (Some(desktop), None) => Some(desktop),
        (None, Some(command)) => Some(command),
        (None, None) => None,
    }
}

// The Codex Desktop installer (OpenAI's official ChatGPT-app distribution since
// the Codex/ChatGPT merge) drops a hash-stamped directory under
// `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe` and never registers an
// uninstaller, App Paths entry or shortcut that the generic locator can pick
// up. Scan the OpenAI\Codex\bin folder one level deep and prefer the newest
// hash directory so version upgrades are reflected without keeping stale paths.
#[cfg(target_os = "windows")]
fn locate_codex_openai_install(context: &DiscoveryContext) -> Option<Installation> {
    use std::cmp::Reverse;

    use super::locator::InstallationKind;

    let local_app_data = context
        .local_app_data
        .as_ref()
        .cloned()
        .or_else(|| Some(context.home.join("AppData/Local")))?;
    let bin_root = local_app_data.join("OpenAI").join("Codex").join("bin");
    let Ok(entries) = fs::read_dir(&bin_root) else {
        return None;
    };
    let mut candidates: Vec<(Reverse<String>, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let exe = path.join("codex.exe");
            if !exe.is_file() {
                return None;
            }
            let version = super::locator::windows_file_version(&exe);
            Some((Reverse(version.clone().unwrap_or_default()), exe))
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    // Sort by version string when present, then fall back to the directory name
    // (the install hash) so the newest hash wins on ties.
    candidates.sort_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| {
            left.1
                .parent()
                .and_then(|p| p.file_name())
                .cmp(&right.1.parent().and_then(|p| p.file_name()))
        })
    });
    let (_, exe) = candidates.into_iter().next()?;
    let version = super::locator::windows_file_version(&exe);
    Some(Installation {
        path: exe,
        version,
        kind: InstallationKind::DesktopApp,
    })
}

// Non-Windows targets have no `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe`
// install layout; keep a stub so `detect()` can call this unconditionally
// without each platform needing its own call site.
#[cfg(not(target_os = "windows"))]
fn locate_codex_openai_install(_context: &DiscoveryContext) -> Option<Installation> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentConfigHealth, AgentInstallStatus};

    fn desired<'a>(
        mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
        credential: &'a str,
    ) -> DesiredAgentBinding<'a> {
        DesiredAgentBinding {
            mode,
            provider_name: "蒙云智算",
            model_id: "glm-test",
            supports_tools: true,
            upstream_protocol,
            source_protocol: ApiProtocol::OpenaiResponses,
            base_url: "http://127.0.0.1:54187/v1",
            credential,
        }
    }

    #[test]
    fn updates_managed_fields_without_losing_comments_or_projects() {
        let source = r#"# user comment
model = "old-model"

[projects."/workspace"]
trust_level = "trusted"
"#;
        let output = build_codex_config(
            source,
            &desired(
                AgentBindingMode::Proxy,
                ApiProtocol::OpenaiChatCompletions,
                "local-token",
            ),
        )
        .expect("config");
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("# user comment"));
        assert!(text.contains("[projects.\"/workspace\"]"));
        assert!(text.contains("model = \"glm-test\""));
        assert!(text.contains("model_provider = \"at_switch\""));
        assert!(text.contains("wire_api = \"responses\""));
        assert!(text.contains("experimental_bearer_token = \"local-token\""));
    }

    #[test]
    fn replaces_and_verifies_the_selected_model_on_every_switch() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("config.toml");
        fs::write(&path, b"# user settings\n").expect("seed");
        let detection = AgentDetection {
            id: "codex",
            display_name: "Codex",
            installation: None,
            config_path: Some(path.clone()),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };

        fs::write(
            &path,
            CodexAdapter
                .build_config(
                    &detection,
                    &desired(
                        AgentBindingMode::Proxy,
                        ApiProtocol::OpenaiChatCompletions,
                        "local-token",
                    ),
                )
                .expect("first config"),
        )
        .expect("write first");
        let next = DesiredAgentBinding {
            model_id: "glm-next",
            ..desired(
                AgentBindingMode::Proxy,
                ApiProtocol::OpenaiChatCompletions,
                "local-token",
            )
        };
        fs::write(
            &path,
            CodexAdapter
                .build_config(&detection, &next)
                .expect("second config"),
        )
        .expect("write second");
        CodexAdapter
            .verify_config(&detection, &next)
            .expect("verify second");

        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("model = \"glm-next\""));
        assert_eq!(text.matches("[model_providers.at_switch]").count(), 1);
    }

    #[test]
    fn direct_mode_allows_any_upstream() {
        assert!(CodexAdapter
            .validate_binding(&desired(
                AgentBindingMode::Direct,
                ApiProtocol::OpenaiChatCompletions,
                "upstream-key",
            ))
            .is_ok());
    }

    #[test]
    fn native_mode_restores_original_route_and_preserves_new_user_settings() {
        let current = r#"model = "glm-test"
model_provider = "at_switch"
new_user_setting = true

[model_providers.at_switch]
base_url = "http://127.0.0.1:54187/v1"
"#;
        let original = r#"model = "gpt-5"
model_provider = "openai"
"#;
        let text = String::from_utf8(build_native_codex_config(current, original).expect("native"))
            .expect("utf8");
        assert!(text.contains("model = \"gpt-5\""));
        assert!(text.contains("model_provider = \"openai\""));
        assert!(text.contains("new_user_setting = true"));
        // 确保保留 at_switch Provider 定义，兼容历史会话
        assert!(text.contains("[model_providers.at_switch]"));
        assert!(text.contains("base_url = \"http://127.0.0.1:54187/v1\""));
    }

    #[test]
    fn native_mode_preserves_workspace_projects_and_provider_definitions() {
        let current = r#"model = "glm-test"
model_provider = "at_switch"

[projects."/Users/star/develop/project/chathub"]
trust_level = "trusted"

[model_providers.at_switch]
name = "智谱 AI"
base_url = "http://127.0.0.1:54187/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "local-token"
"#;
        let original = "";
        let text = String::from_utf8(build_native_codex_config(current, original).expect("native"))
            .expect("utf8");
        // 顶层 model 移除恢复默认，model_provider 显式恢复为官方通道 openai
        assert!(!text.contains("model = \"glm-test\""));
        assert!(text.contains("model_provider = \"openai\""));
        assert!(!text.contains("model_provider = \"at_switch\""));
        // 工作空间项目配置与 Provider 定义必须完好保留
        assert!(text.contains("[projects.\"/Users/star/develop/project/chathub\"]"));
        assert!(text.contains("trust_level = \"trusted\""));
        assert!(text.contains("[model_providers.at_switch]"));
        assert!(text.contains("name = \"智谱 AI\""));
    }

    #[test]
    fn round_trip_switch_preserves_custom_providers_and_workspaces() {
        let seed = r#"# User base config
[projects."/Users/star/develop/project/chathub"]
trust_level = "trusted"

[model_providers.custom_direct]
name = "My Custom Provider"
base_url = "https://custom.api.com/v1"
wire_api = "responses"
"#;
        // 1. 切换到 AT-Switch 自定义模型
        let switched_bytes = build_codex_config(
            seed,
            &desired(
                AgentBindingMode::Proxy,
                ApiProtocol::OpenaiChatCompletions,
                "token-123",
            ),
        )
        .expect("build switched");
        let switched_text = String::from_utf8(switched_bytes).expect("utf8");
        assert!(switched_text.contains("model = \"glm-test\""));
        assert!(switched_text.contains("model_provider = \"at_switch\""));
        assert!(switched_text.contains("[model_providers.custom_direct]"));
        assert!(switched_text.contains("[projects.\"/Users/star/develop/project/chathub\"]"));

        // 2. 切回原生官方模型
        let native_bytes = build_native_codex_config(&switched_text, seed).expect("build native");
        let native_text = String::from_utf8(native_bytes).expect("utf8");
        assert!(!native_text.contains("model = \"glm-test\""));
        assert!(native_text.contains("model_provider = \"openai\""));
        assert!(!native_text.contains("model_provider = \"at_switch\""));
        assert!(native_text.contains("[model_providers.custom_direct]"));
        assert!(native_text.contains("[model_providers.at_switch]"));
        assert!(native_text.contains("[projects.\"/Users/star/develop/project/chathub\"]"));
        assert!(native_text.contains("# User base config"));
    }
}
