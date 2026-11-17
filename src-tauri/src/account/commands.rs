use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::{
    account::{
        auth,
        client::ProviderPresetsSnapshot,
        telemetry::{ErrorEvent, Event},
        types::{AccountStatus, LoginPoll, LoginStart, StoredTokens},
        AccountService,
    },
    domain::{AppResult, CommandError},
    AppState,
};

/// 账号服务未注入（打包期未配置服务端地址）时的统一错误。
fn service(state: &State<'_, AppState>) -> AppResult<Arc<AccountService>> {
    state
        .account
        .clone()
        .ok_or_else(|| CommandError::new("account_service_not_configured", "会员服务未配置"))
}

#[tauri::command]
/// 供应商预设目录（匿名可拉）：供应商表单下拉的数据源。
/// account 未配置（服务端地址未注入）时返回空目录，前端回退内置预设。
pub async fn fetch_provider_presets(state: State<'_, AppState>) -> AppResult<ProviderPresetsSnapshot> {
    let Some(service) = state.account.as_deref() else {
        return Ok(ProviderPresetsSnapshot {
            items: Vec::new(),
            disabled_builtin_kinds: Vec::new(),
        });
    };
    Ok(service.api.provider_presets().await?)
}

#[tauri::command]
pub async fn account_get_status(state: State<'_, AppState>) -> AppResult<AccountStatus> {
    Ok(service(&state)?.status().await)
}

#[tauri::command]
pub async fn account_login_start(
    state: State<'_, AppState>,
    method: String,
) -> AppResult<LoginStart> {
    let service = service(&state)?;
    let device = service.device_context();
    match method.as_str() {
        "wechat" => {
            let (login_state, qr_url) = service.api.create_login_state(&device).await?;
            Ok(LoginStart {
                method,
                login_state,
                qr_url,
            })
        }
        "google" => {
            let login_state = uuid::Uuid::new_v4().to_string();
            let (verifier, challenge) = auth::pkce_pair();
            let authorize_url = service
                .api
                .google_authorize_url(&login_state, &challenge)
                .await?;
            let flow_service = Arc::clone(&service);
            let flow_state = login_state.clone();
            tauri::async_runtime::spawn(async move {
                let result = match auth::wait_for_google_code(&flow_state).await {
                    Ok(code) => {
                        flow_service
                            .api
                            .google_exchange(&code, &verifier, &device)
                            .await
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = &result {
                    flow_service.telemetry.record_error(ErrorEvent {
                        error_code: "AUTH_LOGIN_FAILED",
                        message: &error.message,
                        app_version: env!("CARGO_PKG_VERSION"),
                        context_tag: "login=google",
                        user_id: None,
                    });
                }
                let _ = &result;
                flow_service.google_registry.insert(&flow_state, result);
            });
            auth::open_browser(&authorize_url).inspect_err(|error| {
                service.telemetry.record_error(ErrorEvent {
                    error_code: "AUTH_LOGIN_FAILED",
                    message: &error.message,
                    app_version: env!("CARGO_PKG_VERSION"),
                    context_tag: "login=google/open_browser",
                    user_id: None,
                });
            })?;
            Ok(LoginStart {
                method,
                login_state,
                qr_url: None,
            })
        }
        _ => Err(CommandError::new("auth_login_failed", "不支持的登录方式")),
    }
}

#[tauri::command]
pub async fn account_login_poll(
    state: State<'_, AppState>,
    method: String,
    login_state: String,
) -> AppResult<LoginPoll> {
    let service = service(&state)?;
    match method.as_str() {
        "wechat" => match service.api.poll_login_state(&login_state).await? {
            super::client::LoginPollOutcome::Success(tokens) => {
                service.session.save(&tokens)?;
                // 登录成功：拉取云端供应商配置合入本地（换设备恢复）
                service.spawn_cloud_restore(state.providers.clone());
                Ok(success_poll(&tokens))
            }
            super::client::LoginPollOutcome::Expired => Ok(expired_poll()),
            super::client::LoginPollOutcome::Pending => Ok(pending_poll()),
        },
        "google" => match service.google_registry.take(&login_state) {
            Some(Ok(tokens)) => {
                service.session.save(&tokens)?;
                service.spawn_cloud_restore(state.providers.clone());
                Ok(success_poll(&tokens))
            }
            Some(Err(error)) => Ok(LoginPoll {
                status: "error".to_owned(),
                profile: None,
                error_code: Some(error.code),
                message: Some(error.message),
            }),
            None => Ok(pending_poll()),
        },
        _ => Err(CommandError::new("auth_login_failed", "不支持的登录方式")),
    }
}

fn pending_poll() -> LoginPoll {
    LoginPoll {
        status: "pending".to_owned(),
        profile: None,
        error_code: None,
        message: None,
    }
}

fn expired_poll() -> LoginPoll {
    LoginPoll {
        status: "expired".to_owned(),
        profile: None,
        error_code: None,
        message: None,
    }
}

fn success_poll(tokens: &StoredTokens) -> LoginPoll {
    LoginPoll {
        status: "success".to_owned(),
        profile: Some(tokens.profile.clone()),
        error_code: None,
        message: None,
    }
}

#[tauri::command]
pub async fn account_logout(state: State<'_, AppState>) -> AppResult<()> {
    let service = service(&state)?;
    if let Some(tokens) = service.session.load() {
        // 服务端吊销失败不阻断本地登出（离线也要能退出）。
        let _ = service
            .api
            .logout(&tokens.access_token, &tokens.refresh_token, &service.device_context())
            .await;
        service.telemetry.record_event(Event {
            event: "account_logout",
            app_version: env!("CARGO_PKG_VERSION"),
            target_id: "",
            model_id: "",
            provider_name: "",
            model_detail: "",
            proxy_mode: "",
            result: "success",
            user_id: Some(&tokens.profile.user_id),
        });
    }
    service.session.clear();
    Ok(())
}

#[tauri::command]
pub async fn submit_feedback(
    state: State<'_, AppState>,
    content: String,
    contact: Option<String>,
) -> AppResult<()> {
    let service = service(&state)?;
    let contact = contact.unwrap_or_default();
    // 反馈免登录（PRD 调整）：登录态附带 token 关联用户，匿名直接提交。
    let tokens = service.session.load();
    let access_token = tokens.as_ref().map(|t| t.access_token.clone());
    let claim_user_id = tokens.as_ref().map(|t| t.profile.user_id.clone());
    // 设备上下文与登录、埋点共用同一份，保证反馈同样具备设备/平台/版本公共字段。
    let device = service.device_context();
    service
        .api
        .feedback(
            access_token.as_deref(),
            claim_user_id.as_deref(),
            &content,
            &contact,
            &device,
        )
        .await
        .inspect_err(|error| {
            service.telemetry.record_error(ErrorEvent {
                error_code: "FEEDBACK_SUBMIT_FAILED",
                message: &error.message,
                app_version: env!("CARGO_PKG_VERSION"),
                context_tag: "feedback",
                user_id: claim_user_id.as_deref(),
            });
        })?;
    service.telemetry.record_event(Event {
        event: "feedback_submit",
        app_version: env!("CARGO_PKG_VERSION"),
        target_id: "",
        model_id: "",
        provider_name: "",
        model_detail: "",
        proxy_mode: "",
        result: "success",
        user_id: claim_user_id.as_deref(),
    });
    Ok(())
}

/// 使用明细自驱上报：启动后 45s 首报，之后每 15 分钟随快照刷新；
/// 内容签名去重（record_usage_snapshot），不依赖前端 bootstrap 行为。
pub fn spawn_usage_reporter(app: AppHandle) {
    let has_account = app.state::<AppState>().account.is_some();
    if !has_account {
        return;
    }
    tauri::async_runtime::spawn(async move {
        // 低频使用快照：6 小时一次（+ 启动首报、内容变化即报）
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
        interval.tick().await; // 首个 tick 立即返回，跳过
        tokio::time::sleep(std::time::Duration::from_secs(45)).await;
        loop {
            {
                let state = app.state::<AppState>();
                match state.agents.scan() {
                    Ok(agents) => {
                        let proxy = state.proxy.status().await;
                        crate::commands::record_usage_snapshot(&state, &agents, &proxy);
                    }
                    Err(error) => {
                        log::warn!("usage report scan failed: {}", error.message);
                    }
                }
            }
            interval.tick().await;
        }
    });
}

/// 周期校验任务（15 分钟）：冻结闭环的服务端到客户端传播；
/// 校验失败（离线）也作为卡点错误进入遥测。
pub fn spawn_periodic_check(app: AppHandle) {
    let service = match app.state::<AppState>().account.clone() {
        Some(service) => service,
        None => return,
    };
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(900));
        loop {
            interval.tick().await;
            let status = service.status().await;
            if status.frozen == Some(true) {
                log::info!("account frozen detected by periodic check");
            }
            if status.offline {
                service.telemetry.record_error(ErrorEvent {
                    error_code: "ACCOUNT_OFFLINE",
                    message: "periodic account check unreachable",
                    app_version: env!("CARGO_PKG_VERSION"),
                    context_tag: "periodic_check",
                    user_id: None,
                });
            }
        }
    });
}
