use std::{fs, path::PathBuf};

use serde_json::{json, Map, Value};

use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct QClawAdapter;
pub struct AutoClawAdapter;

const AUTOCLAW_PROVIDER_ID: &str = "at-switch";
// AutoClaw 1.14+ derives the generated OpenClaw provider key from configId.
// Keep it stable across model switches so the old generated provider is
// replaced instead of accumulating a new provider on every switch.
const AUTOCLAW_CONFIG_ID: &str = "at-switch-managed";

impl AgentAdapter for QClawAdapter {
    fn id(&self) -> &'static str {
        "qclaw"
    }

    fn display_name(&self) -> &'static str {
        "QClaw"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["QClaw.app"],
            &["com.tencent.qclaw"],
            &[
                "Programs/QClaw/QClaw.exe",
                "QClaw/QClaw.exe",
                "Tencent/QClaw/QClaw.exe",
            ],
        );
        let state_dir = context.home.join(".qclaw");
        let config_path = qclaw_runtime_config_path(&state_dir)
            .unwrap_or_else(|| state_dir.join("openclaw.json"));
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            config_path,
            probe_openclaw,
            true,
        )
    }

    fn source_protocol(
        &self,
        mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        openclaw_source_protocol(mode, upstream_protocol)
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        build_openclaw_config(detection, desired)
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        build_native_openclaw_config(detection, baseline)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        verify_openclaw_config(detection, desired)
    }
}

impl AgentAdapter for AutoClawAdapter {
    fn id(&self) -> &'static str {
        "autoclaw"
    }

    fn display_name(&self) -> &'static str {
        "AutoClaw"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["AutoClaw.app"],
            &["com.zhipuai.autoclaw"],
            &[
                "Programs/AutoClaw/AutoClaw.exe",
                "AutoClaw/AutoClaw.exe",
                "ZhipuAI/AutoClaw/AutoClaw.exe",
            ],
        );
        let config_path = autoclaw_config_path(context);
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            config_path,
            probe_autoclaw_settings,
            true,
        )
    }

    fn source_protocol(
        &self,
        mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        openclaw_source_protocol(mode, upstream_protocol)
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        build_autoclaw_settings(detection, desired)
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        build_native_autoclaw_settings(detection, baseline)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        verify_autoclaw_settings(detection, desired)
    }
}

fn openclaw_source_protocol(mode: AgentBindingMode, upstream: ApiProtocol) -> ApiProtocol {
    match mode {
        AgentBindingMode::Direct => upstream,
        AgentBindingMode::Proxy => ApiProtocol::OpenaiChatCompletions,
    }
}

fn autoclaw_config_path(context: &DiscoveryContext) -> PathBuf {
    // 1. Scan application_data_dir case-insensitively for any "autoclaw" folder
    if let Ok(entries) = fs::read_dir(&context.application_data_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let name = entry.file_name();
                    if name.to_string_lossy().eq_ignore_ascii_case("autoclaw") {
                        let candidate = entry.path().join("settings.json");
                        if candidate.exists() {
                            return candidate;
                        }
                    }
                }
            }
        }
    }

    // 2. Check ~/.autoclaw/settings.json
    let home_autoclaw = context.home.join(".autoclaw/settings.json");
    if home_autoclaw.exists() {
        return home_autoclaw;
    }

    // 3. Fall back to official Electron default "AutoClaw/settings.json"
    context.application_data_dir.join("AutoClaw/settings.json")
}

fn qclaw_runtime_config_path(state_dir: &std::path::Path) -> Option<PathBuf> {
    let value: Value =
        serde_json::from_slice(&fs::read(state_dir.join("qclaw.json")).ok()?).ok()?;
    value
        .get("configPath")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                state_dir.join(path)
            }
        })
}

fn probe_openclaw(path: &PathBuf) -> AppResult<()> {
    let value: Value = serde_json::from_slice(&fs::read(path)?).map_err(|_| {
        CommandError::new(
            "agent_config_unparseable",
            "OpenClaw 配置不是有效的严格 JSON",
        )
        .with_recovery("请先用对应 Agent 的设置页保存一次配置，再刷新状态。")
    })?;
    if !value.is_object() {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "OpenClaw 配置根节点不是对象",
        ));
    }
    if value
        .pointer("/models/providers")
        .is_some_and(|providers| !providers.is_object())
    {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "OpenClaw 的 models.providers 字段类型不受支持",
        ));
    }
    Ok(())
}

/// AutoClaw treats its Electron `settings.json` model catalog as the source of
/// truth and regenerates `openclaw.json` during startup. Writing only the
/// generated OpenClaw file therefore appears to succeed, but AutoClaw removes
/// the provider immediately after relaunch. AT-Switch manages the authoritative
/// catalog instead, so the same behavior works on macOS and Windows.
fn probe_autoclaw_settings(path: &PathBuf) -> AppResult<()> {
    let value: Value = serde_json::from_slice(&fs::read(path)?).map_err(|_| {
        CommandError::new(
            "agent_config_unparseable",
            "AutoClaw settings.json 不是有效的严格 JSON",
        )
        .with_recovery("请先用 AutoClaw 的设置页保存一次配置，再刷新状态。")
    })?;
    if !value.is_object() {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "AutoClaw settings.json 根节点不是对象",
        ));
    }
    if value
        .pointer("/models/catalog")
        .is_some_and(|catalog| !catalog.is_array())
    {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "AutoClaw 的 models.catalog 字段类型不受支持",
        ));
    }
    Ok(())
}

fn build_autoclaw_settings(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<Vec<u8>> {
    let mut root = read_json_object(detection, "AutoClaw settings.json")?;
    let models = nested_object(&mut root, "models")?;
    let catalog = models
        .entry("catalog".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            CommandError::new(
                "agent_config_shape_unsupported",
                "AutoClaw 的 models.catalog 字段不是数组",
            )
        })?;

    catalog.retain(|entry| !is_at_switch_autoclaw_model(entry));
    let model_entry = autoclaw_model_entry(desired);
    // 把 managed 条目放到 catalog 最前面。AutoClaw 启动后从 settings.json.catalog
    // 重建 openclaw.json，primary 通常取 catalog 的第一个条目；managed 条目在首
    // 位才能确保重建后的 openclaw.json 默认模型是我们刚设置的 managed 模型。
    catalog.insert(0, model_entry.clone());
    models.insert("primary".to_owned(), model_entry);

    serialize_json_object(root, "无法生成 AutoClaw 设置")
}

fn build_native_autoclaw_settings(
    detection: &AgentDetection,
    baseline: &BaselineSnapshot,
) -> AppResult<Vec<u8>> {
    let mut current = read_json_object(detection, "AutoClaw settings.json")?;
    let baseline_value = if baseline.existed && !baseline.content.is_empty() {
        serde_json::from_slice::<Value>(&baseline.content).map_err(|_| {
            CommandError::new("baseline_payload_invalid", "AutoClaw 原始设置备份无法解析")
        })?
    } else {
        json!({})
    };
    let models = nested_object(&mut current, "models")?;
    if let Some(catalog) = models.get_mut("catalog").and_then(Value::as_array_mut) {
        catalog.retain(|entry| !is_at_switch_autoclaw_model(entry));
    }
    if let Some(primary) = baseline_value.pointer("/models/primary").cloned() {
        models.insert("primary".to_owned(), primary);
    } else {
        models.remove("primary");
    }

    serialize_json_object(current, "无法生成 AutoClaw 原始设置")
}

fn verify_autoclaw_settings(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<()> {
    let root = Value::Object(read_json_object(detection, "AutoClaw settings.json")?);
    let primary = root.pointer("/models/primary");
    let primary_matches = primary
        .and_then(|value| value.get("provider"))
        .and_then(Value::as_str)
        == Some(desired.provider_name)
        && primary
            .and_then(|value| value.get("model"))
            .and_then(Value::as_str)
            == Some(desired.model_id)
        && primary
            .and_then(|value| value.get("alias"))
            .and_then(Value::as_str)
            == Some(desired.model_id)
        && primary
            .and_then(|value| value.get("configId"))
            .and_then(Value::as_str)
            == Some(AUTOCLAW_CONFIG_ID)
        && primary
            .and_then(|value| value.get("baseUrl"))
            .and_then(Value::as_str)
            == Some(desired.base_url.trim_end_matches('/'))
        && primary
            .and_then(|value| value.get("api"))
            .and_then(Value::as_str)
            == Some(openclaw_api_name(desired.source_protocol));
    let catalog_matches = root
        .pointer("/models/catalog")
        .and_then(Value::as_array)
        .is_some_and(|catalog| {
            catalog.iter().any(|entry| {
                is_at_switch_autoclaw_model(entry)
                    && entry.get("provider").and_then(Value::as_str) == Some(desired.provider_name)
                    && entry.get("model").and_then(Value::as_str) == Some(desired.model_id)
                    && entry.get("alias").and_then(Value::as_str) == Some(desired.model_id)
                    && entry.get("configId").and_then(Value::as_str) == Some(AUTOCLAW_CONFIG_ID)
                    && entry
                        .get("apiKey")
                        .and_then(Value::as_str)
                        .is_some_and(|api_key| !api_key.trim().is_empty())
            })
        });
    if primary_matches && catalog_matches {
        Ok(())
    } else {
        Err(CommandError::new(
            "agent_config_not_applied",
            "AutoClaw 未读取到目标 AT-Switch 自定义模型配置",
        ))
    }
}

fn autoclaw_model_entry(desired: &DesiredAgentBinding<'_>) -> Value {
    Value::Object(Map::from_iter([
        (
            "provider".to_owned(),
            Value::String(desired.provider_name.to_owned()),
        ),
        (
            "configId".to_owned(),
            Value::String(AUTOCLAW_CONFIG_ID.to_owned()),
        ),
        (
            "model".to_owned(),
            Value::String(desired.model_id.to_owned()),
        ),
        (
            "alias".to_owned(),
            Value::String(desired.model_id.to_owned()),
        ),
        (
            "api".to_owned(),
            Value::String(openclaw_api_name(desired.source_protocol).to_owned()),
        ),
        (
            "baseUrl".to_owned(),
            Value::String(desired.base_url.trim_end_matches('/').to_owned()),
        ),
        ("isCustom".to_owned(), Value::Bool(true)),
        ("reasoning".to_owned(), Value::Bool(false)),
        ("contextWindow".to_owned(), Value::Number(200_000.into())),
        ("maxTokens".to_owned(), Value::Number(32_000.into())),
        (
            "apiKey".to_owned(),
            Value::String(desired.credential.to_owned()),
        ),
    ]))
}

fn is_at_switch_autoclaw_model(value: &Value) -> bool {
    value.get("provider").and_then(Value::as_str) == Some(AUTOCLAW_PROVIDER_ID)
        || value.get("configId").and_then(Value::as_str) == Some(AUTOCLAW_CONFIG_ID)
}

fn read_json_object(
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<Map<String, Value>> {
    let path = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new(
            "agent_config_path_missing",
            format!("未找到 {display_name} 路径"),
        )
    })?;
    let value = if path.exists() {
        serde_json::from_slice::<Value>(&fs::read(path)?).map_err(|_| {
            CommandError::new(
                "agent_config_unparseable",
                format!("{display_name} 无法解析"),
            )
        })?
    } else {
        json!({})
    };
    value.as_object().cloned().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            format!("{display_name} 根节点不是对象"),
        )
    })
}

fn serialize_json_object(root: Map<String, Value>, message: &str) -> AppResult<Vec<u8>> {
    serde_json::to_vec_pretty(&Value::Object(root))
        .map_err(|_| CommandError::internal(message))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn build_openclaw_config(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<Vec<u8>> {
    let path = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new("agent_config_path_missing", "未找到 OpenClaw 配置路径")
    })?;
    let mut root = if path.exists() {
        serde_json::from_slice::<Value>(&fs::read(path)?)
            .map_err(|_| CommandError::new("agent_config_unparseable", "OpenClaw 配置无法解析"))?
    } else {
        json!({})
    };
    let object = root.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "OpenClaw 配置根节点不是对象",
        )
    })?;

    let provider_id = desired.provider_name;
    let provider = json!({
        "baseUrl": desired.base_url.trim_end_matches('/'),
        "apiKey": desired.credential,
        "api": openclaw_api_name(desired.source_protocol),
        "models": [{
            "id": desired.model_id,
            "name": desired.model_id
        }]
    });
    nested_object(object, "models")?
        .entry("mode")
        .or_insert_with(|| Value::String("merge".to_owned()));
    let providers = nested_object(nested_object(object, "models")?, "providers")?;
    providers.retain(|key, _| key != "at-switch" && !key.starts_with("at-switch"));
    providers.insert(provider_id.to_owned(), provider);

    let agents = nested_object(object, "agents")?;
    let defaults = nested_object(agents, "defaults")?;
    let model = nested_object(defaults, "model")?;
    model.insert(
        "primary".to_owned(),
        Value::String(format!("{provider_id}/{}", desired.model_id)),
    );

    serde_json::to_vec_pretty(&root)
        .map_err(|_| CommandError::internal("无法生成 OpenClaw 配置"))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn build_native_openclaw_config(
    detection: &AgentDetection,
    baseline: &BaselineSnapshot,
) -> AppResult<Vec<u8>> {
    let path = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new("agent_config_path_missing", "未找到 OpenClaw 配置路径")
    })?;
    let mut current = if path.exists() {
        serde_json::from_slice::<Value>(&fs::read(path)?)
            .map_err(|_| CommandError::new("agent_config_unparseable", "OpenClaw 配置无法解析"))?
    } else {
        json!({})
    };
    let baseline_value = if baseline.existed && !baseline.content.is_empty() {
        serde_json::from_slice::<Value>(&baseline.content).map_err(|_| {
            CommandError::new("baseline_payload_invalid", "OpenClaw 原始配置备份无法解析")
        })?
    } else {
        json!({})
    };
    let object = current.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            "OpenClaw 配置根节点不是对象",
        )
    })?;

    if let Some(providers) = object
        .get_mut("models")
        .and_then(Value::as_object_mut)
        .and_then(|models| models.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        providers.retain(|key, _| key != "at-switch" && !key.starts_with("at-switch"));
    }

    let original_primary = baseline_value
        .pointer("/agents/defaults/model/primary")
        .cloned();
    if let Some(primary) = original_primary {
        nested_object(
            nested_object(nested_object(object, "agents")?, "defaults")?,
            "model",
        )?
        .insert("primary".to_owned(), primary);
    } else if let Some(model) = object
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|agents| agents.get_mut("defaults"))
        .and_then(Value::as_object_mut)
        .and_then(|defaults| defaults.get_mut("model"))
        .and_then(Value::as_object_mut)
    {
        model.remove("primary");
    }

    serde_json::to_vec_pretty(&current)
        .map_err(|_| CommandError::internal("无法生成 OpenClaw 原始配置"))
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
}

fn verify_openclaw_config(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<()> {
    let path = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new("agent_config_path_missing", "未找到 OpenClaw 配置路径")
    })?;
    let value: Value = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| CommandError::new("agent_config_unparseable", "OpenClaw 配置无法解析"))?;
    let primary = format!("{}/{}", desired.provider_name, desired.model_id);
    let legacy_primary = format!("at-switch/{}", desired.model_id);
    let provider = value
        .pointer(&format!("/models/providers/{}", desired.provider_name))
        .or_else(|| value.pointer("/models/providers/at-switch"));
    let applied = (value
        .pointer("/agents/defaults/model/primary")
        .and_then(Value::as_str)
        == Some(primary.as_str())
        || value
            .pointer("/agents/defaults/model/primary")
            .and_then(Value::as_str)
            == Some(legacy_primary.as_str()))
        && provider
            .and_then(|value| value.get("baseUrl"))
            .and_then(Value::as_str)
            == Some(desired.base_url.trim_end_matches('/'))
        && provider
            .and_then(|value| value.get("models"))
            .and_then(Value::as_array)
            .is_some_and(|models| {
                models
                    .iter()
                    .any(|model| model.get("id").and_then(Value::as_str) == Some(desired.model_id))
            });
    if applied {
        Ok(())
    } else {
        Err(CommandError::new(
            "agent_config_not_applied",
            "OpenClaw 未读取到目标 AT-Switch 路由",
        ))
    }
}

fn nested_object<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> AppResult<&'a mut Map<String, Value>> {
    let value = parent
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "agent_config_shape_unsupported",
            format!("OpenClaw 的 `{key}` 字段不是对象"),
        )
    })
}

fn openclaw_api_name(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => "openai-completions",
        ApiProtocol::OpenaiResponses => "openai-responses",
        ApiProtocol::AnthropicMessages => "anthropic-messages",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use crate::agents::locator::DiscoveryContext;
    use crate::agents::locator::Installation;
    use crate::domain::{AgentConfigHealth, AgentInstallStatus};

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

    #[test]
    fn openclaw_update_preserves_unmanaged_configuration() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("openclaw.json");
        fs::write(
            &path,
            br#"{
              "plugins":{"entries":{"user-plugin":{"enabled":true}}},
              "models":{"providers":{"existing":{"baseUrl":"https://example.test"}}}
            }"#,
        )
        .expect("seed");
        let detection = AgentDetection {
            id: "qclaw",
            display_name: "QClaw",
            installation: Some(Installation {
                path: temp.path().join("QClaw.app"),
                version: Some("1.0.0".to_owned()),
                kind: crate::agents::locator::InstallationKind::DesktopApp,
            }),
            config_path: Some(path),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: false,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };
        let output = build_openclaw_config(&detection, &desired()).expect("config");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            root.pointer("/plugins/entries/user-plugin/enabled"),
            Some(&Value::Bool(true))
        );
        assert!(root.pointer("/models/providers/existing").is_some());
        assert_eq!(
            root.pointer("/models/providers/蒙云智算/api"),
            Some(&Value::String("openai-completions".to_owned()))
        );
        assert_eq!(
            root.pointer("/agents/defaults/model/primary"),
            Some(&Value::String("蒙云智算/glm-test".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/providers/蒙云智算/models/0/name"),
            Some(&Value::String("glm-test".to_owned()))
        );
    }

    #[test]
    fn qclaw_honors_the_runtime_config_path() {
        let temp = tempfile::tempdir().expect("temp");
        let selected = temp.path().join("custom/openclaw.json");
        fs::write(
            temp.path().join("qclaw.json"),
            serde_json::to_vec(&json!({"configPath": selected})).expect("json"),
        )
        .expect("runtime");
        assert_eq!(qclaw_runtime_config_path(temp.path()), Some(selected));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn qclaw_switches_with_the_same_safe_restart_flow_on_desktop() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let state_dir = home.join(".qclaw");
        fs::create_dir_all(&state_dir).expect("state");
        fs::write(state_dir.join("openclaw.json"), b"{}").expect("config");

        #[cfg(target_os = "macos")]
        let application_dirs = {
            let root = temp.path().join("Applications");
            fs::create_dir_all(root.join("QClaw.app")).expect("app");
            vec![root]
        };
        #[cfg(target_os = "windows")]
        let (application_dirs, local_app_data) = {
            let root = temp.path().join("LocalAppData");
            let executable = root.join("Programs/QClaw/QClaw.exe");
            fs::create_dir_all(executable.parent().expect("parent")).expect("app dir");
            fs::write(executable, b"exe").expect("app");
            (Vec::new(), Some(root))
        };

        let context = DiscoveryContext {
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
        };
        let detection = QClawAdapter.detect(&context);
        assert_eq!(detection.install_status, AgentInstallStatus::Installed);
        assert!(detection.needs_restart);
    }

    #[test]
    fn autoclaw_updates_the_authoritative_model_catalog_and_restores_native_model() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("settings.json");
        let native = br#"{
          "appearance":{"theme":"system"},
          "models":{
            "primary":{"provider":"zhipu","model":"zai_auto","alias":"Auto"},
            "catalog":[{"provider":"zhipu","model":"zai_auto","alias":"Auto"}]
          }
        }"#;
        fs::write(&path, native).expect("seed");
        let detection = AgentDetection {
            id: "autoclaw",
            display_name: "AutoClaw",
            installation: Some(Installation {
                path: temp.path().join("AutoClaw.app"),
                version: Some("1.14.2".to_owned()),
                kind: crate::agents::locator::InstallationKind::DesktopApp,
            }),
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

        let output = build_autoclaw_settings(&detection, &desired()).expect("settings");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            root.pointer("/models/primary/provider"),
            Some(&Value::String("蒙云智算".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/primary/model"),
            Some(&Value::String("glm-test".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/primary/alias"),
            Some(&Value::String("glm-test".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/primary/apiKey"),
            Some(&Value::String("local-token".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/primary/configId"),
            Some(&Value::String(AUTOCLAW_CONFIG_ID.to_owned()))
        );
        let catalog = root
            .pointer("/models/catalog")
            .and_then(Value::as_array)
            .expect("catalog");
        assert_eq!(catalog.len(), 2);
        // AutoClaw 启动后从 settings.json.catalog 重建 openclaw.json，
        // 默认模型取 catalog 第一个条目；managed 条目必须位于首位才能保证
        // 重建后的默认模型是我们刚切换的目标。
        assert!(
            is_at_switch_autoclaw_model(&catalog[0]),
            "managed entry must be inserted at catalog[0]"
        );
        let managed = &catalog[0];
        assert_eq!(
            managed.get("apiKey"),
            Some(&Value::String("local-token".to_owned()))
        );
        assert_eq!(
            managed.get("configId"),
            Some(&Value::String(AUTOCLAW_CONFIG_ID.to_owned()))
        );
        assert_eq!(
            root.pointer("/appearance/theme"),
            Some(&Value::String("system".to_owned()))
        );

        fs::write(&path, &output).expect("apply");
        verify_autoclaw_settings(&detection, &desired()).expect("verify");
        let restored = build_native_autoclaw_settings(
            &detection,
            &BaselineSnapshot {
                existed: true,
                content: native.to_vec(),
            },
        )
        .expect("restore");
        let restored: Value = serde_json::from_slice(&restored).expect("restored json");
        assert_eq!(
            restored.pointer("/models/primary/model"),
            Some(&Value::String("zai_auto".to_owned()))
        );
        assert!(!restored
            .pointer("/models/catalog")
            .and_then(Value::as_array)
            .expect("catalog")
            .iter()
            .any(is_at_switch_autoclaw_model));
    }

    #[test]
    fn qclaw_replaces_the_previous_at_switch_model_and_verifies_the_new_one() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("openclaw.json");
        fs::write(&path, b"{}\n").expect("seed");
        let detection = AgentDetection {
            id: "qclaw",
            display_name: "QClaw",
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

        let first = desired();
        fs::write(
            &path,
            build_openclaw_config(&detection, &first).expect("first config"),
        )
        .expect("write first");
        verify_openclaw_config(&detection, &first).expect("verify first");

        let second = DesiredAgentBinding {
            model_id: "glm-next",
            ..desired()
        };
        fs::write(
            &path,
            build_openclaw_config(&detection, &second).expect("second config"),
        )
        .expect("write second");
        verify_openclaw_config(&detection, &second).expect("verify second");

        let root: Value = serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
        assert_eq!(
            root.pointer("/agents/defaults/model/primary"),
            Some(&Value::String("蒙云智算/glm-next".to_owned()))
        );
        let models = root
            .pointer("/models/providers/蒙云智算/models")
            .and_then(Value::as_array)
            .expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "glm-next");
    }

    #[test]
    fn autoclaw_replaces_the_previous_managed_catalog_entry_and_survives_encrypted_keys() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("settings.json");
        fs::write(
            &path,
            br#"{"models":{"catalog":[{"provider":"zhipu","model":"zai_auto"}]}}"#,
        )
        .expect("seed");
        let detection = AgentDetection {
            id: "autoclaw",
            display_name: "AutoClaw",
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
            build_autoclaw_settings(&detection, &desired()).expect("first config"),
        )
        .expect("write first");
        let second = DesiredAgentBinding {
            model_id: "glm-next",
            ..desired()
        };
        let next = build_autoclaw_settings(&detection, &second).expect("second config");
        let mut root: Value = serde_json::from_slice(&next).expect("json");
        // AutoClaw encrypts the catalog credential after its first read. The
        // adapter validates presence, not ciphertext representation.
        // managed 条目位于 catalog[0]（insert(0, ...) 确保首位）。
        root.pointer_mut("/models/catalog/0/apiKey")
            .expect("managed catalog key")
            .clone_from(&Value::String("enc:test-ciphertext".to_owned()));
        root.pointer_mut("/models/primary/apiKey")
            .expect("primary key")
            .clone_from(&Value::String("enc:test-ciphertext".to_owned()));
        fs::write(&path, serde_json::to_vec_pretty(&root).expect("serialize"))
            .expect("write normalized");

        verify_autoclaw_settings(&detection, &second).expect("verify second");
        let catalog = root
            .pointer("/models/catalog")
            .and_then(Value::as_array)
            .expect("catalog");
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| is_at_switch_autoclaw_model(entry))
                .count(),
            1
        );
        assert_eq!(
            root.pointer("/models/primary/model"),
            Some(&Value::String("glm-next".to_owned()))
        );
    }
}
