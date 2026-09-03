#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{process::Command, thread, time::Duration};

#[cfg(target_os = "macos")]
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

// CREATE_NO_WINDOW prevents spawned console subprocesses (taskkill, powershell,
// reg, ...) from flashing a black console window in the installed GUI build.
// The raw constant avoids pulling in extra windows-sys feature gates.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::domain::{AgentRuntimeStatus, AppResult, CommandError};

use super::{
    locator::{Installation, InstallationKind},
    AgentDetection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartOutcome {
    Relaunched,
    WasNotRunning,
    ManualRequired,
}

pub(crate) fn runtime_status(
    installation: &Installation,
    display_name: &str,
) -> AgentRuntimeStatus {
    if installation.kind == InstallationKind::Command {
        return AgentRuntimeStatus::Unknown;
    }
    match desktop_app_running(installation, display_name) {
        Ok(true) => AgentRuntimeStatus::Running,
        Ok(false) => AgentRuntimeStatus::NotRunning,
        Err(error) => {
            log::warn!(
                "unable to inspect {display_name} runtime status: {}",
                error.message
            );
            AgentRuntimeStatus::Unknown
        }
    }
}

#[cfg(target_os = "macos")]
fn desktop_app_running(installation: &Installation, display_name: &str) -> AppResult<bool> {
    let executable = macos_bundle_executable(&installation.path, display_name)?;
    let output = Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=,command="])
        .output()?;
    if !output.status.success() {
        return Err(process_error(
            "agent_process_scan_failed",
            display_name,
            "无法读取正在运行的进程",
        ));
    }
    let process_list = String::from_utf8_lossy(&output.stdout);
    let executable = executable.to_string_lossy();
    Ok(!macos_main_process_ids(&process_list, &executable).is_empty())
}

#[cfg(target_os = "windows")]
fn desktop_app_running(installation: &Installation, display_name: &str) -> AppResult<bool> {
    Ok(!windows_process_ids(&installation.path, display_name)?.is_empty())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn desktop_app_running(_installation: &Installation, _display_name: &str) -> AppResult<bool> {
    Ok(false)
}

/// Holds a desktop Agent closed while AT-Switch updates its configuration.
///
/// If an operation returns early, `Drop` reopens an Agent that AT-Switch
/// previously stopped. CLI installations are deliberately never terminated.
pub(crate) struct DesktopAppPause {
    installation: Option<Installation>,
    display_name: &'static str,
    was_running: bool,
    resumed: bool,
}

impl DesktopAppPause {
    pub(crate) fn resume(mut self) -> AppResult<RestartOutcome> {
        self.resumed = true;
        let Some(installation) = &self.installation else {
            return Ok(RestartOutcome::ManualRequired);
        };
        if self.was_running {
            launch_desktop_app(installation, self.display_name)?;
            Ok(RestartOutcome::Relaunched)
        } else {
            Ok(RestartOutcome::WasNotRunning)
        }
    }
}

impl Drop for DesktopAppPause {
    fn drop(&mut self) {
        if !self.was_running || self.resumed {
            return;
        }
        if let Some(installation) = &self.installation {
            if let Err(error) = launch_desktop_app(installation, self.display_name) {
                log::warn!(
                    "unable to relaunch {} after interrupted configuration: {}",
                    self.display_name,
                    error.message
                );
            }
        }
    }
}

pub(crate) fn pause_for_config_update(detection: &AgentDetection) -> AppResult<DesktopAppPause> {
    let installation = detection
        .installation
        .as_ref()
        .ok_or_else(|| {
            CommandError::new(
                "agent_not_installed",
                format!("未检测到 {}", detection.display_name),
            )
        })?
        .clone();

    if installation.kind == InstallationKind::Command {
        return Ok(DesktopAppPause {
            installation: None,
            display_name: detection.display_name,
            was_running: false,
            resumed: false,
        });
    }

    let was_running = stop_desktop_app_if_running(&installation, detection.display_name)?;
    Ok(DesktopAppPause {
        installation: Some(installation),
        display_name: detection.display_name,
        was_running,
        resumed: false,
    })
}

#[cfg(target_os = "macos")]
fn stop_desktop_app_if_running(installation: &Installation, display_name: &str) -> AppResult<bool> {
    let executable = macos_bundle_executable(&installation.path, display_name)?;
    let output = Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=,command="])
        .output()?;
    if !output.status.success() {
        return Err(process_error(
            "agent_process_scan_failed",
            display_name,
            "无法读取正在运行的进程",
        ));
    }
    let process_list = String::from_utf8_lossy(&output.stdout);
    let executable = executable.to_string_lossy();
    let pids = macos_main_process_ids(&process_list, &executable);
    if pids.is_empty() {
        return Ok(false);
    }

    for pid in &pids {
        let status = Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        if !status.success() {
            return Err(process_error(
                "agent_stop_failed",
                display_name,
                "无法安全退出",
            ));
        }
    }
    wait_for_macos_process_exit(&pids, display_name)?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn macos_bundle_executable(app_path: &Path, display_name: &str) -> AppResult<PathBuf> {
    let plist = plist::Value::from_file(app_path.join("Contents/Info.plist")).map_err(|error| {
        log::warn!("{display_name} bundle metadata could not be read: {error}");
        process_error(
            "agent_bundle_metadata_unavailable",
            display_name,
            "无法读取应用启动信息",
        )
    })?;
    let executable = plist
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleExecutable"))
        .and_then(plist::Value::as_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            process_error(
                "agent_bundle_executable_missing",
                display_name,
                "应用启动信息不完整",
            )
        })?;
    Ok(app_path.join("Contents/MacOS").join(executable))
}

#[cfg(target_os = "macos")]
fn macos_main_process_ids(process_list: &str, executable: &str) -> Vec<u32> {
    process_list
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let pid = parts.next()?.parse::<u32>().ok()?;
            let command = parts.next()?.trim_start();
            (command == executable).then_some(pid)
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn wait_for_macos_process_exit(pids: &[u32], display_name: &str) -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        let any_running = pids.iter().any(|pid| {
            Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .status()
                .is_ok_and(|status| status.success())
        });
        if !any_running {
            thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(process_error(
        "agent_stop_timeout",
        display_name,
        "等待退出超时",
    ))
}

/// Windows 桌面应用退出策略：先发 `WM_CLOSE`（`taskkill /T`，不带 `/F`）
/// 让应用有机会保存会话与清理资源；若优雅退出失败或超时仍未退出，再
/// fallback 到 `taskkill /T /F` 强制终止整棵进程树。
///
/// 之所以加 fallback，是因为不少桌面 Agent（Electron/Tauri/托盘常驻类）
/// 在 Windows 上对 `WM_CLOSE` 的响应不一致：有的只把窗口隐藏而不退出
/// 主进程，有的 helper 子进程拒绝关闭导致 `taskkill /T` 整体返回失败。
/// macOS 上 `kill -TERM` 对 Cocoa 应用是一等公民，几乎不会失败，因此
/// macOS 分支保持单次发送、不强制 fallback；Windows 必须有 fallback
/// 才能让用户的切换流程不被「无法安全退出」卡死，体验与 macOS 对齐。
#[cfg(target_os = "windows")]
fn stop_desktop_app_if_running(installation: &Installation, display_name: &str) -> AppResult<bool> {
    let pids = windows_process_ids(&installation.path, display_name)?;
    if pids.is_empty() {
        return Ok(false);
    }
    log::info!(
        "{display_name} detected running pids={:?}; requesting graceful shutdown",
        pids
    );
    // 第一阶段：优雅退出。`/T` 让主进程负责关闭它的子进程树。
    // Electron 类应用的 helper 子进程会拒绝 WM_CLOSE 导致返回 false，
    // 但部分子进程可能已被杀掉——800ms 等待后由第二阶段兜底。
    for pid in &pids {
        let _ = run_taskkill(pid, false, display_name);
    }
    thread::sleep(Duration::from_millis(800));
    // 第二阶段：强制终止仍在跑的进程。
    // `taskkill /F` 对已死 PID 返回 128 + GBK 编码的 stderr「没有找到进程」。
    // 不依赖 stderr 内容判断（GBK 在 UTF-8 环境下会乱码），而是：调完 /F 后
    // 等 1000ms 再扫描一次进程是否还在——如果不在了，说明 /F 已经成功
    // （或进程本来就死了），视为退出成功。
    //
    // 1000ms 而非 500ms：Electron 类应用在 /F 后，主进程的命名 mutex、
    // 单实例锁管道、SQLite/LevelDB 文件句柄等资源需要更长时间释放。
    // 实测 500ms 在 QClaw/CodeBuddy 这类 Electron 桌面应用上经常导致
    // 紧接着的 launch_desktop_app 因为单实例锁未释放而启动失败或立即退出。
    // 1000ms 在覆盖资源释放的同时不至于让用户感知明显延迟。
    let still_running: Vec<u32> = windows_process_ids(&installation.path, display_name)?
        .into_iter()
        .filter(|pid| pids.contains(pid))
        .collect();
    if !still_running.is_empty() {
        log::warn!(
            "{display_name} still running after graceful request (pids={:?}); forcing termination",
            still_running
        );
        for pid in &still_running {
            let _ = run_taskkill(pid, true, display_name);
        }
        // 等 1000ms 让 Windows 文件系统、单实例锁、句柄表收敛。
        thread::sleep(Duration::from_millis(1000));
        let final_check: Vec<u32> = windows_process_ids(&installation.path, display_name)?
            .into_iter()
            .filter(|pid| pids.contains(pid))
            .collect();
        if !final_check.is_empty() {
            // A normal AT-Switch process cannot terminate an elevated Agent.
            // AutoClaw currently requires administrator privileges, so retry
            // once through the trusted Windows UAC flow. Use the product image
            // name rather than interpolating PIDs into several prompts: one
            // elevated taskkill closes the complete Electron process group.
            let image_name = installation
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    process_error(
                        "agent_stop_failed",
                        display_name,
                        "无法确定需要结束的程序名称",
                    )
                })?;
            log::warn!(
                "{display_name} still running after normal force-terminate (pids={:?}); requesting elevated shutdown",
                final_check
            );
            shell_execute_elevated_command(
                &windows_system_executable("taskkill.exe"),
                Some(&format!("/IM \"{image_name}\" /T /F")),
                None,
                display_name,
                "退出",
            )?;
            thread::sleep(Duration::from_millis(1200));
            if !windows_process_ids(&installation.path, display_name)?.is_empty() {
                log::error!("{display_name} still running after elevated force-terminate");
                return Err(process_error(
                    "agent_stop_failed",
                    display_name,
                    "通过 Windows 权限确认后仍无法退出",
                ));
            }
        }
    }
    wait_for_windows_process_exit(&installation.path, display_name)?;
    Ok(true)
}

/// 包装 `taskkill` 调用，统一带 `CREATE_NO_WINDOW` 与错误诊断。
/// `force=true` 时附加 `/F` 走强制终止路径。
///
/// 返回值只是 taskkill 的退出码。不在此处判断"已死 PID"——因为
/// taskkill 在中文 Windows 上输出 GBK 编码的 stderr，UTF-8 环境下
/// 会乱码，无法可靠匹配中文关键词。调用方应在 /F 调完后重新扫描
/// 进程是否还在，以进程消失为成功判据。
#[cfg(target_os = "windows")]
fn run_taskkill(pid: &u32, force: bool, display_name: &str) -> AppResult<()> {
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &pid.to_string(), "/T"])
        .creation_flags(CREATE_NO_WINDOW);
    if force {
        command.arg("/F");
    }
    let output = command.output().map_err(|error| {
        log::error!("{display_name} taskkill spawn failed for pid={pid}: {error}");
        process_error(
            "agent_stop_failed",
            display_name,
            "无法自动退出，请手动结束后重试",
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        log::warn!(
            "{display_name} taskkill pid={pid} force={force} status={:?} stdout={:?} stderr={:?}",
            output.status,
            stdout.trim(),
            stderr.trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_process_ids(executable: &std::path::Path, _display_name: &str) -> AppResult<Vec<u32>> {
    // Process names are not unique on Windows. We previously compared the
    // process's on-disk path to the installed executable to avoid stopping a
    // similarly named app installed elsewhere. Several desktop apps (notably
    // AutoClaw) launch themselves with a technique that releases the process
    // module handle, after which both `Get-Process -Name X | Select Path` and
    // `Get-CimInstance Win32_Process | Select ExecutablePath` return $null.
    // Comparing on the missing path would silently match every copy of the
    // process running on the machine, or—worse—match none and skip the
    // graceful shutdown entirely.
    //
    // To stay correct we match on the image name and additionally verify that
    // the process command line contains the install directory we are managing.
    // The command line is populated even when the module handle is dropped
    // (the kernel records it for WMI / ETW). The image-name match keeps the
    // query scoped to the same process group (ms), and the command-line
    // substring filter prevents accidentally stopping a different product that
    // happens to ship an executable with the same basename.
    //
    // Performance: `Get-CimInstance Win32_Process` enumerates the full process
    // table (~200–500 processes on a typical workstation) in roughly
    // 100–300ms. We pipeline image name + command line so we make a single
    // WMI round-trip per detection call.
    let Some(process_name) = executable
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    else {
        return Ok(Vec::new());
    };
    let install_dir_token = match executable.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_string_lossy().into_owned(),
        _ => return Ok(Vec::new()),
    };
    // 用 format! 把 process_name / install_dir_token 注入到 PS 脚本里。
    // PS 单引号字符串不展开变量，但可以先用 format! 把字面量嵌入，再用 PS
    // 双引号字符串把它们拼到最终的 WQL filter 与 Where-Object 表达式里。
    // 这样路径里的非 ASCII 字符与单引号都被 Rust 侧正确转义，不会污染 PS。
    let name_literal = format!("'{}'", process_name.replace('\'', "''"));
    let dir_literal = format!("'{}'", install_dir_token.replace('\'', "''"));
    let script = format!(
        "$name = {name}; $dir = {dir}; \
         Get-CimInstance Win32_Process -Filter (\"Name = '\" + $name + \".exe'\") \
         | Where-Object {{ $_.CommandLine -and $_.CommandLine -like ('*' + $dir + '*') }} \
         | Select-Object -ExpandProperty ProcessId",
        name = name_literal,
        dir = dir_literal,
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect();

    if !pids.is_empty() {
        return Ok(pids);
    }

    // Final fallback: if CommandLine is also empty for some reason (e.g. the
    // installer launched the app via a stub that strips its own path), still
    // accept any process whose image name matches. The risk of stopping an
    // unrelated app with the same basename is bounded: AT-Switch only manages
    // desktop apps it has already detected via `AgentAdapter::detect`, whose
    // basenames are product-specific (`AutoClaw`, `Codex`, `Codebuddy`, …).
    // We log a warning so this path is visible in `at-switch --verbose`.
    // Elevated applications hide CommandLine/ExecutablePath from a normal
    // process. AutoClaw currently declares requireAdministrator, so CIM can
    // return access denied even though its main process is healthy. Get-Process
    // can still expose the PID and is sufficient for this product-specific
    // image-name fallback.
    let fallback_script = format!(
        "Get-Process -Name '{}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id",
        process_name.replace('\'', "''")
    );
    let fallback_output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &fallback_script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match fallback_output {
        Ok(output) if output.status.success() => {
            let fallback_pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .collect();
            if !fallback_pids.is_empty() {
                log::warn!(
                    "{} matched by image name only (CommandLine unavailable); using fallback",
                    process_name
                );
            }
            Ok(fallback_pids)
        }
        _ => Ok(Vec::new()),
    }
}

#[cfg(target_os = "windows")]
fn wait_for_windows_process_exit(
    executable: &std::path::Path,
    display_name: &str,
) -> AppResult<()> {
    for _ in 0..48 {
        if windows_process_ids(executable, display_name)?.is_empty() {
            thread::sleep(Duration::from_millis(250));
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(process_error(
        "agent_stop_timeout",
        display_name,
        "等待退出超时",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn stop_desktop_app_if_running(
    _installation: &Installation,
    _display_name: &str,
) -> AppResult<bool> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn launch_desktop_app(installation: &Installation, display_name: &str) -> AppResult<()> {
    Command::new("/usr/bin/open")
        .arg(&installation.path)
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            log::warn!("{display_name} could not be relaunched: {error}");
            CommandError::new(
                "agent_relaunch_failed",
                format!("{display_name} 配置已经保存，但未能自动重新打开"),
            )
            .with_recovery(format!("请手动打开 {display_name}；新配置已经保存。"))
        })
}

#[cfg(target_os = "windows")]
fn launch_desktop_app(installation: &Installation, display_name: &str) -> AppResult<()> {
    // Electron/Tauri 类桌面应用在 taskkill /F 后立即 spawn，常因单实例锁
    // （命名 mutex / 命名管道 / 共享内存）尚未释放而立即退出或拒绝启动。
    // 因此 spawn 后等 800ms 再用 windows_process_ids 验证进程是否真正
    // 起来——如果没起来，再 spawn 一次（第一次通常会失败，第二次锁已释放）。
    // 第二次还不行才报错，让前端走 ManualRequired 路径提示用户手动打开。
    //
    // 800ms 是经验值：Electron 主进程通常在 200~500ms 内完成初始化并接管
    // 单实例锁；少于这个值可能查到的是 spawn 时的瞬时进程；多于这个值会让
    // 用户感知明显延迟。两段式重试总成本约 1.6s，远小于"显示重启失败、
    // 用户手动打开"的体验成本。
    let working_directory = windows_launch_working_directory(&installation.path, display_name);
    for attempt in 0..2 {
        let mut command = Command::new(&installation.path);
        command.creation_flags(CREATE_NO_WINDOW);
        if let Some(directory) = &working_directory {
            command.current_dir(directory);
        }
        match command.spawn() {
            Ok(_) => {
                thread::sleep(Duration::from_millis(800));
                if !windows_process_ids(&installation.path, display_name)?.is_empty() {
                    return Ok(());
                }
                log::warn!(
                    "{display_name} spawn #{} succeeded but process not running after 800ms; \
                     likely killed by stale single-instance lock, retrying",
                    attempt + 1
                );
            }
            Err(error) => {
                log::warn!("{display_name} spawn #{} failed: {error}", attempt + 1);
                if error.raw_os_error() == Some(740) {
                    shell_execute_elevated(
                        &installation.path,
                        working_directory.as_deref(),
                        display_name,
                    )?;
                    thread::sleep(Duration::from_millis(1200));
                    if !windows_process_ids(&installation.path, display_name)?.is_empty() {
                        return Ok(());
                    }
                }
            }
        }
    }
    Err(CommandError::new(
        "agent_relaunch_failed",
        format!("{display_name} 配置已经保存，但未能自动重新打开"),
    )
    .with_recovery(format!("请手动打开 {display_name}；新配置已经保存。")))
}

#[cfg(target_os = "windows")]
fn windows_launch_working_directory(
    executable: &Path,
    display_name: &str,
) -> Option<std::path::PathBuf> {
    let install_dir = executable.parent()?;
    if display_name == "AutoClaw" {
        let gateway_dir = install_dir.join("resources/gateway/openclaw");
        if gateway_dir.is_dir() {
            return Some(gateway_dir);
        }
    }
    Some(install_dir.to_path_buf())
}

#[cfg(target_os = "windows")]
fn shell_execute_elevated(
    executable: &Path,
    working_directory: Option<&Path>,
    display_name: &str,
) -> AppResult<()> {
    shell_execute_elevated_command(
        executable,
        None,
        working_directory,
        display_name,
        "重新打开",
    )
}

#[cfg(target_os = "windows")]
fn windows_system_executable(name: &str) -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(name)
}

#[cfg(target_os = "windows")]
fn shell_execute_elevated_command(
    executable: &Path,
    parameters: Option<&str>,
    working_directory: Option<&Path>,
    display_name: &str,
    action: &str,
) -> AppResult<()> {
    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let verb = wide(OsStr::new("runas"));
    let executable = wide(executable.as_os_str());
    let parameters = parameters.map(|value| wide(OsStr::new(value)));
    let directory = working_directory.map(|path| wide(path.as_os_str()));
    // Safety: all strings are owned, NUL-terminated UTF-16 buffers and remain
    // alive for the duration of ShellExecuteW. HWND and parameters are null by
    // design. Windows displays its trusted UAC consent UI for `runas`.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            executable.as_ptr(),
            parameters
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            directory
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        return Ok(());
    }
    Err(CommandError::new(
        "agent_relaunch_failed",
        format!("未能通过 Windows 权限确认{action} {display_name}"),
    )
    .with_recovery("请在 Windows 权限确认框中选择“是”，然后重试。"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch_desktop_app(_installation: &Installation, _display_name: &str) -> AppResult<()> {
    Ok(())
}

fn process_error(code: &str, display_name: &str, detail: &str) -> CommandError {
    CommandError::new(code, format!("{display_name}：{detail}"))
        .with_recovery(format!("请手动完全退出 {display_name} 后重试。"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{AgentConfigHealth, AgentInstallStatus};

    #[test]
    fn command_installations_are_never_terminated_or_relaunched() {
        let detection = AgentDetection {
            id: "codex",
            display_name: "Codex",
            installation: Some(Installation {
                path: PathBuf::from("/usr/local/bin/codex"),
                version: Some("test".to_owned()),
                kind: InstallationKind::Command,
            }),
            config_path: Some(PathBuf::from("/tmp/config.toml")),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: true,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        };

        let pause = pause_for_config_update(&detection).expect("pause");
        assert_eq!(
            pause.resume().expect("resume"),
            RestartOutcome::ManualRequired
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_process_match_only_targets_the_exact_desktop_main_process() {
        let executable = "/Applications/Codex.app/Contents/MacOS/Codex";
        let process_list = format!(
            "  101 {executable}\n\
               102 {executable} --type=renderer\n\
               103 /usr/local/bin/codex app-server\n"
        );

        assert_eq!(macos_main_process_ids(&process_list, executable), vec![101]);
    }

    // 跨平台不变量：错误构造器必须携带稳定错误码和面向用户的恢复建议，
    // 这样前端才能据错误码展示一致的 Toast，而不是靠字符串匹配。
    #[test]
    fn process_error_carries_stable_code_and_recovery() {
        let error = process_error("agent_stop_failed", "WorkBuddy", "无法自动退出");
        assert_eq!(error.code, "agent_stop_failed");
        assert!(error.message.contains("WorkBuddy"));
        assert!(error.message.contains("无法自动退出"));
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|hint| hint.contains("WorkBuddy")),
            "恢复建议必须包含具体 Agent 名称"
        );
    }

    // Windows 退出策略：当目标进程已经不在运行时，stop 必须返回 Ok(false)
    // 而不是报错。这是两阶段 fallback 的前置不变量——不存在进程就没有
    // 「无法安全退出」可言。该测试在 Windows CI 上执行，用一组不存在的
    // 可执行路径触发 pids 为空分支。
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_stop_returns_false_when_no_process_running() {
        let installation = Installation {
            // 指向一个肯定不存在的可执行文件，windows_process_ids 应返回空。
            path: PathBuf::from("C:\\at-switch-test-nonexistent.exe"),
            version: None,
            kind: InstallationKind::DesktopApp,
        };
        let result = stop_desktop_app_if_running(&installation, "TestAgent");
        assert!(result.is_ok(), "无进程时不应报错: {:?}", result.err());
        assert!(!result.unwrap(), "无进程时应返回 false");
    }

    // windows_process_ids 用 `Get-Process -Name <basename>` 按进程名预过滤。
    // 该测试验证：对不存在的 image name，PowerShell 不报错（-ErrorAction
    // SilentlyContinue），stdout 为空，函数返回空 Vec。这是 stop/launch
    // 重试逻辑的前置不变量——找不到进程不能等同于"扫描失败"。
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_process_ids_returns_empty_for_unknown_executable() {
        let executable = PathBuf::from("C:\\at-switch-imaginary-agent-v9999.exe");
        let result = windows_process_ids(&executable, "ImaginaryAgent");
        assert!(
            result.is_ok(),
            "扫描不存在的进程不应报错: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "不存在的 image name 应返回空 PID 列表"
        );
    }

    // launch_desktop_app 现在做了 spawn 后 800ms 验证 + 一次重试。当目标
    // exe 根本不存在时，spawn 会立即返回 Err，重试一次仍 Err，最终返回
    // `agent_relaunch_failed` 错误码——这是前端走 ManualRequired 路径的
    // 判据。该测试在 Windows CI 上验证错误码和恢复建议都带 Agent 名称。
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_launch_desktop_app_reports_relaunch_failure_for_missing_executable() {
        let installation = Installation {
            path: PathBuf::from("C:\\at-switch-definitely-missing-v9999.exe"),
            version: None,
            kind: InstallationKind::DesktopApp,
        };
        let result = launch_desktop_app(&installation, "MissingAgent");
        let error = result.expect_err("缺失 exe 时应返回 relaunch 错误");
        assert_eq!(error.code, "agent_relaunch_failed");
        assert!(error.message.contains("MissingAgent"));
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|hint| hint.contains("MissingAgent")),
            "恢复建议必须包含具体 Agent 名称"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autoclaw_uses_the_gateway_working_directory_when_available() {
        let temp = tempfile::tempdir().expect("tempdir");
        let install_dir = temp.path().join("AutoClaw");
        let gateway_dir = install_dir.join("resources/gateway/openclaw");
        std::fs::create_dir_all(&gateway_dir).expect("gateway directory");
        let executable = install_dir.join("AutoClaw.exe");

        assert_eq!(
            windows_launch_working_directory(&executable, "AutoClaw"),
            Some(gateway_dir)
        );
        assert_eq!(
            windows_launch_working_directory(&executable, "QClaw"),
            Some(install_dir)
        );
    }
}
