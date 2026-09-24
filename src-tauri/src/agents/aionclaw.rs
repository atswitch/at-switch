use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::domain::{AppResult, CommandError};

use super::{AgentDetection, DesiredAgentBinding};

pub(crate) const MANAGED_PROVIDER_ID: &str = "at-switch";
const CONFIG_KEY: &str = "app_config";

/// AionClaw 的模型状态有两处必须同时改写，缺一个都不生效：
///
/// 1. `kv.app_config` —— Provider 目录与 `model.defaultModel` / `defaultModelProvider`。
///    应用启动时由它重建 `openclaw/state/openclaw.json`，并给用户自建的 Provider
///    重新编号（实测用户的自定义 Provider 在 JSON 里叫 `custom_0`），所以直接写
///    `openclaw.json` 会被启动流程覆盖。
/// 2. `agents.model` —— **界面真正显示与使用的当前模型**，格式为
///    `<provider>/<model_id>`。只改第 1 处时，模型会被列进界面但仍处于"未选中"
///    状态，因为 Agent 自身还指向旧模型。
///
/// `app_config` 结构（实测）：
/// ```json
/// {
///   "providers": { "custom_0": { "enabled": true, "apiKey": "...", "baseUrl": "...",
///                                "apiFormat": "openai", "displayName": "g2",
///                                "models": [{ "id": "NMauto", "name": "NMauto", "supportsImage": false }] } },
///   "model": { "defaultModel": "NMauto", "defaultModelProvider": "custom_0",
///              "availableModels": [ ... ] }
/// }
/// ```
pub(crate) struct ConfigChange {
    pub previous: String,
    pub previous_agent_model: Option<String>,
}

pub(crate) fn apply_config(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<ConfigChange> {
    let path = database_path(detection)?;
    let connection = Connection::open(path).map_err(|error| {
        CommandError::new(
            "aionclaw_db_locked",
            format!("无法写入 AionClaw 的配置数据库：{error}"),
        )
        .with_recovery("请完全退出 AionClaw 后重试。")
    })?;

    let previous: String = connection
        .query_row("SELECT value FROM kv WHERE key = ?1", [CONFIG_KEY], |row| {
            row.get(0)
        })
        .map_err(|error| {
            CommandError::new(
                "aionclaw_config_unreadable",
                format!("无法读取 AionClaw 的模型配置：{error}"),
            )
            .with_recovery("请确认 AionClaw 已至少启动过一次。")
        })?;

    let mut root: Value = serde_json::from_str(&previous).map_err(|error| {
        CommandError::new(
            "aionclaw_config_unparseable",
            format!("AionClaw 的模型配置不是有效的 JSON：{error}"),
        )
    })?;
    let object = root.as_object_mut().ok_or_else(|| shape_error("根节点"))?;

    // 受管 Provider 固定为单个条目：每次切换是替换而非追加，重复切换保持幂等。
    let providers = object
        .entry("providers")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| shape_error("providers"))?;
    providers.retain(|key, _| key != MANAGED_PROVIDER_ID);
    providers.insert(
        MANAGED_PROVIDER_ID.to_owned(),
        json!({
            "enabled": true,
            "displayName": "AT-Switch",
            "baseUrl": desired.base_url.trim_end_matches('/'),
            "apiKey": desired.credential,
            "apiFormat": aionclaw_api_format(desired.source_protocol),
            "models": [{
                "id": desired.model_id,
                "name": desired.model_id,
                "supportsImage": false,
            }],
        }),
    );

    let model = object
        .entry("model")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| shape_error("model"))?;
    model.insert(
        "defaultModel".to_owned(),
        Value::String(desired.model_id.to_owned()),
    );
    model.insert(
        "defaultModelProvider".to_owned(),
        Value::String(MANAGED_PROVIDER_ID.to_owned()),
    );

    let updated = serde_json::to_string(&root)
        .map_err(|_| CommandError::internal("无法生成 AionClaw 的模型配置"))?;
    connection
        .execute(
            "UPDATE kv SET value = ?1 WHERE key = ?2",
            params![updated, CONFIG_KEY],
        )
        .map_err(|error| {
            CommandError::new(
                "aionclaw_config_write_failed",
                format!("无法更新 AionClaw 的模型配置：{error}"),
            )
        })?;

    // 光把 Provider 列进目录不够：界面显示的当前模型来自 Agent 自身，
    // 不改这里就会停在"已添加但未选中"。
    let previous_agent_model = read_agent_model(&connection)?;
    connection
        .execute(
            "UPDATE agents SET model = ?1 WHERE is_default = 1 OR id = 'main'",
            params![agent_model_value(desired.model_id)],
        )
        .map_err(|error| {
            CommandError::new(
                "aionclaw_agent_model_write_failed",
                format!("无法更新 AionClaw 的当前模型：{error}"),
            )
        })?;

    Ok(ConfigChange {
        previous,
        previous_agent_model,
    })
}

/// Agent 的模型字段使用 `<provider>/<model>` 形式（实测 `custom_0/NMauto`）。
fn agent_model_value(model_id: &str) -> String {
    format!("{MANAGED_PROVIDER_ID}/{model_id}")
}

fn read_agent_model(connection: &Connection) -> AppResult<Option<String>> {
    connection
        .query_row(
            "SELECT model FROM agents WHERE is_default = 1 OR id = 'main' LIMIT 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(|error| {
            CommandError::new(
                "aionclaw_config_unreadable",
                format!("无法读取 AionClaw 的当前模型：{error}"),
            )
        })
}

/// 校验数据库里当前默认模型是否已指向受管 Provider。
pub(crate) fn verify_config(
    detection: &AgentDetection,
    desired: &DesiredAgentBinding<'_>,
) -> AppResult<()> {
    let path = database_path(detection)?;
    let connection = Connection::open(path).map_err(|error| {
        CommandError::new(
            "aionclaw_db_locked",
            format!("无法读取 AionClaw 的配置数据库：{error}"),
        )
    })?;
    let stored: String = connection
        .query_row("SELECT value FROM kv WHERE key = ?1", [CONFIG_KEY], |row| {
            row.get(0)
        })
        .map_err(|error| {
            CommandError::new(
                "aionclaw_config_unreadable",
                format!("无法读取 AionClaw 的模型配置：{error}"),
            )
        })?;
    let root: Value = serde_json::from_str(&stored).map_err(|error| {
        CommandError::new(
            "aionclaw_config_unparseable",
            format!("AionClaw 的模型配置不是有效的 JSON：{error}"),
        )
    })?;

    let provider = root
        .pointer("/model/defaultModelProvider")
        .and_then(Value::as_str);
    let model_id = root.pointer("/model/defaultModel").and_then(Value::as_str);
    let agent_model = read_agent_model(&connection)?;
    let expected = agent_model_value(desired.model_id);
    if provider == Some(MANAGED_PROVIDER_ID)
        && model_id == Some(desired.model_id)
        && agent_model.as_deref() == Some(expected.as_str())
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "aionclaw_write_verification_failed",
            "AionClaw 的默认模型与目标不一致",
        )
        .with_recovery("请重新点击目标模型的“切换”，AT-Switch 会重新写入并校验。"))
    }
}

/// 把配置回滚为写入前的原文。
pub(crate) fn restore_config(detection: &AgentDetection, change: &ConfigChange) -> AppResult<()> {
    let path = database_path(detection)?;
    let connection = Connection::open(path).map_err(|error| {
        CommandError::new(
            "aionclaw_db_locked",
            format!("无法写入 AionClaw 的配置数据库：{error}"),
        )
    })?;
    connection
        .execute(
            "UPDATE kv SET value = ?1 WHERE key = ?2",
            params![change.previous, CONFIG_KEY],
        )
        .map_err(|error| {
            CommandError::new(
                "aionclaw_restore_failed",
                format!("无法恢复 AionClaw 的模型配置：{error}"),
            )
        })?;
    connection
        .execute(
            "UPDATE agents SET model = ?1 WHERE is_default = 1 OR id = 'main'",
            params![change.previous_agent_model],
        )
        .map_err(|error| {
            CommandError::new(
                "aionclaw_restore_failed",
                format!("无法恢复 AionClaw 的当前模型：{error}"),
            )
        })?;
    Ok(())
}

/// `config_path` 是 `<root>/openclaw/state/openclaw.json`（事务用它承载 trace），
/// 真相源数据库在同级的 `<root>/aionclaw.sqlite`，因此由它反向推导。
fn database_path(detection: &AgentDetection) -> AppResult<PathBuf> {
    let config = detection.config_path.as_ref().ok_or_else(|| {
        CommandError::new("aionclaw_config_path_missing", "未找到 AionClaw 的配置路径")
    })?;
    let root = config
        .parent()
        .and_then(|state| state.parent())
        .and_then(|openclaw| openclaw.parent())
        .ok_or_else(|| {
            CommandError::new(
                "aionclaw_config_path_missing",
                "无法从配置路径推导 AionClaw 数据目录",
            )
        })?;
    Ok(root.join("aionclaw.sqlite"))
}

fn shape_error(key: &str) -> CommandError {
    CommandError::new(
        "aionclaw_config_shape_unsupported",
        format!("AionClaw 配置中的 {key} 不是对象"),
    )
}

/// AionClaw 的 `apiFormat` 取值（实测用户自建 Provider 使用 `openai`）。
fn aionclaw_api_format(protocol: crate::domain::ApiProtocol) -> &'static str {
    use crate::domain::ApiProtocol;
    match protocol {
        ApiProtocol::OpenaiChatCompletions => "openai",
        ApiProtocol::OpenaiResponses => "openai-responses",
        ApiProtocol::AnthropicMessages => "anthropic",
    }
}

#[cfg(test)]
#[path = "aionclaw_tests.rs"]
mod tests;
