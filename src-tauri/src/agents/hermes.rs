use std::{env, fs, path::Path, path::PathBuf};

use serde_yaml::{Mapping, Value};

use super::{
    locator::{
        locate_command, locate_desktop_app, DiscoveryContext, Installation, InstallationKind,
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

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        // 桌面版优先：桌面版存在时 config_directory 也指向它的 hermes-home，
        // 暂停/重启必须作用于桌面版进程，而不是 PATH 上的 CLI。
        let installation = locate_desktop_app(
            context,
            &["Hermes.app", "Hermes.exe", "Hermes"],
            &["com.hermes.agent"],
            &[
                "Programs/Hermes/Hermes.exe",
                "Hermes/Hermes.exe",
                "Hermes.exe",
                "Hermes Agent CN Desktop/hermes-agent-cn-desktop.exe",
            ],
        )
        .or_else(|| Self::detect_command(context))
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
            true,
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
        let model = model_block(&mapping).ok_or_else(applied_error)?;

        fn field<'a>(block: &'a Mapping, key: &str) -> Option<&'a str> {
            block.get(key).and_then(Value::as_str)
        }
        if field(model, "provider") != Some(MANAGED_PROVIDER)
            || field(model, "default") != Some(desired.model_id)
        {
            return Err(applied_error());
        }
        // 模型块不承载明文凭据。
        if model.contains_key("api_key") {
            return Err(applied_error());
        }
        // 受管 provider 条目必须与目标一致（端点、模型、凭据指针、API 形态）。
        let entry = mapping
            .get(Value::from("providers"))
            .and_then(Value::as_mapping)
            .and_then(|providers| providers.get(Value::from(MANAGED_PROVIDER)))
            .and_then(Value::as_mapping)
            .ok_or_else(applied_error)?;
        if field(entry, "base_url") != Some(desired.base_url.trim_end_matches('/'))
            || field(entry, "default_model") != Some(desired.model_id)
            || field(entry, "key_env") != Some(HERMES_API_KEY_ENV)
            || field(entry, "api_mode") != Some("chat_completions")
        {
            return Err(applied_error());
        }
        // provider 条目同样拒绝明文凭据。
        if entry.contains_key("api_key") {
            return Err(applied_error());
        }
        // 旧版适配器遗留的受管 custom_providers 条目必须清理。
        if legacy_managed_custom_provider_exists(&mapping) {
            return Err(applied_error());
        }
        if !env_file_has_api_key(detection) {
            return Err(CommandError::new(
                "hermes_api_key_env_missing",
                "Hermes 的凭据文件缺少 HERMES_AT_SWITCH_API_KEY",
            )
            .with_recovery(
                "AT-Switch 切换时会自动写入该键；若 ~/.hermes/.env 被手动改动，重新执行一次切换即可修复。",
            ));
        }
        Ok(())
    }
}

fn config_directory(context: &DiscoveryContext) -> Option<PathBuf> {
    resolve_config_directory(context, env::var("HERMES_HOME").ok().as_deref())
}

fn resolve_config_directory(
    context: &DiscoveryContext,
    hermes_home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(home) = hermes_home {
        let directory = PathBuf::from(home);
        if directory.is_absolute() {
            return Some(directory);
        }
    }
    if let Some(directory) = desktop_data_home(context) {
        return Some(directory);
    }
    Some(context.home.join(".hermes"))
}

/// 桌面版（Hermes Agent CN Desktop）把 CLI 的 hermes-home 数据目录放在
/// `%LOCALAPPDATA%\Hermes Agent CN Desktop\data\hermes-home`，与 CLI 的
/// `~/.hermes` 互不共享。装了桌面版时用户实际读取的是这里，必须优先；
/// 数据目录尚不存在（装好未启动过）时也按桌面版处理，写入会自动建目录。
#[cfg(target_os = "windows")]
fn desktop_data_home(context: &DiscoveryContext) -> Option<PathBuf> {
    let local = context
        .local_app_data
        .clone()
        .or_else(|| env::var_os("LOCALAPPDATA").map(PathBuf::from))?;
    let app_root = local.join("Hermes Agent CN Desktop");
    let data_home = app_root.join("data").join("hermes-home");
    let installed =
        app_root.join("hermes-agent-cn-desktop.exe").is_file() || data_home.is_dir();
    installed.then_some(data_home)
}

#[cfg(not(target_os = "windows"))]
fn desktop_data_home(context: &DiscoveryContext) -> Option<PathBuf> {
    let _ = context;
    None
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
    if let Some(model) = mapping.get(Value::from("model")) {
        if !model.is_mapping() && !model.is_string() {
            // v45 使用嵌套块，旧版使用扁平字符串；其余形状一律拒绝。
            return Err(shape_error());
        }
    }
    if let Some(custom_providers) = mapping.get(Value::from("custom_providers")) {
        if !custom_providers.is_sequence() && !custom_providers.is_null() {
            return Err(shape_error());
        }
    }
    if let Some(providers) = mapping.get(Value::from("providers")) {
        if !providers.is_mapping() && !providers.is_null() {
            return Err(shape_error());
        }
    }
    Ok(())
}

fn read_config_mapping(path: &Path) -> AppResult<Mapping> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(Mapping::new());
    };
    let value: Value = serde_yaml::from_slice(&bytes).map_err(|_| unparseable_error())?;
    if value.is_null() {
        return Ok(Mapping::new());
    }
    value.as_mapping().cloned().ok_or_else(shape_error)
}

/// Hermes v0.21.x（`_config_version: 45`）只认两种受管写法：
/// 命名 provider 条目（`providers.<name>`，凭据经条目 `key_env` 指针解析），
/// 或裸 `provider: custom` 终端回退。裸 custom 的终端解析器**不读**模型块上的
/// `key_env`——没有 key 时会以 `no-key-required` 占位符发起请求，服务端返回
/// 401。因此写入命名条目并让 `model.provider` 指向它；凭据经
/// `~/.hermes/.env` 的 `key_env` 指针，不落明文。
fn update_hermes_config(original: &str, desired: &DesiredAgentBinding<'_>) -> AppResult<String> {
    let mut root: Value = serde_yaml::from_str(original).map_err(|_| unparseable_error())?;
    if !root.is_mapping() {
        root = Value::Mapping(Mapping::new());
    }
    let root_map = root.as_mapping_mut().expect("checked mapping");

    let model = ensure_mapping(root_map, "model")?;
    for (key, value) in [("provider", MANAGED_PROVIDER), ("default", desired.model_id)] {
        model.insert(Value::from(key), Value::from(value));
    }
    // 端点与凭据收敛到 provider 条目；模型块不再重复承载（也顺带清掉旧适配器
    // 写进模型块的 key_env/base_url，避免裸 custom 回退路径读到悬空指针）。
    for key in ["base_url", "api_mode", "key_env", "api_key", "api_key_env"] {
        model.remove(Value::from(key));
    }

    let providers = ensure_mapping(root_map, "providers")?;
    let entry = ensure_mapping(providers, MANAGED_PROVIDER)?;
    let entry_fields = [
        ("name", DISPLAY_NAME),
        ("base_url", desired.base_url.trim_end_matches('/')),
        ("default_model", desired.model_id),
        ("key_env", HERMES_API_KEY_ENV),
        ("api_mode", "chat_completions"),
    ];
    for (key, value) in entry_fields {
        entry.insert(Value::from(key), Value::from(value));
    }
    entry.insert(Value::from("enabled"), Value::from(true));
    // 拒绝明文凭据：无论来自旧适配器还是用户手写，统一改为 key_env 指针。
    entry.remove(Value::from("api_key"));
    entry.remove(Value::from("api_key_env"));

    remove_legacy_managed_custom_provider(root_map);
    remove_legacy_managed_flat_provider(root_map);

    let rendered = serde_yaml::to_string(&root).map_err(|error| {
        CommandError::new(
            "agent_config_serialize_failed",
            format!("无法生成 Hermes 配置：{error}"),
        )
    })?;
    Ok(rendered)
}

fn ensure_mapping<'a>(root: &'a mut Mapping, key: &str) -> AppResult<&'a mut Mapping> {
    let key = Value::from(key);
    if !root.get(&key).is_some_and(Value::is_mapping) {
        root.insert(key.clone(), Value::Mapping(Mapping::new()));
    }
    root.get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .ok_or_else(shape_error)
}

fn remove_legacy_managed_custom_provider(root: &mut Mapping) {
    let key = Value::from("custom_providers");
    if let Some(Value::Sequence(entries)) = root.get_mut(&key) {
        entries.retain(|entry| entry.get("name").and_then(Value::as_str) != Some(MANAGED_PROVIDER));
    }
}

/// 旧版适配器会写扁平的 `provider: custom:at-switch:<model>`；只清理
/// AT-Switch 自己写入的值，用户指向其它 Provider 的配置原样保留。
fn remove_legacy_managed_flat_provider(root: &mut Mapping) {
    let key = Value::from("provider");
    let managed = root
        .get(&key)
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with(&format!("custom:{MANAGED_PROVIDER}:")));
    if managed {
        root.remove(&key);
    }
}

fn legacy_managed_custom_provider_exists(mapping: &Mapping) -> bool {
    mapping
        .get(Value::from("custom_providers"))
        .and_then(Value::as_sequence)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("name").and_then(Value::as_str) == Some(MANAGED_PROVIDER))
        })
}

fn model_block(mapping: &Mapping) -> Option<&Mapping> {
    mapping
        .get(Value::from("model"))
        .and_then(Value::as_mapping)
}

fn env_file_path(detection: &AgentDetection) -> Option<PathBuf> {
    detection
        .config_path
        .as_ref()?
        .parent()
        .map(|directory| directory.join(".env"))
}

/// Hermes 启动时加载 `~/.hermes/.env`，运行时通过 `os.getenv(key_env)` 解析
/// 凭据指针；没有这一行，切换后的模型会因缺少 API Key 而无法请求。
fn env_file_has_api_key(detection: &AgentDetection) -> bool {
    let Some(path) = env_file_path(detection) else {
        return false;
    };
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content.lines().any(|line| {
        line.trim_start()
            .strip_prefix("HERMES_AT_SWITCH_API_KEY=")
            .is_some_and(|value| !value.trim().is_empty())
    })
}

/// 切换时把凭据写入 `~/.hermes/.env` 的受管行：已有则原地替换（覆盖
/// Direct 凭据轮换与 Direct↔Proxy 互切后的本地令牌），没有则追加；用户
/// 自有的其它键原样保留。config.yaml 只承载 `key_env` 指针，凭据本体由
/// 这一步落盘，`verify_config` 才能放行。
pub(super) fn ensure_env_api_key(
    detection: &AgentDetection,
    credential: &str,
) -> AppResult<()> {
    if credential.trim().is_empty() {
        return Err(CommandError::new(
            "hermes_api_key_empty",
            "待写入的 Hermes API Key 为空",
        ));
    }
    let path = env_file_path(detection)
        .ok_or_else(|| CommandError::new("agent_config_path_missing", "Hermes 配置路径不可用"))?;
    let prefix = format!("{HERMES_API_KEY_ENV}=");
    let managed_line = format!("{prefix}{credential}");
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    let mut replaced = false;
    for line in lines.iter_mut() {
        if line.trim_start().starts_with(&prefix) {
            *line = managed_line.clone();
            replaced = true;
        }
    }
    if !replaced {
        lines.push(managed_line);
    }
    #[cfg(unix)]
    let created = !path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::new(
                "hermes_env_write_failed",
                format!("无法创建 Hermes 凭据目录：{error}"),
            )
        })?;
    }
    fs::write(&path, lines.join("\n") + "\n").map_err(|error| {
        CommandError::new(
            "hermes_env_write_failed",
            format!("无法写入 Hermes 凭据文件：{error}"),
        )
    })?;
    #[cfg(unix)]
    {
        if created {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    Ok(())
}

fn applied_error() -> CommandError {
    CommandError::new("agent_config_not_applied", "Hermes 配置与目标模型不一致")
}

fn shape_error() -> CommandError {
    CommandError::new("agent_config_shape_unsupported", "Hermes 配置结构不受支持")
}

fn unparseable_error() -> CommandError {
    CommandError::new("agent_config_unparseable", "Hermes 配置不是有效 YAML")
}

fn hermes_package_version(path: &Path) -> Option<String> {
    // Hermes 的安装目录带有源码清单；从 package.json / 目录清单里读取版本，
    // 读取失败时返回 None，状态展示退化为不显示版本。
    let meta = fs::read_dir(path).ok()?;
    for entry in meta.flatten() {
        let name = entry.file_name();
        if name != "package.json" && name != "app-update.yml" {
            continue;
        }
        let bytes = fs::read(entry.path()).ok()?;
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
