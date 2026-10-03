use std::{fs, path::PathBuf};

use serde_json::{json, Map, Value};

use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    locator::{locate_desktop_app, DiscoveryContext, DiscoveryHints},
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

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["com.tencent.qclaw"],
            windows_relative_paths: &[
                "Programs/QClaw/QClaw.exe",
                "QClaw/QClaw.exe",
                "Tencent/QClaw/QClaw.exe",
            ],
        }
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

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["com.zhipuai.autoclaw"],
            windows_relative_paths: &[
                "Programs/AutoClaw/AutoClaw.exe",
                "AutoClaw/AutoClaw.exe",
                "ZhipuAI/AutoClaw/AutoClaw.exe",
            ],
        }
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

pub struct AionClawAdapter;

impl AgentAdapter for AionClawAdapter {
    fn id(&self) -> &'static str {
        "aionclaw"
    }

    fn display_name(&self) -> &'static str {
        "AionClaw"
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["com.quyuanai.aionclaw"],
            windows_relative_paths: &["Programs/AionClaw/AionClaw.exe", "AionClaw/AionClaw.exe"],
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["AionClaw.app"],
            &["com.quyuanai.aionclaw"],
            &["Programs/AionClaw/AionClaw.exe", "AionClaw/AionClaw.exe"],
        );
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            aionclaw_state_dir(context).join("openclaw.json"),
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

/// EasyClaw（猎豹移动，`ai.easyclawcn.desktop`）同样是 OpenClaw 发行版：应用内
/// `Resources/cfmind/gateway.asar/openclaw.mjs` 就是 OpenClaw 运行时，配置落在
/// `~/.easyclaw/easyclaw.json`，其中 `models.providers` 与
/// `agents.defaults.model.primary` 的结构与 QClaw / AionClaw 逐字一致，因此直接复用
/// 同一套读写实现。
///
/// 与 AutoClaw 不同，这里不需要再写第二处：`~/.easyclaw/easyclawcli` 里明确设置了
/// `EASYCLAW_CONFIG_DIR=/…/.easyclaw`，`easyclaw.json` 本身就是权威配置。它只在安装/
/// 升级时按 `.config-merge-signal.json` 合并一次默认值（并留一份
/// `.easyclaw-snapshots/*.bak`），不会在每次启动时重建。
pub struct EasyClawAdapter;

impl AgentAdapter for EasyClawAdapter {
    fn id(&self) -> &'static str {
        "easyclaw"
    }

    fn display_name(&self) -> &'static str {
        "EasyClaw"
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["ai.easyclawcn.desktop"],
            windows_relative_paths: &["Programs/EasyClaw/EasyClaw.exe", "EasyClaw/EasyClaw.exe"],
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["easyclaw.app"],
            &["ai.easyclawcn.desktop"],
            &["Programs/EasyClaw/EasyClaw.exe", "EasyClaw/EasyClaw.exe"],
        );
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            context.home.join(".easyclaw").join("easyclaw.json"),
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

/// AionClaw 把 OpenClaw 运行时放在 macOS 沙盒容器内，因此其 `openclaw.json`
/// 不在 `~/.aionclaw`，而在容器的 `AionClaw/openclaw/state/` 下；实测该路径
/// 对当前用户可读写，且结构（`models.providers` + `agents.defaults.model.primary`）
/// 与其它 OpenClaw 发行版完全一致，因此直接复用同一套读写实现。
fn aionclaw_state_dir(context: &DiscoveryContext) -> PathBuf {
    let container = context.home.join(
        "Library/Containers/com.quyuanai.aionclaw/Data/Library/Application Support/AionClaw/openclaw/state",
    );
    if container.join("openclaw.json").exists() {
        return container;
    }
    let portable = context.application_data_dir.join("AionClaw/openclaw/state");
    if portable.join("openclaw.json").exists() {
        return portable;
    }
    // 两者都不存在时仍返回沙盒路径，保证写入目标稳定、报错信息一致。
    container
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
    } else if let Some(primary) = models.get("primary").cloned() {
        // No baseline means AT-Switch never wrote anything to this Agent. The
        // current `models.primary` belongs to the user (or AutoClaw's first-run
        // default). Keep it so AutoClaw launches against its factory selection
        // instead of a cleared `primary`.
        models.insert("primary".to_owned(), primary);
    }
    // Else: `primary` is still absent. That's the genuine factory state where
    // AutoClaw derives its default model from the catalog.

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

    let providers_map = object
        .get_mut("models")
        .and_then(Value::as_object_mut)
        .and_then(|models| models.get_mut("providers"))
        .and_then(Value::as_object_mut);
    if let Some(providers) = providers_map {
        providers.retain(|key, _| key != "at-switch" && !key.starts_with("at-switch"));
        // Restore baseUrl / api for every provider that existed in the baseline so a
        // managed provider that pointed to the upstream endpoint does not leave the
        // user's pre-existing provider record pointing at the same upstream endpoint.
        if let Some(baseline_providers) = baseline_value
            .pointer("/models/providers")
            .and_then(Value::as_object)
        {
            for (key, baseline_entry) in baseline_providers {
                if key == "at-switch" || key.starts_with("at-switch") {
                    continue;
                }
                if let Some(entry) = providers.get_mut(key).and_then(Value::as_object_mut) {
                    if let Some(baseline_url) = baseline_entry.get("baseUrl").cloned() {
                        entry.insert("baseUrl".to_owned(), baseline_url);
                    }
                    if let Some(baseline_api) = baseline_entry.get("api").cloned() {
                        entry.insert("api".to_owned(), baseline_api);
                    }
                }
            }
        }
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
    } else {
        let current_primary = object
            .get("agents")
            .and_then(|agents| agents.get("defaults"))
            .and_then(|defaults| defaults.get("model"))
            .and_then(|model| model.get("primary"))
            .cloned();
        if let Some(primary) = current_primary {
            // No baseline means AT-Switch never managed this Agent. Keep the
            // current factory/default primary so OpenClaw starts with its own
            // selection instead of a blank model.
            nested_object(
                nested_object(nested_object(object, "agents")?, "defaults")?,
                "model",
            )?
            .insert("primary".to_owned(), primary);
        }
        // If current primary is also absent, leave the object untouched — that
        // is the genuine factory state where OpenClaw derives its default from
        // catalog.
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

    /// EasyClaw 复用同一套 OpenClaw 内核，但它自己的配置是**权威源**（`easyclaw.json`），
    /// 且带 `models.mode: "replace"` 与官方 provider，所以这里按真机形状起测：
    /// 官方那条必须原样保留，只新增我们的 provider 并把 primary 指过去。
    #[test]
    fn easyclaw_keeps_the_official_provider_and_switches_through_the_shared_kernel() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("easyclaw.json");
        fs::write(
            &path,
            br#"{
              "meta":{"lastTouchedVersion":"2026.4.14"},
              "models":{"mode":"replace","providers":{
                "easyclaw":{"baseUrl":"https://aibot-srv.easyclaw.cn","apiKey":"official","models":[
                  {"id":"deepseek.deepseek-v4-flash","api":"openai-completions"}
                ]}
              }},
              "agents":{"defaults":{"model":{"primary":"easyclaw/deepseek.deepseek-v4-flash"}}}
            }"#,
        )
        .expect("seed");
        let detection = AgentDetection {
            id: "easyclaw",
            display_name: "EasyClaw",
            installation: Some(Installation {
                path: temp.path().join("easyclaw.app"),
                version: Some("1.3.110".to_owned()),
                kind: crate::agents::locator::InstallationKind::DesktopApp,
            }),
            config_path: Some(path.clone()),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: false,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };

        let bytes = EasyClawAdapter
            .build_config(&detection, &desired())
            .expect("build config");
        fs::write(&path, bytes).expect("apply");

        EasyClawAdapter
            .verify_config(&detection, &desired())
            .expect("verification must pass right after the write");

        let root: Value =
            serde_json::from_slice(&fs::read(&path).expect("read")).expect("valid json");
        assert_eq!(
            root.pointer("/agents/defaults/model/primary"),
            Some(&Value::String("蒙云智算/glm-test".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/providers/蒙云智算/api"),
            Some(&Value::String("openai-completions".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/providers/easyclaw/apiKey"),
            Some(&Value::String("official".to_owned())),
            "the built-in easyclaw provider must survive untouched"
        );
        assert_eq!(
            root.pointer("/models/mode"),
            Some(&Value::String("replace".to_owned())),
            "the existing models.mode must not be rewritten"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn easyclaw_resolves_its_config_under_the_home_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let state_dir = home.join(".easyclaw");
        fs::create_dir_all(&state_dir).expect("state");
        fs::write(
            state_dir.join("easyclaw.json"),
            b"{\"models\":{\"providers\":{}}}",
        )
        .expect("config");
        let applications = temp.path().join("Applications");
        fs::create_dir_all(applications.join("easyclaw.app")).expect("app");

        let context = DiscoveryContext {
            home,
            application_data_dir: temp.path().join("ApplicationData"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            system_candidates: None,
        };
        let detection = EasyClawAdapter.detect(&context);

        assert_eq!(detection.id, "easyclaw");
        assert_eq!(detection.display_name, "EasyClaw");
        assert_eq!(
            detection.config_path,
            Some(state_dir.join("easyclaw.json")),
            "EasyClaw keeps its authoritative config in ~/.easyclaw"
        );
        assert!(detection.write_supported);
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
            system_candidates: None,
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

    #[test]
    fn openclaw_native_restore_rewrites_baseline_provider_endpoint() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("openclaw.json");
        // The user's native configuration pointed OpenClaw at the Z.ai endpoint.
        let native = br#"{
          "models": {
            "mode": "merge",
            "providers": {
              "zai": {
                "baseUrl": "https://api.z.ai/api/paas/v4",
                "apiKey": "user-key",
                "api": "openai-completions",
                "models": [{"id": "glm-4.7", "name": "glm-4.7"}]
              }
            }
          },
          "agents": {"defaults": {"model": {"primary": "zai/glm-4.7"}}}
        }"#;
        fs::write(&path, native).expect("seed");

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

        // Apply the managed switch: QClaw is now routed to a different upstream.
        let managed = desired();
        fs::write(
            &path,
            build_openclaw_config(&detection, &managed).expect("managed config"),
        )
        .expect("write managed");
        verify_openclaw_config(&detection, &managed).expect("verify managed");

        // Restore from the baseline; the zai provider must point back at the
        // endpoint it had before the switch, not at the managed upstream.
        let restored = build_native_openclaw_config(
            &detection,
            &BaselineSnapshot {
                existed: true,
                content: native.to_vec(),
            },
        )
        .expect("restore");
        fs::write(&path, &restored).expect("write restored");

        let root: Value = serde_json::from_slice(&restored).expect("json");
        assert_eq!(
            root.pointer("/agents/defaults/model/primary"),
            Some(&Value::String("zai/glm-4.7".to_owned()))
        );
        assert!(root
            .pointer("/models/providers")
            .and_then(Value::as_object)
            .is_some_and(|providers| !providers.contains_key("at-switch")));
        assert_eq!(
            root.pointer("/models/providers/zai/baseUrl"),
            Some(&Value::String("https://api.z.ai/api/paas/v4".to_owned()))
        );
        assert_eq!(
            root.pointer("/models/providers/zai/api"),
            Some(&Value::String("openai-completions".to_owned()))
        );
    }
}
