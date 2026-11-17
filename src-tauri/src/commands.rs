use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

pub use crate::account::commands::{
    account_get_status, account_login_poll, account_login_start, account_logout,
    fetch_provider_presets, spawn_periodic_check, spawn_usage_reporter, submit_feedback,
};
pub use crate::account::update::{check_update, install_update, relaunch_app};
use crate::{
    domain::{
        AgentBindingDraft, AgentSummary, ApiProtocol, AppResult, AppSettings, AppSnapshot,
        CommandError, ProviderDraft, ProviderSummary, ProxyStatus, RemoteModelSummary,
        SettingsPatch,
    },
    AppState,
};

/// 供应商变更 / 模型切换的遥测与元数据上报钩子（遥测匿名可报，元数据同步需登录）。
fn record_provider_change(
    state: &AppState,
    event: &str,
    target_id: &str,
    provider_id: &str,
    model_id: &str,
) {
    let Some(account) = state.account.clone() else {
        return;
    };
    let providers = state.providers.list().unwrap_or_default();
    let summary = providers.iter().find(|p| p.id == provider_id);
    let provider_name = summary.map(|p| p.name.as_str()).unwrap_or("");
    let model_detail = summary
        .map(|p| {
            p.models
                .iter()
                .map(|m| m.model_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let user_id = account.session.load().map(|tokens| tokens.profile.user_id);
    account
        .telemetry
        .record_event(crate::account::telemetry::Event {
            event,
            app_version: env!("CARGO_PKG_VERSION"),
            target_id,
            model_id,
            provider_name,
            model_detail: &model_detail,
            proxy_mode: "",
            result: "success",
            user_id: user_id.as_deref(),
        });
    if event != "agent_switch" {
        account.spawn_provider_sync(providers);
    }
}

/// 业务失败统一上报（智能体未找到、切换未生效、验证失败、代理启动失败等）。
fn record_command_failure(state: &AppState, context: &str, error: &CommandError) {
    let Some(account) = state.account.clone() else {
        return;
    };
    let user_id = account.session.load().map(|tokens| tokens.profile.user_id);
    account.telemetry.record_error(crate::account::telemetry::ErrorEvent {
        error_code: &error.code,
        message: &error.message,
        app_version: env!("CARGO_PKG_VERSION"),
        context_tag: context,
        user_id: user_id.as_deref(),
    });
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    snapshot(&state).await
}

#[tauri::command]
pub async fn refresh_snapshot(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    snapshot(&state).await
}

#[tauri::command]
pub fn set_agent_install_path(
    state: State<'_, AppState>,
    agent_id: String,
    path: Option<String>,
) -> AppResult<AgentSummary> {
    state
        .agents
        .set_custom_install_path(&agent_id, path.as_deref())
}

#[tauri::command]
pub async fn delete_provider(state: State<'_, AppState>, provider_id: String) -> AppResult<()> {
    let delete = || state.providers.delete(&provider_id);
    let affected = state
        .agents
        .delete_provider_with_service_restore(&provider_id, &delete)
        .await
        .inspect_err(|error| record_command_failure(&state, "delete_provider", error))?;
    for agent_id in affected {
        if let Err(error) = state
            .agents
            .restore_native_after_provider_deletion(&agent_id)
            .await
        {
            log::warn!(
                "Auto-restore native config for agent '{}' after provider deletion failed: {}",
                agent_id,
                error.message,
            );
        }
    }
    record_provider_change(&state, "provider_delete", &provider_id, &provider_id, "");
    Ok(())
}

#[tauri::command]
pub fn save_provider(
    state: State<'_, AppState>,
    draft: ProviderDraft,
) -> AppResult<ProviderSummary> {
    let saved = state.providers.save(draft)?;
    record_provider_change(&state, "provider_save", &saved.id, &saved.id, "");
    Ok(saved)
}

#[tauri::command]
pub fn get_provider_api_key_mask(
    state: State<'_, AppState>,
    provider_id: String,
) -> AppResult<String> {
    state.providers.masked_api_key(&provider_id)
}

#[tauri::command]
pub fn reveal_provider_api_key(
    state: State<'_, AppState>,
    provider_id: String,
) -> AppResult<String> {
    state.providers.reveal_api_key(&provider_id)
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: Option<String>,
) -> AppResult<ProviderSummary> {
    state
        .providers
        .test(&provider_id, model_id.as_deref())
        .await
        .inspect_err(|error| record_command_failure(&state, "provider_test", error))
}

/// 保存前用草稿值做连接测试（不落库）；失败时给出真实连接错误，阻止保存。
#[tauri::command]
pub async fn test_provider_draft(
    state: State<'_, AppState>,
    draft: ProviderDraft,
) -> AppResult<()> {
    state.providers.test_draft(&draft).await
}

/// 连接测试通过后保存，并把刚测试过的默认文本模型直接标记为已验证。
#[tauri::command]
pub async fn save_provider_verified(
    state: State<'_, AppState>,
    draft: ProviderDraft,
) -> AppResult<ProviderSummary> {
    state.providers.save_verified(draft).await
}

/// 拉取上游 GET {base}/models 的模型摘要（不落库）。
/// API Key 未填时，编辑场景回退到凭据库中的存量 Key。
#[tauri::command]
pub async fn fetch_provider_models(
    state: State<'_, AppState>,
    base_url: String,
    protocol: ApiProtocol,
    api_key: Option<String>,
    provider_id: Option<String>,
) -> AppResult<Vec<RemoteModelSummary>> {
    let api_key = match api_key {
        Some(key) if !key.trim().is_empty() => key,
        _ => {
            let provider_id = provider_id.ok_or_else(|| {
                CommandError::new("secret_missing", "请先填写 API Key")
                    .with_recovery("在供应商表单中填写 API Key 后再获取模型列表。")
            })?;
            state.providers.reveal_api_key(&provider_id)?
        }
    };
    state
        .providers
        .list_remote_models(&base_url, protocol, &api_key)
        .await
}

#[tauri::command]
pub async fn apply_agent_binding(
    state: State<'_, AppState>,
    draft: AgentBindingDraft,
    confirm_account_connection: Option<bool>,
) -> AppResult<AgentSummary> {
    let model_id = draft.model_id.clone();
    let provider_id = draft.provider_id.clone();
    let agent_id = draft.agent_id.clone();
    let summary = state
        .agents
        .apply_authorized(draft, confirm_account_connection.unwrap_or(false))
        .await
        .inspect_err(|error| record_command_failure(&state, "apply_binding", error))?;
    // 切换未生效检测：回读结果与请求不一致或配置健康度异常。
    let effective = summary.model_id.as_deref() == Some(model_id.as_str())
        && matches!(
            summary.config_health,
            crate::domain::AgentConfigHealth::Healthy
        );
    if !effective {
        record_command_failure(
            &state,
            "model_not_effective",
            &CommandError::new(
                "model_not_effective",
                format!(
                    "agent={} requested_model={} applied_model={:?} health={:?}",
                    agent_id, model_id, summary.model_id, summary.config_health
                ),
            ),
        );
    }
    record_provider_change(&state, "agent_switch", &summary.id, &provider_id, &model_id);
    Ok(summary)
}

#[tauri::command]
pub async fn restore_agent_native(
    state: State<'_, AppState>,
    agent_id: String,
    confirm_account_connection: Option<bool>,
) -> AppResult<AgentSummary> {
    state
        .agents
        .restore_native_authorized(&agent_id, confirm_account_connection.unwrap_or(false))
        .await
        .inspect_err(|error| record_command_failure(&state, "restore_native", error))
}

#[tauri::command]
pub async fn start_proxy(state: State<'_, AppState>) -> AppResult<ProxyStatus> {
    // Loading persisted routes is intentionally separate from starting the
    // listener. The listener only starts after this explicit user command.
    state.agents.restore_proxy_routes().await?;
    let port = state.database.proxy_port()?;
    let status = state.proxy.start(port).await.inspect_err(
        |error| record_command_failure(&state, "proxy_start", error),
    )?;
    record_proxy_event(&state, "proxy_start", port, _proxy_running(&status));
    Ok(status)
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> AppResult<ProxyStatus> {
    let status = state.proxy.stop().await?;
    record_proxy_event(&state, "proxy_stop", status.port, _proxy_running(&status));
    Ok(status)
}

fn record_proxy_event(state: &AppState, event: &str, port: u16, running: bool) {
    let Some(account) = state.account.clone() else {
        return;
    };
    let user_id = account.session.load().map(|tokens| tokens.profile.user_id);
    account.telemetry.record_event(crate::account::telemetry::Event {
        event,
        app_version: env!("CARGO_PKG_VERSION"),
        target_id: &port.to_string(),
        model_id: "",
        provider_name: "",
        model_detail: "",
        proxy_mode: if running { "proxy" } else { "stopped" },
        result: "success",
        user_id: user_id.as_deref(),
    });
}

#[allow(dead_code)]
fn _proxy_running(status: &ProxyStatus) -> bool {
    matches!(status.status, crate::domain::ProxyRuntimeStatus::Running)
}

#[tauri::command]
pub async fn update_proxy_port(state: State<'_, AppState>, port: u16) -> AppResult<ProxyStatus> {
    if port < 1024 {
        return Err(CommandError::new(
            "proxy_port_privileged",
            "代理端口必须在 1024–65535 之间",
        ));
    }
    let previous = state.database.proxy_port()?;
    let status = state.proxy.set_stopped_port(port).await?;
    if let Err(error) = state.database.update_proxy_port(port) {
        let _ = state.proxy.set_stopped_port(previous).await;
        return Err(error);
    }
    Ok(status)
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: SettingsPatch,
) -> AppResult<AppSettings> {
    let previous = state.database.settings()?;
    let mut next = previous.clone();
    if let Some(language) = settings.language {
        if !matches!(language.as_str(), "zh-CN" | "en") {
            return Err(CommandError::new(
                "language_invalid",
                "语言只能是 zh-CN 或 en",
            ));
        }
        next.language = language;
    }
    if let Some(theme) = settings.theme {
        if !matches!(theme.as_str(), "system" | "light" | "dark") {
            return Err(CommandError::new(
                "theme_invalid",
                "主题只能是 system、light 或 dark",
            ));
        }
        next.theme = theme;
    }
    if let Some(enabled) = settings.start_at_login {
        next.start_at_login = enabled;
    }
    if let Some(enabled) = settings.keep_running_in_background {
        next.keep_running_in_background = enabled;
    }

    if next.start_at_login != previous.start_at_login {
        set_autostart(&app, next.start_at_login)?;
    }
    if let Err(error) = state.database.save_settings(&next) {
        if next.start_at_login != previous.start_at_login {
            let _ = set_autostart(&app, previous.start_at_login);
        }
        return Err(error);
    }
    Ok(next)
}

async fn snapshot(state: &AppState) -> AppResult<AppSnapshot> {
    state.agents.restore_proxy_routes().await?;
    let agents = state.agents.scan()?;
    let proxy_status = state.proxy.status().await;
    // 使用明细（匿名上报，不登录也统计）：智能体 + 模型 + 代理模式；内容变化才上报。
    record_usage_snapshot(state, &agents, &proxy_status);
    Ok(AppSnapshot {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: std::env::consts::OS.to_owned(),
        providers: state.providers.list()?,
        agents,
        proxy: proxy_status,
        settings: state.database.settings()?,
    })
}

/// 使用明细：每个「已配置绑定」的智能体一条事件（agent + model + 代理模式）。
/// 以全部绑定内容做签名去重，避免刷新按钮反复上报。
pub(crate) fn record_usage_snapshot(
    state: &AppState,
    agents: &[AgentSummary],
    proxy_status: &ProxyStatus,
) {
    let Some(account) = state.account.clone() else {
        return;
    };
    let mut bound: Vec<String> = agents
        .iter()
        .filter(|agent| agent.provider_id.is_some() && agent.model_id.is_some())
        .map(|agent| {
            format!(
                "{}|{}|{}|{}|{}",
                agent.id,
                agent.model_id.as_deref().unwrap_or(""),
                agent.provider_name.as_deref().unwrap_or(""),
                agent.mode.as_deref().unwrap_or("direct"),
                agent.config_health.as_str(),
            )
        })
        .collect();
    bound.sort();
    let signature = format!(
        "{:?}|{}|{}",
        bound,
        _proxy_running(proxy_status),
        proxy_status.port
    );
    let mut last = state.usage_signature.lock().expect("usage guard poisoned");
    if *last == Some(signature.clone()) {
        return;
    }
    *last = Some(signature);
    println!("[usage] snapshot: bound agents = {}", bound.len());
    let user_id = account.session.load().map(|tokens| tokens.profile.user_id);
    for line in &bound {
        let mut parts = line.splitn(5, '|');
        let agent_id = parts.next().unwrap_or("");
        let model_id = parts.next().unwrap_or("");
        let _provider_name = parts.next().unwrap_or("");
        let mode = parts.next().unwrap_or("direct");
        let health = parts.next().unwrap_or("healthy");
        account.telemetry.record_event(crate::account::telemetry::Event {
            event: "usage_snapshot",
            app_version: env!("CARGO_PKG_VERSION"),
            target_id: agent_id,
            model_id,
            provider_name: _provider_name,
            model_detail: "",
            proxy_mode: mode,
            result: health,
            user_id: user_id.as_deref(),
        });
    }
    // 代理运行状态单独一条（未绑定智能体也可见）。
    account.telemetry.record_event(crate::account::telemetry::Event {
        event: "proxy_status",
        app_version: env!("CARGO_PKG_VERSION"),
        target_id: &proxy_status.port.to_string(),
        model_id: "",
        provider_name: "",
        model_detail: "",
        proxy_mode: if matches!(
            proxy_status.status,
            crate::domain::ProxyRuntimeStatus::Running
        ) {
            "proxy"
        } else {
            "stopped"
        },
        result: "success",
        user_id: user_id.as_deref(),
    });
}

fn set_autostart(app: &AppHandle, enabled: bool) -> AppResult<()> {
    let autostart = app.autolaunch();
    let result = if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    };
    result.map_err(|error| {
        log::warn!("start-at-login update failed: {error}");
        CommandError::new("autostart_update_failed", "无法更新“登录时启动”设置")
            .with_recovery("请检查系统权限，或在系统登录项中手动设置。")
    })
}

#[allow(dead_code)]
fn _assert_shared_state_is_thread_safe(_: Arc<AppState>) {}
