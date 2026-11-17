mod account;
mod agents;
mod commands;
mod domain;
mod infrastructure;
mod proxy;
mod services;

use std::sync::Arc;

use agents::AgentService;
use commands::{
    account_get_status, account_login_poll, account_login_start, account_logout,
    apply_agent_binding, bootstrap, check_update, delete_provider, fetch_provider_models, fetch_provider_presets,
    get_provider_api_key_mask, install_update, refresh_snapshot, relaunch_app,
    restore_agent_native, reveal_provider_api_key, save_provider, save_provider_verified,
    set_agent_install_path, spawn_periodic_check, start_proxy, stop_proxy, submit_feedback,
    test_provider, test_provider_draft, update_proxy_port, update_settings,
};
use domain::{AppResult, CommandError};
use infrastructure::{Database, NativeSecretStore, SecretStore};
use proxy::ProxySupervisor;
use services::ProviderService;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

pub struct AppState {
    database: Arc<Database>,
    providers: Arc<ProviderService>,
    agents: Arc<AgentService>,
    proxy: Arc<ProxySupervisor>,
    account: Option<Arc<account::AccountService>>,
    /// 使用明细上报去重签名：内容不变则不重复上报。
    usage_signature: std::sync::Mutex<Option<String>>,
}

impl AppState {
    fn initialize(app: &tauri::App) -> AppResult<Self> {
        let app_data = app.path().app_data_dir().map_err(|error| {
            log::error!("app data directory could not be resolved: {error}");
            CommandError::internal("无法确定应用数据目录")
        })?;
        std::fs::create_dir_all(&app_data)?;
        let database = Arc::new(Database::open(&app_data.join("at-switch.db"))?);
        // On non-macOS targets `NativeSecretStore` is a unit struct, so prefer
        // direct construction over `Default::default()` to satisfy clippy.
        #[cfg(target_os = "macos")]
        let secrets_store = NativeSecretStore::default();
        #[cfg(not(target_os = "macos"))]
        let secrets_store = NativeSecretStore;
        let secrets: Arc<dyn SecretStore> = Arc::new(secrets_store);
        let providers = Arc::new(ProviderService::new(
            Arc::clone(&database),
            Arc::clone(&secrets),
        )?);
        let proxy = ProxySupervisor::new(database.proxy_port()?, Arc::clone(&secrets))?;
        let agents = Arc::new(AgentService::new(
            Arc::clone(&database),
            Arc::clone(&secrets),
            app_data.join("agent-backups"),
            Arc::clone(&proxy),
        ));
        // 会员服务仅在打包期注入了服务端地址时存在；未配置时本地功能完全不受影响。
        let device_id_path = app_data.join("device-id");
        let account =
            account::AccountService::new(secrets, read_or_create_device_id(&device_id_path))
                .map(Arc::new);
        Ok(Self {
            database,
            providers,
            agents,
            proxy,
            account,
            usage_signature: std::sync::Mutex::new(None),
        })
    }
}

/// 设备号：优先取平台硬件标识（macOS=IOPlatformUUID，Windows=MachineGuid），
/// 取不到时回退随机 UUID。结果持久化到 device-id 文件，前缀 `hw:` 标记硬件来源。
fn read_or_create_device_id(path: &std::path::Path) -> String {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim().to_owned();
        if trimmed.starts_with("hw:") {
            return trimmed;
        }
    }
    let device_id = match platform_device_id() {
        Some(hardware_id) => format!("hw:{hardware_id}"),
        None => uuid::Uuid::new_v4().to_string(),
    };
    let _ = std::fs::write(path, &device_id);
    device_id
}

fn platform_device_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("\"IOPlatformUUID\" = ") {
                return Some(value.trim_matches('"').to_owned());
            }
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .find_map(|line| {
                let mut parts = line.split_whitespace();
                let name = parts.next()?;
                let value = parts.next()?.to_owned();
                (name.eq_ignore_ascii_case("MachineGuid")).then_some(value)
            })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = AppState::initialize(app)?;
            println!("[account] setup: account service present = {}", state.account.is_some());
            if let Some(account) = state.account.as_deref() {
                account.telemetry.record_event(account::telemetry::Event {
                    event: "app_start",
                    app_version: env!("CARGO_PKG_VERSION"),
                    target_id: "",
                    model_id: "",
                    provider_name: "",
                    model_detail: "",
                    proxy_mode: "",
                    result: "success",
                    user_id: account.session.load().map(|t| t.profile.user_id).as_deref(),
                });
                account.telemetry.spawn_flusher();
            }
            app.manage(state);
            spawn_periodic_check(app.handle().clone());
            commands::spawn_usage_reporter(app.handle().clone());
            install_tray(app)?;
            install_close_behavior(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            refresh_snapshot,
            set_agent_install_path,
            save_provider,
            save_provider_verified,
            fetch_provider_models,
            fetch_provider_presets,
            delete_provider,
            get_provider_api_key_mask,
            reveal_provider_api_key,
            test_provider,
            test_provider_draft,
            apply_agent_binding,
            restore_agent_native,
            start_proxy,
            stop_proxy,
            update_proxy_port,
            update_settings,
            account_get_status,
            account_login_start,
            account_login_poll,
            account_logout,
            submit_feedback,
            check_update,
            install_update,
            relaunch_app
        ])
        .run(tauri::generate_context!())
        .expect("AT-Switch failed to start");
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开 AT-Switch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("AT-Switch")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let proxy = Arc::clone(&app.state::<AppState>().proxy);
                    let _ = proxy.stop().await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn install_close_behavior(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let window_for_close = window.clone();
    let app_handle = app.handle().clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            let keep_running = app_handle
                .state::<AppState>()
                .database
                .settings()
                .map(|settings| settings.keep_running_in_background)
                .unwrap_or(true);
            if keep_running {
                api.prevent_close();
                let _ = window_for_close.hide();
            }
        }
    });
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
