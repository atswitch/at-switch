use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::{
    domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError},
    services::BaselineSnapshot,
};

pub struct OpenCodeAdapter;

const MANAGED_PROVIDER: &str = "at-switch";
const MANAGED_NAME: &str = "AT-Switch";
const PROVIDER_SDK: &str = "@ai-sdk/openai-compatible";

impl AgentAdapter for OpenCodeAdapter {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["OpenCode.app"],
            &["ai.opencode.desktop"],
            &[
                "Programs/OpenCode/OpenCode.exe",
                "OpenCode/OpenCode.exe",
                "OpenCode.exe",
            ],
        );
        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            resolve_config_path(context),
            probe_config,
            true,
        )
    }

    fn source_protocol(
        &self,
        _mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode == AgentBindingMode::Direct
            && desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "opencode_direct_protocol_unsupported",
                "OpenCode 直连模式要求 Provider 支持 OpenAI Chat API",
            )
            .with_recovery("请改用本地代理模式，AT-Switch 会完成协议转换。"));
        }
        if desired.source_protocol != ApiProtocol::OpenaiChatCompletions {
            return Err(CommandError::new(
                "opencode_protocol_unsupported",
                "OpenCode 仅支持 OpenAI Chat 兼容入口",
            ));
        }
        Ok(())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        self.validate_binding(desired)?;
        let source = read_config_text(config_path(detection)?)?;
        let current = parse_config(source.as_bytes())?;
        let provider = provider_config(desired);
        let mut updated = source;
        if current.get("provider").is_none() {
            let initial = json!({ MANAGED_PROVIDER: provider });
            updated = set_jsonc_member(&updated, None, "provider", &initial)?;
        }
        updated = set_jsonc_member(&updated, Some("provider"), MANAGED_PROVIDER, &provider)?;
        updated = set_jsonc_member(&updated, None, "model", &json!(managed_model_id(desired)))?;
        parse_config(updated.as_bytes())?;
        Ok(updated.into_bytes())
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        let source = read_config_text(config_path(detection)?)?;
        let current = parse_config(source.as_bytes())?;
        let baseline = if baseline.existed {
            parse_config(&baseline.content)?
        } else {
            json!({})
        };
        let mut updated = source;

        if current
            .pointer("/provider/at-switch")
            .is_some_and(is_managed_provider)
        {
            let baseline_provider = baseline.pointer("/provider/at-switch");
            match baseline_provider {
                Some(original) if !is_managed_provider(original) => {
                    updated =
                        set_jsonc_member(&updated, Some("provider"), MANAGED_PROVIDER, original)?;
                }
                _ => {
                    updated = remove_jsonc_member(&updated, Some("provider"), MANAGED_PROVIDER)?;
                    if current
                        .get("provider")
                        .and_then(Value::as_object)
                        .is_some_and(|providers| providers.len() == 1)
                        && baseline.get("provider").is_none()
                    {
                        updated = remove_jsonc_member(&updated, None, "provider")?;
                    }
                }
            }
        }

        if current
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|model| model.starts_with(&format!("{MANAGED_PROVIDER}/")))
        {
            match baseline
                .get("model")
                .filter(|model| !model.as_str().is_some_and(is_managed_model_id))
            {
                Some(original) => {
                    updated = set_jsonc_member(&updated, None, "model", original)?;
                }
                None => {
                    updated = remove_jsonc_member(&updated, None, "model")?;
                }
            }
        }
        parse_config(updated.as_bytes())?;
        Ok(updated.into_bytes())
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        self.validate_binding(desired)?;
        let value = read_config(config_path(detection)?)?;
        let expected_model = format!("{MANAGED_PROVIDER}/{}", desired.model_id);
        let provider = value.get("provider").and_then(|v| v.get(MANAGED_PROVIDER));
        let applied = value.get("model").and_then(Value::as_str) == Some(expected_model.as_str())
            && provider.is_some_and(is_managed_provider)
            && provider.and_then(|p| p.get("npm")).and_then(Value::as_str) == Some(PROVIDER_SDK)
            && provider
                .and_then(|p| p.pointer("/options/baseURL"))
                .and_then(Value::as_str)
                == Some(desired.base_url.trim_end_matches('/'))
            && provider
                .and_then(|p| p.pointer("/options/apiKey"))
                .and_then(Value::as_str)
                == Some(desired.credential)
            && provider
                .and_then(|p| p.get("models"))
                .and_then(Value::as_object)
                .is_some_and(|models| models.contains_key(desired.model_id));
        if applied {
            Ok(())
        } else {
            Err(CommandError::new(
                "opencode_config_not_applied",
                "OpenCode 配置与目标模型不一致",
            ))
        }
    }
}

fn config_path(detection: &AgentDetection) -> AppResult<&Path> {
    detection.config_path.as_deref().ok_or_else(|| {
        CommandError::new("opencode_config_path_missing", "未找到 OpenCode 配置路径")
    })
}

fn config_directory(context: &DiscoveryContext) -> PathBuf {
    if let Ok(directory) = env::var("OPENCODE_HOME") {
        let directory = PathBuf::from(directory);
        if directory.is_absolute() {
            return directory;
        }
    }
    if cfg!(target_os = "windows") {
        return context.application_data_dir.join("opencode");
    }
    context.home.join(".config").join("opencode")
}

fn resolve_config_path(context: &DiscoveryContext) -> PathBuf {
    let directory = config_directory(context);
    let json_path = directory.join("opencode.json");
    if json_path.exists() {
        return json_path;
    }
    directory.join("opencode.jsonc")
}

fn provider_config(desired: &DesiredAgentBinding<'_>) -> Value {
    json!({
        "npm": PROVIDER_SDK,
        "name": MANAGED_NAME,
        "options": {
            "baseURL": desired.base_url.trim_end_matches('/'),
            "apiKey": desired.credential,
        },
        "models": {
            desired.model_id: {
                "name": desired.model_id,
            },
        },
    })
}

fn is_managed_provider(provider: &Value) -> bool {
    provider.get("name").and_then(Value::as_str) == Some(MANAGED_NAME)
}

fn read_config(path: &Path) -> AppResult<Value> {
    match fs::read(path) {
        Ok(bytes) => parse_config(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error.into()),
    }
}

fn read_config_text(path: &Path) -> AppResult<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn managed_model_id(desired: &DesiredAgentBinding<'_>) -> String {
    format!("{MANAGED_PROVIDER}/{}", desired.model_id)
}

fn is_managed_model_id(model: &str) -> bool {
    model.starts_with(&format!("{MANAGED_PROVIDER}/"))
}

fn parse_config(bytes: &[u8]) -> AppResult<Value> {
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| CommandError::new("opencode_config_unparseable", "OpenCode 配置不是 UTF-8"))?;
    let normalized = strip_jsonc_comments(&text);
    if normalized.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_slice(normalized.as_bytes()).map_err(|_| {
        CommandError::new("opencode_config_unparseable", "OpenCode 配置不是有效 JSONC")
    })?;
    if !value.is_object()
        || value
            .get("provider")
            .is_some_and(|provider| !provider.is_object())
    {
        return Err(CommandError::new(
            "opencode_config_shape_unsupported",
            "OpenCode 配置的根节点或 provider 字段不是对象",
        ));
    }
    Ok(value)
}

#[derive(Debug)]
struct JsoncMember {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

#[derive(Debug)]
struct JsoncObject {
    close: usize,
    members: Vec<JsoncMember>,
}

impl JsoncObject {
    fn find(&self, key: &str) -> Option<usize> {
        self.members.iter().position(|member| member.key == key)
    }
}

fn jsonc_parse_error() -> CommandError {
    CommandError::new("opencode_config_unparseable", "OpenCode 配置不是有效 JSONC")
}

fn skip_jsonc_noise(source: &[u8], mut index: usize) -> usize {
    while index < source.len() {
        match source[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'/' if source.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < source.len() && source[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if source.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index < source.len()
                    && !(source[index] == b'*' && source.get(index + 1) == Some(&b'/'))
                {
                    index += 1;
                }
                index = (index + 2).min(source.len());
            }
            _ => break,
        }
    }
    index
}

fn parse_jsonc_string(source: &[u8], start: usize) -> AppResult<usize> {
    let mut index = start + 1;
    while index < source.len() {
        match source[index] {
            b'\\' => index = (index + 2).min(source.len()),
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err(jsonc_parse_error())
}

fn parse_jsonc_value_end(source: &[u8], start: usize) -> AppResult<usize> {
    match source.get(start) {
        Some(b'"') => parse_jsonc_string(source, start),
        Some(b'{') | Some(b'[') => {
            let mut index = start;
            let mut depth = 0usize;
            while index < source.len() {
                match source[index] {
                    b'"' => index = parse_jsonc_string(source, index)?,
                    b'/' if matches!(source.get(index + 1), Some(b'/') | Some(b'*')) => {
                        index = skip_jsonc_noise(source, index);
                    }
                    b'{' | b'[' => {
                        depth += 1;
                        index += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            return Ok(index);
                        }
                    }
                    _ => index += 1,
                }
            }
            Err(jsonc_parse_error())
        }
        _ => {
            let mut index = start;
            while index < source.len()
                && !matches!(source[index], b',' | b'}' | b']' | b'\r' | b'\n' | b'/')
            {
                index += 1;
            }
            Ok(index)
        }
    }
}

fn parse_jsonc_object_at(source: &str, open: usize) -> AppResult<JsoncObject> {
    let bytes = source.as_bytes();
    let mut index = open + 1;
    let mut members = Vec::new();
    loop {
        index = skip_jsonc_noise(bytes, index);
        let key_start = match bytes.get(index) {
            Some(b'}') => {
                return Ok(JsoncObject {
                    close: index,
                    members,
                })
            }
            Some(b'"') => index,
            _ => return Err(jsonc_parse_error()),
        };

        let key_end = parse_jsonc_string(bytes, key_start)?;
        let key =
            String::from_utf8_lossy(&bytes[key_start + 1..key_end.saturating_sub(1)]).to_string();
        index = skip_jsonc_noise(bytes, key_end);
        if bytes.get(index).copied() != Some(b':') {
            return Err(jsonc_parse_error());
        }
        index = skip_jsonc_noise(bytes, index + 1);
        let value_start = index;
        let value_end = parse_jsonc_value_end(bytes, value_start)?;
        index = skip_jsonc_noise(bytes, value_end);
        members.push(JsoncMember {
            key,
            key_start,
            value_start,
            value_end,
        });

        match bytes.get(index).copied() {
            Some(b',') => index = skip_jsonc_noise(bytes, index + 1),
            Some(b'}') => {
                return Ok(JsoncObject {
                    close: index,
                    members,
                })
            }
            _ => return Err(jsonc_parse_error()),
        }
    }
}

fn parse_jsonc_object(source: &str) -> AppResult<JsoncObject> {
    let bytes = source.as_bytes();
    let start = skip_jsonc_noise(bytes, 0);
    if bytes.get(start).copied() != Some(b'{') {
        return Err(jsonc_parse_error());
    }
    parse_jsonc_object_at(source, start)
}

fn target_jsonc_object(source: &str, parent: Option<&str>) -> AppResult<JsoncObject> {
    match parent {
        None => parse_jsonc_object(source),
        Some(parent) => {
            let root = parse_jsonc_object(source)?;
            let position = root.find(parent).ok_or_else(|| {
                CommandError::new(
                    "opencode_config_shape_unsupported",
                    "OpenCode 配置缺少 provider 对象",
                )
            })?;
            let member = &root.members[position];
            if source.as_bytes().get(member.value_start).copied() != Some(b'{') {
                return Err(CommandError::new(
                    "opencode_config_shape_unsupported",
                    "OpenCode 配置的 provider 字段不是对象",
                ));
            }
            parse_jsonc_object_at(source, member.value_start)
        }
    }
}

fn splice_jsonc(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut output = String::with_capacity(source.len() + replacement.len());
    output.push_str(&source[..start]);
    output.push_str(replacement);
    output.push_str(&source[end..]);
    output
}

fn render_jsonc_value(value: &Value) -> AppResult<String> {
    serde_json::to_string_pretty(value)
        .map_err(|_| CommandError::internal("无法生成 OpenCode 配置"))
}

fn insert_jsonc_member(source: &str, object: &JsoncObject, key: &str, rendered: &str) -> String {
    let member = format!(
        "{}: {}",
        serde_json::to_string(key).expect("JSON key"),
        rendered
    );
    if object.members.is_empty() {
        let prefix = source[..object.close].trim_end();
        let mut output = String::with_capacity(source.len() + member.len() + 8);
        output.push_str(prefix);
        output.push_str("\n  ");
        output.push_str(&member);
        output.push('\n');
        output.push_str(&source[object.close..]);
        return output;
    }

    let last = object.members.last().expect("non-empty object");
    let after_last = skip_jsonc_noise(source.as_bytes(), last.value_end);
    if source.as_bytes().get(after_last).copied() == Some(b',') {
        splice_jsonc(
            source,
            after_last + 1,
            after_last + 1,
            &format!("\n  {member}"),
        )
    } else {
        splice_jsonc(
            source,
            last.value_end,
            last.value_end,
            &format!(",\n  {member}"),
        )
    }
}

fn set_jsonc_member(
    source: &str,
    parent: Option<&str>,
    key: &str,
    value: &Value,
) -> AppResult<String> {
    let rendered = render_jsonc_value(value)?;
    if strip_jsonc_comments(source).trim().is_empty() {
        let member = format!(
            "{}: {}",
            serde_json::to_string(key).expect("JSON key"),
            rendered
        );
        return Ok(format!("{{\n  {member}\n}}\n"));
    }

    let object = target_jsonc_object(source, parent)?;
    if let Some(position) = object.find(key) {
        let member = &object.members[position];
        return Ok(splice_jsonc(
            source,
            member.value_start,
            member.value_end,
            &rendered,
        ));
    }
    Ok(insert_jsonc_member(source, &object, key, &rendered))
}

fn remove_jsonc_member(source: &str, parent: Option<&str>, key: &str) -> AppResult<String> {
    let object = target_jsonc_object(source, parent)?;
    let position = object.find(key).ok_or_else(|| {
        CommandError::new(
            "opencode_config_shape_unsupported",
            "OpenCode 配置缺少 AT-Switch 托管字段",
        )
    })?;
    let member = &object.members[position];
    let bytes = source.as_bytes();
    let after_member = skip_jsonc_noise(bytes, member.value_end);

    if bytes.get(after_member).copied() == Some(b',') {
        return Ok(splice_jsonc(source, member.key_start, after_member + 1, ""));
    }
    if let Some(previous) = position.checked_sub(1).map(|index| &object.members[index]) {
        let after_previous = skip_jsonc_noise(bytes, previous.value_end);
        if bytes.get(after_previous).copied() == Some(b',') {
            return Ok(splice_jsonc(source, after_previous, member.value_end, ""));
        }
    }
    Ok(splice_jsonc(source, member.key_start, member.value_end, ""))
}

#[allow(clippy::ptr_arg)] // ConfigProbe is shared with the existing adapters.
fn probe_config(path: &PathBuf) -> AppResult<()> {
    read_config(path).map(|_| ())
}

enum JsoncState {
    Code,
    String,
    Line,
    Block,
}

fn strip_jsonc_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut state = JsoncState::Code;
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        match state {
            JsoncState::Code => match character {
                '"' => {
                    output.push(character);
                    state = JsoncState::String;
                }
                '/' => match characters.peek() {
                    Some('/') => {
                        characters.next();
                        state = JsoncState::Line;
                    }
                    Some('*') => {
                        characters.next();
                        state = JsoncState::Block;
                    }
                    _ => output.push(character),
                },
                _ => output.push(character),
            },
            JsoncState::String => {
                output.push(character);
                if character == '\\' {
                    if let Some(escaped) = characters.next() {
                        output.push(escaped);
                    }
                } else if character == '"' {
                    state = JsoncState::Code;
                }
            }
            JsoncState::Line => {
                if character == '\n' {
                    output.push(character);
                    state = JsoncState::Code;
                }
            }
            JsoncState::Block => {
                if character == '*' && characters.peek() == Some(&'/') {
                    characters.next();
                    state = JsoncState::Code;
                }
            }
        }
    }

    remove_jsonc_trailing_commas(&output)
}

fn remove_jsonc_trailing_commas(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut pending_comma = false;
    for character in source.chars() {
        if pending_comma {
            if character.is_whitespace() {
                output.push(character);
            } else if character == '}' || character == ']' {
                pending_comma = false;
                output.push(character);
            } else if character == ',' {
                pending_comma = true;
            } else {
                output.push(',');
                output.push(character);
                pending_comma = false;
            }
        } else if character == ',' {
            pending_comma = true;
        } else {
            output.push(character);
        }
    }
    if pending_comma {
        output.push(',');
    }
    output
}

#[cfg(test)]
#[path = "opencode_tests.rs"]
mod tests;
