mod agents;
mod commands;
mod domain;
mod infrastructure;
mod proxy;
mod services;

use std::sync::Arc;

use agents::AgentService;
use commands::{
    apply_agent_binding, bootstrap, delete_provider, get_provider_api_key_mask, refresh_snapshot,
    restore_agent_native, reveal_provider_api_key, save_provider, set_agent_install_path,
    start_proxy, stop_proxy, test_provider, update_proxy_port, update_settings,
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
            secrets,
            app_data.join("agent-backups"),
            Arc::clone(&proxy),
        ));
        Ok(Self {
            database,
            providers,
            agents,
            proxy,
        })
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
        .setup(|app| {
            let state = AppState::initialize(app)?;
            app.manage(state);
            install_tray(app)?;
            install_close_behavior(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            refresh_snapshot,
            set_agent_install_path,
            save_provider,
            delete_provider,
            get_provider_api_key_mask,
            reveal_provider_api_key,
            test_provider,
            apply_agent_binding,
            restore_agent_native,
            start_proxy,
            stop_proxy,
            update_proxy_port,
            update_settings
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
