use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    domain::{
        AgentBindingDraft, AgentSummary, AppResult, AppSettings, AppSnapshot, CommandError,
        ProviderDraft, ProviderSummary, ProxyStatus, SettingsPatch,
    },
    AppState,
};

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
    let affected = state
        .database
        .affected_agent_ids_for_provider(&provider_id)?;
    state.providers.delete(&provider_id)?;
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
    Ok(())
}

#[tauri::command]
pub fn save_provider(
    state: State<'_, AppState>,
    draft: ProviderDraft,
) -> AppResult<ProviderSummary> {
    state.providers.save(draft)
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
}

#[tauri::command]
pub async fn apply_agent_binding(
    state: State<'_, AppState>,
    draft: AgentBindingDraft,
) -> AppResult<AgentSummary> {
    state.agents.apply(draft).await
}

#[tauri::command]
pub async fn restore_agent_native(
    state: State<'_, AppState>,
    agent_id: String,
) -> AppResult<AgentSummary> {
    state.agents.restore_native(&agent_id).await
}

#[tauri::command]
pub async fn start_proxy(state: State<'_, AppState>) -> AppResult<ProxyStatus> {
    // Loading persisted routes is intentionally separate from starting the
    // listener. The listener only starts after this explicit user command.
    state.agents.restore_proxy_routes().await?;
    let port = state.database.proxy_port()?;
    state.proxy.start(port).await
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> AppResult<ProxyStatus> {
    state.proxy.stop().await
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
    Ok(AppSnapshot {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: std::env::consts::OS.to_owned(),
        providers: state.providers.list()?,
        agents: state.agents.scan()?,
        proxy: state.proxy.status().await,
        settings: state.database.settings()?,
    })
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
