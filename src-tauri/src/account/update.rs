use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::{
    account::config,
    domain::{AppResult, CommandError},
};

/// 更新检查结果：available=false 即「已是最新」或服务端未配置发布物。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

fn update_endpoints() -> AppResult<Vec<url::Url>> {
    let base = config::api_base()
        .ok_or_else(|| CommandError::new("account_service_not_configured", "会员服务未配置"))?;
    let endpoint = format!("{base}/v1/updates/{{{{target}}}}/{{{{arch}}}}");
    let url = url::Url::parse(&endpoint)
        .map_err(|_| CommandError::internal("更新服务地址无效（打包注入值）"))?;
    Ok(vec![url])
}

/// tauri-plugin-updater 的原始错误多为英文，这里映射为用户可读的中文提示。
fn friendly_update_error(error: &tauri_plugin_updater::Error) -> String {
    let raw = error.to_string();
    let lower = raw.to_lowercase();
    if raw.contains("secure protocol") || raw.contains("https") {
        "更新服务地址不是 HTTPS，已跳过（可联系管理员启用 HTTPS）".to_owned()
    } else if lower.contains("signature") || lower.contains("minisign") {
        // 下载/安装阶段签名或哈希不匹配（清单 version 与包内版本不一致也会落到这里）。
        "更新包校验失败（签名或版本不一致），已放弃安装".to_owned()
    } else if lower.contains("network")
        || lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("proxy")
    {
        "无法连接更新服务，请检查网络".to_owned()
    } else if lower.contains("release json") || lower.contains("deserialize") {
        // 代理拦截、非 JSON 响应、清单字段缺失等都会走到这里。
        "更新服务返回内容异常（请检查网络代理或稍后重试）".to_owned()
    } else {
        format!("检查更新失败：{raw}")
    }
}

fn build_updater(app: &AppHandle) -> AppResult<tauri_plugin_updater::Updater> {
    let endpoints = update_endpoints()?;
    app.updater_builder()
        .endpoints(endpoints)
        .map_err(|error| {
            CommandError::new("update_check_failed", format!("更新服务配置无效：{error}"))
        })?
        // updater 内部的 reqwest 默认读取系统/环境代理，会把 127.0.0.1 回环地址也交给
        // 本机代理并收到 502，进而误报 "Could not fetch a valid release JSON"。
        // 注意：插件在 check() 内部自行 `ClientBuilder::new()` 建请求，只有 UpdaterBuilder
        // 的 no_proxy 标志位为真时才会对该请求调用 no_proxy()；configure_client 是后置叠加，
        // 无法清掉已经读取的系统代理。两处都必须设置。
        .no_proxy()
        .configure_client(|builder| builder.no_proxy())
        .build()
        .map_err(|error| {
            CommandError::new("update_check_failed", format!("更新服务配置无效：{error}"))
        })
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> AppResult<UpdateCheck> {
    let current = app.package_info().version.to_string();
    let update = build_updater(&app)?.check().await.map_err(|error| {
        log::warn!("update check failed: {error}");
        CommandError::new("update_check_failed", friendly_update_error(&error))
            .with_recovery("请稍后重试；若持续失败请反馈给管理员。")
    })?;
    Ok(match update {
        Some(update) => UpdateCheck {
            current_version: current,
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
        },
        None => UpdateCheck {
            current_version: current,
            available: false,
            version: None,
            notes: None,
        },
    })
}

/// 下载并安装更新，进度经 `update://progress` 事件推送前端。
/// 失败保留当前版本可用（不阻断主流程）。
#[tauri::command]
pub async fn install_update(app: AppHandle) -> AppResult<()> {
    let update = build_updater(&app)?
        .check()
        .await
        .map_err(|error| {
            CommandError::new("update_check_failed", format!("检查更新失败：{error}"))
        })?
        .ok_or_else(|| CommandError::new("update_unavailable", "当前已是最新版本"))?;

    let app_handle = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app_handle.emit(
                    "update://progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {
                log::info!("update downloaded, installing");
            },
        )
        .await
        .map_err(|error| {
            log::warn!("update install failed: {error}");
            CommandError::new("update_download_failed", format!("下载/安装失败：{error}"))
                .with_recovery("当前版本仍可正常使用，请稍后重试。")
        })?;
    // 下载安装完成：提示「重启后生效」，由用户点「立即重启」触发 relaunch_app。
    let _ = app_handle.emit(
        "update://ready",
        serde_json::json!({ "version": update.version }),
    );
    Ok(())
}

#[tauri::command]
pub async fn relaunch_app(app: AppHandle) -> AppResult<()> {
    // restart() 永不返回（! 会收敛到 AppResult）
    app.restart()
}
