use std::{env, fs, path::Path, path::PathBuf};

use serde_yaml::Value;

use super::{
    locator::{
        locate_command, locate_desktop_app, DiscoveryContext, DiscoveryHints, Installation,
        InstallationKind,
    },
    AgentAdapter, AgentDetection, BaselineSnapshot, DesiredAgentBinding, LaunchEnv,
};
use crate::domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError};

const DISPLAY_NAME: &str = "DeepSeek Harness";
/// dsh composes its plugin tree from named profiles. Every profile keeps its own
/// patch layer at `<home>/profiles/<profile>/cordis.patch.yml`; the patch layer is
/// the only user-owned file in the tree and the only one AT-Switch may write.
const PATCH_FILE: &str = "cordis.patch.yml";
/// Patch entry that selects the model dsh uses for a fresh session.
const DEFAULT_MODEL_ENTRY: &str = "agent-default-model";
/// Patch entry whose config hosts the provider registry (`llm-pi-ai` is the
/// bundle-provided OpenAI-compatible adapter; targeting it by `id` overrides the
/// config of an already-loaded plugin instead of adding a new one).
const PROVIDER_ENTRY: &str = "llm-pi-ai";
/// Provider id used for every AT-Switch managed provider. Scoped so a user's own
/// providers keep working untouched.
const MANAGED_PROVIDER: &str = "at-switch";
const DSH_API_KEY_ENV: &str = "DSH_AT_SWITCH_API_KEY";
/// dsh's wire protocol for the managed provider. Only the OpenAI Chat flavour is
/// wired up; Responses/Anthropic reach dsh through the local proxy.
const MANAGED_API: &str = "openai-completions";
const DEFAULT_PROFILE: &str = "desktop";
/// Variable name AT-Switch sets on launchd via `launchctl setenv` so that the
/// dsh GUI App inherits it on startup. dsh refuses the same variable name in
/// profile-level `.env` files (it is a launching-environment variable), so the
/// env-var route is the only one dsh accepts.
pub(crate) const DSH_API_KEY_LAUNCHD_VAR: &str = "DSH_AT_SWITCH_API_KEY";

pub struct DshAdapter;

impl DshAdapter {
    /// Resolves the dsh home directory. `DSH_HOME` wins when it is an absolute
    /// path, otherwise the documented `~/.dsh` default applies.
    fn home_directory(context: &DiscoveryContext) -> PathBuf {
        if let Some(home) = absolute_env_path("DSH_HOME") {
            return home;
        }
        context.home.join(".dsh")
    }

    /// dsh reaches users through two surfaces: a `com.deepseek.dsh` desktop App
    /// and an npm-distributed CLI. The desktop App is probed first because it is
    /// what almost every install has, and detecting it as a desktop bundle is what
    /// lets the apply pipeline stop the running host, patch the config, and bring
    /// it back up. The CLI and the profile directory remain as fallbacks for
    /// headless installs that have no App bundle at all.
    fn detect_installation(context: &DiscoveryContext) -> Option<Installation> {
        locate_desktop_app(
            context,
            &["DeepSeek Harness.app"],
            &["com.deepseek.dsh"],
            &["DeepSeek Harness/DeepSeek Harness.exe"],
        )
        .or_else(|| locate_command(context, &["dsh", "dsh.exe", "dsh.cmd"]))
        .or_else(|| {
            let profiles = Self::home_directory(context).join("profiles");
            profiles.is_dir().then_some(Installation {
                version: None,
                path: profiles,
                kind: InstallationKind::Command,
            })
        })
    }
}

impl AgentAdapter for DshAdapter {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn discovery_hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            macos_bundle_identifiers: &["com.deepseek.dsh"],
            windows_relative_paths: &["DeepSeek Harness/DeepSeek Harness.exe"],
        }
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let config_path = Self::home_directory(context)
            .join("profiles")
            .join(active_profile(context))
            .join(PATCH_FILE);
        AgentDetection::from_file_probe(
            self.id(),
            DISPLAY_NAME,
            Self::detect_installation(context),
            config_path,
            probe_patch,
            true,
        )
    }

    fn source_protocol(
        &self,
        _desired_mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        // dsh talks OpenAI Chat to the managed provider by design; every other
        // upstream protocol is converted by the local proxy before it reaches
        // dsh, so the wire format dsh sees never changes.
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode == AgentBindingMode::Direct
            && desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "dsh_direct_protocol_unsupported",
                "DeepSeek Harness 直连模式仅支持 OpenAI Chat 协议",
            )
            .with_recovery("请改用本地代理模式，AT-Switch 会完成协议转换。"));
        }
        Ok(())
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        self.validate_binding(desired)?;
        let path = config_path(detection)?;
        let original = fs::read_to_string(path).unwrap_or_default();
        // A patch file that does not parse must never be taken over: rewriting it
        // blind would drop whatever the user (or dsh itself) put there.
        read_patch_entries(path)?;
        Ok(update_patch(&original, desired)?.into_bytes())
    }

    fn build_native_config(
        &self,
        _detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        if baseline.existed {
            return Ok(baseline.content.clone());
        }
        // Never managed by AT-Switch: the patch layer on disk is still whatever
        // dsh wrote on first run, so restore that untouched. Returning an empty
        // entry list lets dsh fall back to its own default provider/model without
        // hard-coding a vendor-specific provider id.
        Ok(b"[]\n".to_vec())
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        let entries = read_patch_entries(config_path(detection)?)?;
        let default_entry = entries
            .iter()
            .find(|entry| entry_id(entry) == Some(DEFAULT_MODEL_ENTRY))
            .ok_or_else(not_applied_error)?;
        let default_config = entry_config(default_entry).ok_or_else(not_applied_error)?;
        if entry_str(default_config, "provider") != Some(MANAGED_PROVIDER)
            || entry_str(default_config, "model") != Some(desired.model_id)
        {
            return Err(not_applied_error());
        }

        let provider_entry = entries
            .iter()
            .find(|entry| entry_id(entry) == Some(PROVIDER_ENTRY))
            .ok_or_else(not_applied_error)?;
        let provider =
            managed_provider(entry_config(provider_entry).ok_or_else(not_applied_error)?)
                .ok_or_else(not_applied_error)?;

        if entry_str(provider, "api") != Some(MANAGED_API)
            || entry_str(provider, "baseURL") != Some(desired.base_url.trim_end_matches('/'))
            || entry_str(provider, "apiKeyEnv") != Some(DSH_API_KEY_ENV)
        {
            return Err(not_applied_error());
        }
        if provider.contains_key("apiKey") {
            // A plaintext key in the patch layer would defeat the Hermes-style
            // credential split, so treat its presence as a failed apply.
            return Err(not_applied_error());
        }
        if declared_models(provider)
            .iter()
            .any(|model| model != &desired.model_id)
        {
            return Err(not_applied_error());
        }
        Ok(())
    }

    /// dsh reserves `DSH_AT_SWITCH_API_KEY` as a launching-environment
    /// variable: setting it in profile `.env` aborts startup with
    /// "which only the launching environment may set". Returning it here lets
    /// the apply pipeline publish it via `launchctl setenv` before dsh is
    /// relaunched, which is the one delivery channel dsh accepts.
    fn launch_env<'a>(&self, desired: &'a DesiredAgentBinding<'a>) -> LaunchEnv<'a> {
        vec![(DSH_API_KEY_LAUNCHD_VAR, desired.credential)]
    }
}

fn config_path(detection: &AgentDetection) -> AppResult<&Path> {
    detection
        .config_path
        .as_deref()
        .ok_or_else(|| CommandError::new("agent_config_path_missing", "dsh 配置路径不可用"))
}

/// Resolves the profile whose patch layer AT-Switch manages. `DSH_PROFILE` is
/// honoured when it names an existing directory, otherwise the default profile
/// (`desktop`) is used; the remaining profiles are only probed when neither is
/// present, which covers headless-only installs.
fn active_profile(context: &DiscoveryContext) -> String {
    let profiles = DshAdapter::home_directory(context).join("profiles");
    if let Some(requested) = absolute_env_path("DSH_PROFILE") {
        let name = requested
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if profiles.join(&name).is_dir() {
            return name;
        }
    }
    if profiles.join(DEFAULT_PROFILE).is_dir() {
        return DEFAULT_PROFILE.to_owned();
    }
    std::fs::read_dir(&profiles)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .find(|name| profiles.join(name).is_dir())
        })
        .unwrap_or_else(|| DEFAULT_PROFILE.to_owned())
}

fn absolute_env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[allow(clippy::ptr_arg)] // ConfigProbe is shared with the existing adapters.
fn probe_patch(path: &PathBuf) -> AppResult<()> {
    read_patch_entries(path).map(|_| ())
}

/// Parses the patch layer into its top-level entry list, rejecting anything that
/// is not a list of mappings — dsh cannot boot from such a file either.
fn read_patch_entries(path: &Path) -> AppResult<Vec<Value>> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(Vec::new());
    };
    if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(Vec::new());
    }
    let value: Value = serde_yaml::from_slice(&bytes).map_err(|_| unparseable_error())?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    let entries = value.as_sequence().ok_or_else(shape_error)?;
    if entries.iter().any(|entry| !entry.is_mapping()) {
        return Err(shape_error());
    }
    Ok(entries.clone())
}

fn entry_id(entry: &Value) -> Option<&str> {
    entry.get("id").and_then(Value::as_str)
}

fn entry_config(entry: &Value) -> Option<&serde_yaml::Mapping> {
    entry.get("config").and_then(Value::as_mapping)
}

fn entry_str<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    mapping.get(Value::from(key)).and_then(Value::as_str)
}

fn managed_provider(config: &serde_yaml::Mapping) -> Option<&serde_yaml::Mapping> {
    config
        .get(Value::from("providers"))
        .and_then(Value::as_mapping)
        .and_then(|providers| providers.get(Value::from(MANAGED_PROVIDER)))
        .and_then(Value::as_mapping)
}

/// Model ids declared under the managed provider, in document order.
fn declared_models(provider: &serde_yaml::Mapping) -> Vec<&str> {
    provider
        .get(Value::from("models"))
        .and_then(Value::as_sequence)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

fn not_applied_error() -> CommandError {
    CommandError::new("dsh_config_not_applied", "dsh 配置与目标模型不一致")
}

fn shape_error() -> CommandError {
    CommandError::new(
        "dsh_config_shape_unsupported",
        "dsh 补丁配置不是受支持的条目列表",
    )
}

fn unparseable_error() -> CommandError {
    CommandError::new("dsh_config_unparseable", "dsh 补丁配置不是有效 YAML")
}

/// Rewrites the patch layer in place, touching only the two managed entries.
/// Every other entry — user comments, UI settings, hand-written providers — is
/// carried over verbatim.
fn update_patch(original: &str, desired: &DesiredAgentBinding<'_>) -> AppResult<String> {
    let mut lines: Vec<String> = original
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.to_owned()
            }
        })
        .collect();

    // A pristine `[]` document has a single placeholder line and no entries at
    // all; drop it so the new entries are not nested inside a flow sequence.
    if lines.len() == 1 && lines[0].trim() == "[]" {
        lines.clear();
    }

    upsert_default_model_entry(&mut lines, desired);
    upsert_provider_entry(&mut lines, desired);
    // `llm-pi-ai` must win over the bundle's own default-model entry, which is
    // injected by a bundle layer rather than the patch layer.
    ensure_last(&mut lines, PROVIDER_ENTRY);

    let ends_with_newline = original.ends_with('\n') || original.is_empty();
    let mut rendered = lines.join("\n");
    if ends_with_newline && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    validate_rendered(&rendered)?;
    Ok(rendered)
}

/// Overwrites `config.provider` / `config.model` of the managed default-model
/// entry, or appends the whole entry when dsh has not written one yet.
fn upsert_default_model_entry(lines: &mut Vec<String>, desired: &DesiredAgentBinding<'_>) {
    if find_entry_start(lines, DEFAULT_MODEL_ENTRY).is_some() {
        replace_managed_keys(lines, DEFAULT_MODEL_ENTRY, desired);
        return;
    }
    append_entry(lines, &render_default_model_entry(desired));
}

/// Keeps `config.providers` of the `llm-pi-ai` entry in sync: the `at-switch`
/// provider is replaced wholesale (its shape is fully owned by AT-Switch), while
/// every sibling provider the user registered by hand survives untouched.
fn upsert_provider_entry(lines: &mut Vec<String>, desired: &DesiredAgentBinding<'_>) {
    let Some(start) = find_entry_start(lines, PROVIDER_ENTRY) else {
        append_entry(lines, &render_provider_entry(desired));
        return;
    };
    let end = entry_end(lines, start);
    let managed_at =
        (start..end).find(|index| lines[*index].trim() == format!("{MANAGED_PROVIDER}:"));
    let Some(managed_at) = managed_at else {
        // The entry exists without our provider (user registered their own
        // providers) — inject ours as the last key of `providers`.
        if let Some(providers_at) =
            (start..end).find(|index| is_key_line(&lines[*index], "providers"))
        {
            let indent = leading_spaces(&lines[providers_at]) + 2;
            let block = render_provider_body(desired, indent);
            lines.splice(providers_at + 1..providers_at + 1, block);
        } else if let Some(config_at) =
            (start..end).find(|index| is_key_line(&lines[*index], "config"))
        {
            let indent = leading_spaces(&lines[config_at]) + 2;
            let mut block = vec![format!("{}providers:\n", " ".repeat(indent))];
            block.extend(render_provider_body(desired, indent + 2));
            lines.splice(config_at + 1..config_at + 1, block);
        }
        return;
    };
    let managed_end = (managed_at + 1..end)
        .find(|index| {
            let indent = leading_spaces(&lines[*index]);
            indent > 0 && indent <= leading_spaces(&lines[managed_at])
        })
        .unwrap_or(end);
    lines.splice(
        managed_at..managed_end,
        render_provider_body(desired, leading_spaces(&lines[managed_at])),
    );
}

fn render_default_model_entry(desired: &DesiredAgentBinding<'_>) -> String {
    format!(
        "- id: {DEFAULT_MODEL_ENTRY}\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: {MANAGED_PROVIDER}\n    model: {}\n",
        yaml_scalar(desired.model_id)
    )
}

fn render_provider_entry(desired: &DesiredAgentBinding<'_>) -> String {
    let mut lines = vec![
        format!("- id: {PROVIDER_ENTRY}"),
        "  name: \"@deepseek-ai/dsh-llm-pi-ai\"".to_owned(),
        "  config:".to_owned(),
        "    providers:".to_owned(),
    ];
    lines.extend(render_provider_body(desired, 6));
    format!("{}\n", lines.join("\n"))
}

fn yaml_scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Rewrites the entry whose `- id:` matches `id`. Only the keys AT-Switch owns
/// are touched: every sibling key the user set inside `config` — `temperature`,
/// hand-written comments — is carried over verbatim, so taking over the entry
/// never silently resets the user's own tuning.
///
/// `reasoningEffort` is **not** preserved: it is a deepseek-flash-only setting
/// that is incompatible with third-party providers. Removing it lets dsh fall
/// back to its own default behaviour for the target model.
fn replace_managed_keys(lines: &mut Vec<String>, id: &str, desired: &DesiredAgentBinding<'_>) {
    let Some(start) = find_entry_start(lines, id) else {
        return;
    };
    let end = entry_end(lines, start);
    let config_at = (start..end).find(|index| is_key_line(&lines[*index], "config"));
    let Some(config_at) = config_at else {
        // No `config:` mapping to update and we must not guess where user keys
        // belong; leave the entry untouched rather than corrupting it.
        return;
    };
    let key_indent = leading_spaces(&lines[config_at]) + 2;
    // Keys AT-Switch manages: overwrite in place when present, insert when not.
    let mut rebuilt: Vec<String> = Vec::with_capacity(end - start + 4);
    let mut provider_done = false;
    let mut model_done = false;
    for (offset, index) in (start..end).enumerate() {
        let line = &lines[index];
        if offset > config_at - start {
            match key_of(line) {
                Some("provider") if !provider_done => {
                    rebuilt.push(format!(
                        "{}provider: {MANAGED_PROVIDER}",
                        " ".repeat(leading_spaces(line))
                    ));
                    provider_done = true;
                    continue;
                }
                Some("model") if !model_done => {
                    rebuilt.push(format!(
                        "{}model: {}",
                        " ".repeat(leading_spaces(line)),
                        yaml_scalar(desired.model_id)
                    ));
                    model_done = true;
                    continue;
                }
                // `reasoningEffort` is deepseek-flash-specific and breaks third-party
                // providers — drop it silently so dsh uses its own model default.
                Some("reasoningEffort") => {
                    continue;
                }
                _ => {}
            }
        }
        rebuilt.push(line.clone());
    }
    let mut insert_at = config_at - start + 1;
    if !provider_done {
        rebuilt.insert(
            insert_at,
            format!("{}provider: {MANAGED_PROVIDER}", " ".repeat(key_indent)),
        );
        insert_at += 1;
    }
    if !model_done {
        rebuilt.insert(
            insert_at,
            format!(
                "{}model: {}",
                " ".repeat(key_indent),
                yaml_scalar(desired.model_id)
            ),
        );
    }
    lines.splice(start..end, rebuilt);
}

/// Appends `rendered` as a new top-level entry, separated by a blank line so the
/// document stays readable. Used when dsh has not yet emitted an entry of this
/// id and AT-Switch therefore has to seed it.
fn append_entry(lines: &mut Vec<String>, rendered: &str) {
    if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.push(String::new());
    }
    for line in rendered.trim_end_matches('\n').lines() {
        lines.push(line.to_owned());
    }
}

/// Renders the managed provider block (api / baseURL / apiKeyEnv / models)
/// under an arbitrary `indent`, so the same body is reused whether the block
/// sits at the top of `providers:` or nests further down.
fn render_provider_body(desired: &DesiredAgentBinding<'_>, indent: usize) -> Vec<String> {
    let inner = " ".repeat(indent + 2);
    let deeper = " ".repeat(indent + 4);
    vec![
        format!("{}at-switch:", " ".repeat(indent)),
        format!("{}api: {MANAGED_API}", inner),
        format!(
            "{}baseURL: {}",
            inner,
            yaml_scalar(desired.base_url.trim_end_matches('/'))
        ),
        format!("{}apiKeyEnv: {DSH_API_KEY_ENV}", inner),
        format!("{}models:", inner),
        format!("{}- id: {}", deeper, yaml_scalar(desired.model_id)),
    ]
}

fn is_key_line(line: &str, key: &str) -> bool {
    key_of(line) == Some(key)
}

fn key_of(line: &str) -> Option<&str> {
    let trimmed = line.trim_start().trim_end_matches('\n');
    let end = trimmed.find(':')?;
    let key = trimmed[..end].trim();
    if key.is_empty() || key.contains(' ') {
        None
    } else {
        Some(key)
    }
}

/// Moves the entry identified by `id` (with its comment block) to the end of the
/// document so later layers override earlier ones.
fn ensure_last(lines: &mut Vec<String>, id: &str) {
    let Some(start) = find_entry_start(lines, id) else {
        return;
    };
    let end = entry_end(lines, start);
    if end == lines.len() {
        return;
    }
    let entry: Vec<String> = lines[start..end].to_vec();
    lines.drain(start..end);
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.extend(entry);
}

fn find_entry_start(lines: &[String], id: &str) -> Option<usize> {
    lines.iter().position(|line| {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("- ") else {
            return false;
        };
        // Entries are `- id: <value>`, so the id lives in the *value* position
        // of the first key, not in the key itself.
        entry_id_value(rest) == Some(id)
    })
}

/// Extracts the value of the `- id: <value>` marker that opens a patch entry.
fn entry_id_value(rest: &str) -> Option<&str> {
    let end = rest.find(':')?;
    let key = rest[..end].trim();
    if key != "id" {
        return None;
    }
    let value = rest[end + 1..].trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// End of the entry that starts at `start`, i.e. the first following line that
/// belongs to the next top-level list item or leaves the document's list.
fn entry_end(lines: &[String], start: usize) -> usize {
    (start + 1..lines.len())
        .find(|index| {
            let line = &lines[*index];
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                return false;
            }
            leading_spaces(line) == 0 && trimmed.starts_with("- ")
        })
        .unwrap_or(lines.len())
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn validate_rendered(content: &str) -> AppResult<()> {
    let value: Value = serde_yaml::from_str(content).map_err(|_| unparseable_error())?;
    let Some(entries) = value.as_sequence() else {
        return Err(shape_error());
    };
    if entries.iter().any(|entry| !entry.is_mapping()) {
        return Err(shape_error());
    }
    Ok(())
}

#[cfg(test)]
#[path = "dsh_tests.rs"]
mod tests;
