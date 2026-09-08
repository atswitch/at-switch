use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{json, Map, Value};

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::{
    domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError},
    services::BaselineSnapshot,
};

pub struct DuMateAdapter;

const MANAGED_PROVIDER: &str = "at-switch";
const NATIVE_PROVIDER: &str = "QianfanPersonalQuota";
const MANAGED_NAME: &str = "AT-Switch · 百度搭子";
const MODEL_TARGET_HEADER: &str = "X-Dumate-Model-Target";

impl AgentAdapter for DuMateAdapter {
    fn id(&self) -> &'static str {
        "dumate"
    }
    fn display_name(&self) -> &'static str {
        "百度搭子"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["DuMate.app", "百度搭子.app"],
            &["com.baidu.qianfan.desktop"],
            &[
                "Programs/DuMate/DuMate.exe",
                "DuMate/DuMate.exe",
                "Programs/百度搭子/DuMate.exe",
                "百度搭子/DuMate.exe",
                "DuMate.exe",
            ],
        );
        let app_data = context.application_data_dir.join("qianfan-desktop-app");
        let user_id = match resolve_user_id(&app_data) {
            Ok(Some(id)) => id,
            result => {
                return AgentDetection::manual(
                    self.id(),
                    self.display_name(),
                    installation,
                    &result.err().map(|e| e.message).unwrap_or_else(|| {
                        "请先登录百度搭子并打开一次编码智能体，再刷新状态。".to_owned()
                    }),
                )
            }
        };
        let runtime_data_dir = app_data.join("qianfan_desk_xdg").join(&user_id);
        // DuMate regenerates opencode.json on every start, then loads the
        // account-scoped opencode.jsonc override. The latter applies to every
        // conversation directory and survives desktop restarts.
        let mut detection = AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            runtime_data_dir.join("config/opencode/opencode.jsonc"),
            probe_config,
            true,
        );
        // Read-only source of native model aliases. Never mutate the generated file.
        detection.runtime_data_dir = Some(runtime_data_dir);
        detection
    }

    fn source_protocol(&self, _mode: AgentBindingMode, _upstream: ApiProtocol) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode == AgentBindingMode::Direct
            && desired.upstream_protocol != ApiProtocol::OpenaiChatCompletions
        {
            return Err(CommandError::new(
                "dumate_direct_protocol_unsupported",
                "百度搭子直连模式要求 Provider 支持 OpenAI Chat API",
            )
            .with_recovery("请改用本地代理模式，AT-Switch 会完成协议转换。"));
        }
        if desired.source_protocol != ApiProtocol::OpenaiChatCompletions {
            return Err(CommandError::new(
                "dumate_protocol_unsupported",
                "百度搭子仅支持 OpenAI Chat 兼容入口",
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
        let mut root = read_config(config_path(detection)?)?;
        let object = root.as_object_mut().expect("validated object");
        let providers = object
            .entry("provider")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("validated providers");
        providers.insert(
            MANAGED_PROVIDER.to_owned(),
            provider_config(desired, &[desired.model_id.to_owned()]),
        );

        // Existing conversations and DuMate's artifact validator can explicitly
        // request native aliases instead of the top-level default. Overlay those
        // aliases in this account override; keep the generated native source intact.
        let native = detection
            .runtime_data_dir
            .as_ref()
            .map(|dir| read_config(&dir.join("config/opencode/opencode.json")))
            .transpose()?;
        let mut aliases = vec![
            "glm-5".to_owned(),
            "model-text".to_owned(),
            "model-artifact-validate".to_owned(),
        ];
        for source in [
            native
                .as_ref()
                .and_then(|v| v.get("provider"))
                .and_then(|v| v.get(NATIVE_PROVIDER)),
            providers.get(NATIVE_PROVIDER),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(models) = source.get("models").and_then(Value::as_object) {
                aliases.extend(models.keys().cloned());
            }
        }
        aliases.sort();
        aliases.dedup();
        providers.insert(
            NATIVE_PROVIDER.to_owned(),
            provider_config(desired, &aliases),
        );
        object.insert(
            "model".to_owned(),
            json!(format!("{MANAGED_PROVIDER}/{}", desired.model_id)),
        );
        object.insert(
            "small_model".to_owned(),
            json!(format!("{MANAGED_PROVIDER}/{}", desired.model_id)),
        );
        encode(&root)
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        let mut current = read_config(config_path(detection)?)?;
        let baseline = if baseline.existed {
            parse_config(&baseline.content)?
        } else {
            json!({})
        };
        let baseline_uses_managed_route = ["model", "small_model"].iter().any(|key| {
            baseline
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|model| model.starts_with("at-switch/"))
        });
        let baseline_provider_is_absent_or_legacy_only = match baseline.get("provider") {
            None => true,
            Some(Value::Object(providers)) if baseline_uses_managed_route => {
                providers.keys().all(|id| id == MANAGED_PROVIDER)
            }
            _ => false,
        };
        let object = current.as_object_mut().expect("validated object");
        if let Some(providers) = object.get_mut("provider").and_then(Value::as_object_mut) {
            for provider_id in [MANAGED_PROVIDER, NATIVE_PROVIDER] {
                if providers.get(provider_id).is_some_and(is_managed_provider) {
                    match baseline
                        .get("provider")
                        .and_then(|p| p.get(provider_id))
                        .filter(|p| !is_managed_provider(p))
                        .filter(|_| provider_id != MANAGED_PROVIDER || !baseline_uses_managed_route)
                    {
                        Some(original) => {
                            providers.insert(provider_id.to_owned(), original.clone());
                        }
                        None => {
                            providers.remove(provider_id);
                        }
                    }
                }
            }
        }
        if baseline_provider_is_absent_or_legacy_only
            && object
                .get("provider")
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
        {
            object.remove("provider");
        }
        for key in ["model", "small_model"] {
            if object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|v| v.starts_with("at-switch/"))
            {
                match baseline
                    .get(key)
                    .filter(|v| !v.as_str().is_some_and(|m| m.starts_with("at-switch/")))
                {
                    Some(original) => {
                        object.insert(key.to_owned(), original.clone());
                    }
                    // No account override means DuMate loads its fresh native
                    // default, including the backend port chosen at startup.
                    None => {
                        object.remove(key);
                    }
                }
            }
        }
        encode(&current)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        self.validate_binding(desired)?;
        let value = read_config(config_path(detection)?)?;
        let expected_model = format!("{MANAGED_PROVIDER}/{}", desired.model_id);
        let models_match = ["model", "small_model"]
            .iter()
            .all(|key| value.get(key).and_then(Value::as_str) == Some(expected_model.as_str()));
        let providers_match = [MANAGED_PROVIDER, NATIVE_PROVIDER].iter().all(|id| {
            let Some(provider) = value.get("provider").and_then(|v| v.get(id)) else {
                return false;
            };
            let Some(models) = provider.get("models").and_then(Value::as_object) else {
                return false;
            };
            let required = if *id == MANAGED_PROVIDER {
                desired.model_id
            } else {
                "glm-5"
            };
            is_managed_provider(provider)
                && provider.get("npm").and_then(Value::as_str) == Some("@ai-sdk/openai-compatible")
                && provider.pointer("/options/baseURL").and_then(Value::as_str)
                    == Some(desired.base_url.trim_end_matches('/'))
                && provider.pointer("/options/apiKey").and_then(Value::as_str)
                    == Some(desired.credential)
                && models.contains_key(required)
                && (*id != NATIVE_PROVIDER || models.contains_key("model-artifact-validate"))
                && models.values().all(|m| {
                    m.get("id").and_then(Value::as_str) == Some(desired.model_id)
                        && m.get("headers")
                            .and_then(|h| h.get(MODEL_TARGET_HEADER))
                            .and_then(Value::as_str)
                            == Some(desired.model_id)
                })
        });
        if models_match && providers_match {
            Ok(())
        } else {
            Err(CommandError::new(
                "agent_config_not_applied",
                "百度搭子用户覆盖配置与目标模型不一致",
            ))
        }
    }
}

fn provider_config(desired: &DesiredAgentBinding<'_>, aliases: &[String]) -> Value {
    let models: Map<String, Value> = aliases
        .iter()
        .map(|alias| {
            (
                alias.clone(),
                json!({
                    "name": desired.model_id, "id": desired.model_id,
                    // DuMate rewrites body.model to dm-auto-model/text.L0 by default.
                    // Its fixed-target header takes precedence and is consumed locally by
                    // the built-in SDK fetch wrapper, not forwarded to the upstream.
                    "headers": { (MODEL_TARGET_HEADER): desired.model_id }
                }),
            )
        })
        .collect();
    json!({ "npm": "@ai-sdk/openai-compatible", "name": MANAGED_NAME,
        "options": { "baseURL": desired.base_url.trim_end_matches('/'), "apiKey": desired.credential },
        "models": models })
}

fn is_managed_provider(provider: &Value) -> bool {
    provider.get("name").and_then(Value::as_str) == Some(MANAGED_NAME)
}

fn config_path(detection: &AgentDetection) -> AppResult<&Path> {
    detection.config_path.as_deref().ok_or_else(|| {
        CommandError::new(
            "agent_config_path_missing",
            "未找到百度搭子当前账号的覆盖配置",
        )
    })
}

fn read_config(path: &Path) -> AppResult<Value> {
    match fs::read(path) {
        Ok(bytes) => parse_config(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error.into()),
    }
}

fn parse_config(bytes: &[u8]) -> AppResult<Value> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        CommandError::new(
            "agent_config_unparseable",
            "百度搭子 opencode.jsonc 不是有效 JSON",
        )
    })?;
    if !value.is_object() || value.get("provider").is_some_and(|p| !p.is_object()) {
        return Err(CommandError::new(
            "agent_config_shape_unsupported",
            "百度搭子配置的根节点或 provider 字段不是对象",
        ));
    }
    Ok(value)
}

#[allow(clippy::ptr_arg)] // ConfigProbe is shared with the existing adapters.
fn probe_config(path: &PathBuf) -> AppResult<()> {
    read_config(path).map(|_| ())
}
fn encode(value: &Value) -> AppResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| CommandError::internal("无法生成百度搭子配置"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

// Account lookup never scans arbitrary workspace folders (which may be tasks,
// not accounts), and never silently binds another user when auth is corrupt.
fn resolve_user_id(app_data: &Path) -> AppResult<Option<String>> {
    match fs::read(app_data.join("auth.json")) {
        Ok(bytes) => {
            let auth: Value = serde_json::from_slice(&bytes).map_err(|_| {
                CommandError::new(
                    "dumate_auth_invalid",
                    "百度搭子账号文件无法解析，请重新登录后刷新。",
                )
            })?;
            let profiles = auth.get("accountProfiles");
            let entries: Vec<(Option<&str>, &Value)> = match profiles {
                Some(Value::Array(list)) => list.iter().map(|p| (None, p)).collect(),
                Some(Value::Object(map)) => {
                    map.iter().map(|(id, p)| (Some(id.as_str()), p)).collect()
                }
                _ => Vec::new(),
            };
            if let Some(active) = auth
                .get("activeProfileId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                let profile = entries.iter().find(|(key, p)| {
                    *key == Some(active)
                        || p.get("profileId").and_then(Value::as_str) == Some(active)
                });
                let id = profile
                    .and_then(|(_, p)| p.get("bceUserId"))
                    .and_then(Value::as_str)
                    .and_then(valid_user_id);
                return id.map(Some).ok_or_else(|| {
                    CommandError::new(
                        "dumate_active_account_invalid",
                        "无法确定百度搭子当前账号，请重新登录后刷新。",
                    )
                });
            }
            // Legacy DuMate mirrors the active user's identity at the top level.
            if let Some(id) = auth
                .get("bceUserId")
                .and_then(Value::as_str)
                .and_then(valid_user_id)
            {
                return Ok(Some(id));
            }
            if let Some(id) = entries
                .iter()
                .filter_map(|(_, p)| {
                    let id = p
                        .get("bceUserId")
                        .and_then(Value::as_str)
                        .and_then(valid_user_id)?;
                    Some((p.get("lastLogin").and_then(Value::as_i64).unwrap_or(0), id))
                })
                .max_by(|a, b| a.cmp(b))
                .map(|(_, id)| id)
            {
                return Ok(Some(id));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let directory = app_data.join("qianfan_desk_xdg");
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().and_then(valid_user_id) else {
            continue;
        };
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, id));
    }
    candidates.sort();
    Ok(candidates.pop().map(|(_, id)| id))
}

fn valid_user_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && !value.starts_with('.')
        && !matches!(
            value,
            "global" | "data" | "config" | "cache" | "state" | "default" | "sessions"
        )
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then(|| value.to_owned())
}

#[cfg(test)]
#[path = "dumate_tests.rs"]
mod tests;
