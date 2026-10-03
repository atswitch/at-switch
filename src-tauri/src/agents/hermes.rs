use std::{env, fs, path::Path, path::PathBuf};

use serde_yaml::Value;

use super::{
    locator::{
        locate_command, locate_desktop_app, DiscoveryContext, DiscoveryHints, Installation,
        InstallationKind,
    },
    AgentAdapter, AgentDetection, BaselineSnapshot, DesiredAgentBinding,
};
use crate::domain::{ApiProtocol, AppResult, CommandError};

const DISPLAY_NAME: &str = "Hermes Agent";
const CONFIG_FILE: &str = "config.yaml";
const MANAGED_PROVIDER: &str = "at-switch";
const HERMES_API_KEY_ENV: &str = "HERMES_AT_SWITCH_API_KEY";

pub struct HermesAdapter;

impl HermesAdapter {
    fn detect_command(context: &DiscoveryContext) -> Option<Installation> {
        let mut candidates = Vec::new();
        for name in ["hermes", "hermes.exe", "hermes.cmd"] {
            candidates.push(context.home.join(".local/bin").join(name));
            candidates.push(context.home.join(".cargo/bin").join(name));
            candidates.push(context.home.join(".npm/bin").join(name));
            #[cfg(target_os = "windows")]
            candidates.push(context.home.join("AppData/Roaming/npm").join(name));
        }
        candidates
            .into_iter()
            .find(|path| path.is_file() || path.is_symlink())
            .map(|path| Installation {
                version: hermes_package_version(&path),
                path,
                kind: InstallationKind::Command,
            })
            .or_else(|| locate_command(context, &["hermes", "hermes.exe", "hermes.cmd"]))
    }
}

impl AgentAdapter for HermesAdapter {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["com.hermes.agent"],
            windows_relative_paths: &[
                "Programs/Hermes/Hermes.exe",
                "Hermes/Hermes.exe",
                "Hermes.exe",
            ],
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = Self::detect_command(context)
            .or_else(|| {
                locate_desktop_app(
                    context,
                    &["Hermes.app", "Hermes.exe", "Hermes"],
                    &["com.hermes.agent"],
                    &[
                        "Programs/Hermes/Hermes.exe",
                        "Hermes/Hermes.exe",
                        "Hermes.exe",
                    ],
                )
            })
            .or_else(|| {
                config_directory(context).map(|directory| Installation {
                    version: None,
                    path: directory,
                    kind: InstallationKind::Command,
                })
            });

        let config_path = config_directory(context)
            .map(|directory| directory.join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE));
        AgentDetection::from_file_probe(
            self.id(),
            DISPLAY_NAME,
            installation,
            config_path,
            probe_hermes,
            false,
        )
    }

    fn source_protocol(
        &self,
        desired_mode: super::AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        match desired_mode {
            super::AgentBindingMode::Direct => upstream_protocol,
            super::AgentBindingMode::Proxy => ApiProtocol::OpenaiChatCompletions,
        }
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        let path = config_path(detection)?;
        let original = fs::read_to_string(path).unwrap_or_default();
        read_config_mapping(path)?;
        let updated = update_hermes_config(&original, desired)?;
        Ok(updated.into_bytes())
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        if baseline.existed {
            return Ok(baseline.content.clone());
        }
        // No baseline means the user has never been managed by AT-Switch. We
        // never wrote to their disk, so the disk is already in its factory
        // state (or the file simply doesn't exist yet). Returning empty bytes
        // lets Hermes fall back to its built-in default model/provider.
        let _ = detection;
        Ok(Vec::new())
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        let mapping = read_config_mapping(config_path(detection)?)?;
        let expected_provider = expected_top_level_provider(desired.model_id);
        let top_level_model = mapping.get("model").and_then(Value::as_str);
        let top_level_provider = mapping.get("provider").and_then(Value::as_str);
        if top_level_model != Some(desired.model_id)
            || top_level_provider != Some(expected_provider.as_str())
        {
            return Err(applied_error());
        }

        let provider = managed_provider(&mapping).ok_or_else(applied_error)?;
        let transport = provider.get("transport").and_then(Value::as_str);
        let default_model = provider.get("default_model").and_then(Value::as_str);
        let key_env = provider.get("key_env").and_then(Value::as_str);
        let base_url = provider
            .get("api")
            .or_else(|| provider.get("base_url"))
            .and_then(Value::as_str);

        if transport != Some(transport_name(desired.source_protocol))
            || default_model != Some(desired.model_id)
            || key_env != Some(HERMES_API_KEY_ENV)
            || base_url != Some(desired.base_url.trim_end_matches('/'))
        {
            return Err(applied_error());
        }
        if provider.contains_key("api_key") || provider.contains_key("key_cmd") {
            return Err(applied_error());
        }
        Ok(())
    }
}

fn config_directory(context: &DiscoveryContext) -> Option<PathBuf> {
    if let Ok(home) = env::var("HERMES_HOME") {
        let directory = PathBuf::from(home);
        if directory.is_absolute() {
            return Some(directory);
        }
    }
    Some(context.home.join(".hermes"))
}

fn config_path(detection: &AgentDetection) -> AppResult<&Path> {
    detection
        .config_path
        .as_deref()
        .ok_or_else(|| CommandError::new("agent_config_path_missing", "Hermes 配置路径不可用"))
}

#[allow(clippy::ptr_arg)] // ConfigProbe is shared with the existing adapters.
fn probe_hermes(path: &PathBuf) -> AppResult<()> {
    let mapping = read_config_mapping(path)?;
    if let Some(provider) = mapping.get("provider") {
        let Some(provider) = provider.as_str() else {
            return Err(shape_error());
        };
        if let Some(name) = named_custom_provider_key(provider) {
            let providers = mapping
                .get("providers")
                .and_then(Value::as_mapping)
                .ok_or_else(shape_error)?;
            providers
                .get(name)
                .and_then(Value::as_mapping)
                .ok_or_else(shape_error)?;
        }
    }
    if let Some(providers) = mapping.get("providers") {
        if !providers.is_mapping() {
            return Err(shape_error());
        }
    }
    Ok(())
}

fn read_config_mapping(path: &Path) -> AppResult<serde_yaml::Mapping> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(serde_yaml::Mapping::new());
    };
    let value: Value = serde_yaml::from_slice(&bytes).map_err(|_| unparseable_error())?;
    if value.is_null() {
        return Ok(serde_yaml::Mapping::new());
    }
    value.as_mapping().cloned().ok_or_else(shape_error)
}

fn transport_name(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => "chat_completions",
        ApiProtocol::OpenaiResponses => "codex_responses",
        ApiProtocol::AnthropicMessages => "anthropic_messages",
    }
}

fn expected_top_level_provider(model_id: &str) -> String {
    format!("custom:{MANAGED_PROVIDER}:{model_id}")
}

fn named_custom_provider_key(provider: &str) -> Option<&str> {
    provider.split(':').find_map(|part| {
        part.strip_prefix("custom:")
            .and_then(|rest| rest.split(':').next())
    })
}

fn managed_provider(mapping: &serde_yaml::Mapping) -> Option<&serde_yaml::Mapping> {
    mapping
        .get("providers")
        .and_then(Value::as_mapping)
        .and_then(|providers| providers.get(MANAGED_PROVIDER))
        .and_then(Value::as_mapping)
}

fn applied_error() -> CommandError {
    CommandError::new("agent_config_not_applied", "Hermes 配置与目标模型不一致")
}

fn shape_error() -> CommandError {
    CommandError::new(
        "agent_config_shape_unsupported",
        "Hermes 配置不是受支持的 YAML 映射",
    )
}

fn unparseable_error() -> CommandError {
    CommandError::new("agent_config_unparseable", "Hermes 配置不是有效 YAML")
}

fn update_hermes_config(original: &str, desired: &DesiredAgentBinding<'_>) -> AppResult<String> {
    let ends_with_newline = original.ends_with('\n') || original.is_empty();
    let mut lines = original.lines().map(ToOwned::to_owned).collect::<Vec<_>>();

    replace_or_append_top_level(&mut lines, "model", &yaml_scalar(desired.model_id));
    replace_or_append_top_level(
        &mut lines,
        "provider",
        &yaml_scalar(&expected_top_level_provider(desired.model_id)),
    );

    let lines = with_managed_provider(&lines, desired)?;
    validate_lines(&lines.join("\n"))?;

    let mut rendered = lines.join("\n");
    if ends_with_newline && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn yaml_scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn replace_or_append_top_level(lines: &mut Vec<String>, key: &str, value: &str) {
    if let Some(index) = find_top_level_key(lines, key) {
        let indent = leading_spaces(&lines[index]);
        let trimmed = lines[index].trim_start();
        let rest = trimmed[key.len() + 1..].trim_start();
        let comment = comment_suffix(rest).map(|value| format!(" {value}"));
        lines[index] = format!("{}{key}: {value}", " ".repeat(indent));
        if let Some(comment) = comment {
            lines[index].push_str(&comment);
        }
        return;
    }

    lines.push(format!("{key}: {value}"));
}

fn with_managed_provider(
    lines: &[String],
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<Vec<String>> {
    let Some(providers_index) = find_top_level_key(lines, "providers") else {
        let mut updated = lines.to_owned();
        updated.push("providers:".to_owned());
        append_provider_block(&mut updated, 2, desired);
        return Ok(updated);
    };

    let existing = lines[providers_index].trim_start();
    let value = existing["providers".len() + 1..].trim_start();
    if !value.is_empty() && !value.starts_with('#') {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "Hermes 的 providers 字段为内联映射，请改为块状 YAML 后重试",
        ));
    }

    let child_indent = child_indent(lines, providers_index);
    let mut updated = lines.to_owned();
    if let Some(managed_index) =
        find_provider_entry(&updated, providers_index, child_indent, MANAGED_PROVIDER)
    {
        let end = provider_entry_end(&updated, managed_index, child_indent, providers_index);
        updated.drain(managed_index..end);
    }

    let insertion = providers_section_end(&updated, providers_index);
    let mut block = Vec::new();
    append_provider_block(&mut block, child_indent, desired);
    updated.splice(insertion..insertion, block);
    Ok(updated)
}

fn append_provider_block(
    lines: &mut Vec<String>,
    indent: usize,
    desired: &DesiredAgentBinding<'_>,
) {
    let child = " ".repeat(indent);
    let field = " ".repeat(indent + 2);
    lines.push(format!("{child}{MANAGED_PROVIDER}:"));
    lines.push(format!(
        "{field}api: {}",
        yaml_scalar(desired.base_url.trim_end_matches('/'))
    ));
    lines.push(format!("{field}key_env: {HERMES_API_KEY_ENV}"));
    lines.push(format!(
        "{field}transport: {}",
        transport_name(desired.source_protocol)
    ));
    lines.push(format!(
        "{field}default_model: {}",
        yaml_scalar(desired.model_id)
    ));
}

fn validate_lines(content: &str) -> AppResult<()> {
    let value: Value = serde_yaml::from_str(content).map_err(|_| unparseable_error())?;
    if !value.is_null() && !value.is_mapping() {
        return Err(shape_error());
    }
    Ok(())
}

fn find_top_level_key(lines: &[String], key: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| top_level_key(line) == Some(key))
}

fn top_level_key(line: &str) -> Option<&str> {
    if leading_spaces(line) != 0 || line.trim_start().starts_with('#') {
        return None;
    }
    let trimmed = line.trim_start();
    let end = trimmed.find(':')?;
    normalize_key(&trimmed[..end])
}

fn normalize_key(key: &str) -> Option<&str> {
    let key = key.trim();
    let quoted = (key.starts_with('"') && key.ends_with('"') && key.len() >= 2)
        || (key.starts_with('\'') && key.ends_with('\'') && key.len() >= 2);
    let key = if quoted { &key[1..key.len() - 1] } else { key };
    if key.is_empty()
        || key.chars().any(|character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
    {
        return None;
    }
    Some(key)
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn comment_suffix(rest: &str) -> Option<&str> {
    let mut single = false;
    let mut double = false;
    for (index, character) in rest.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double && (index == 0 || rest[..index].ends_with([' ', '\t'])) => {
                return Some(&rest[index..]);
            }
            _ => {}
        }
    }
    None
}

fn child_indent(lines: &[String], providers_index: usize) -> usize {
    (providers_index + 1..lines.len())
        .map(|index| {
            let line = &lines[index];
            let indent = leading_spaces(line);
            let trimmed = line.trim_start();
            (indent, !trimmed.is_empty() && !trimmed.starts_with('#'))
        })
        .find(|(_, meaningful)| *meaningful)
        .map(|(indent, _)| indent)
        .unwrap_or(2)
}

fn find_provider_entry(
    lines: &[String],
    providers_index: usize,
    child_indent: usize,
    key: &str,
) -> Option<usize> {
    (providers_index + 1..lines.len()).find(|index| {
        let line = &lines[*index];
        let indent = leading_spaces(line);
        let trimmed = line.trim_start();
        indent == child_indent && !trimmed.starts_with('#') && entry_key(trimmed) == Some(key)
    })
}

fn entry_key(line: &str) -> Option<&str> {
    let end = line.find(':')?;
    normalize_key(&line[..end])
}

fn provider_entry_end(
    lines: &[String],
    start: usize,
    child_indent: usize,
    providers_index: usize,
) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        let line = &lines[index];
        let indent = leading_spaces(line);
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        if indent < child_indent {
            break;
        }
        if indent == child_indent && top_level_key(line).is_none() {
            break;
        }
        index += 1;
    }
    if index == start + 1 {
        return start + 1;
    }
    let _ = providers_index;
    index
}

fn providers_section_end(lines: &[String], providers_index: usize) -> usize {
    (providers_index + 1..lines.len())
        .find(|index| {
            leading_spaces(&lines[*index]) == 0 && !lines[*index].trim_start().starts_with('#')
        })
        .unwrap_or(lines.len())
}

fn hermes_package_version(path: &Path) -> Option<String> {
    let resolved = fs::canonicalize(path).ok()?;
    for ancestor in resolved.ancestors().take(8) {
        let package_path = ancestor.join("package.json");
        let Ok(bytes) = fs::read(&package_path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if value.get("name").and_then(|value| value.as_str()) == Some("hermes-agent") {
            return value
                .get("version")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
        }
    }
    None
}

#[cfg(test)]
#[path = "hermes_tests.rs"]
mod tests;
