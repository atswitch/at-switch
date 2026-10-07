//! ima keeps background processes alive after WM_CLOSE. Restart Manager asks
//! its browser process to save state and exit, without killing renderer trees.

use std::{os::windows::process::CommandExt, process::Command, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME},
    System::{
        RestartManager::{
            RmEndSession, RmRegisterResources, RmShutdown, RmStartSession, RM_UNIQUE_PROCESS,
        },
        Threading::{
            GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

use super::{
    process_error, wait_for_windows_process_exit, windows_process_ids, Installation,
    CREATE_NO_WINDOW,
};
use crate::domain::AppResult;

pub(super) fn stop(installation: &Installation) -> AppResult<bool> {
    if windows_process_ids(&installation.path, "ima")?.is_empty() {
        return Ok(false);
    }
    let fail = || {
        process_error(
            "agent_stop_failed",
            "ima",
            "无法安全退出，请从 ima 托盘菜单退出后重试",
        )
    };
    // Never use the generic image-name fallback for a shutdown request. Only
    // register the exact executable's browser process, excluding --type helpers.
    let expected = installation.path.canonicalize().map_err(|_| fail())?;
    let expected = expected.to_string_lossy();
    let literal = expected.trim_start_matches(r"\\?\").replace('\'', "''");
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"Name = 'ima.copilot.exe'\" | \
         Where-Object {{ $_.ExecutablePath -eq '{literal}' -and $_.CommandLine -and \
         $_.CommandLine -notmatch '--type=' }} | Select-Object -ExpandProperty ProcessId"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    if !output.status.success() || pids.len() != 1 {
        return Err(fail());
    }
    // Bind PID to creation time and recheck its executable before registering;
    // a recycled PID must never cause another application to be closed.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pids[0]) };
    if process.is_null() {
        return Err(fail());
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    let valid = unsafe {
        QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) != 0
            && GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0
    };
    unsafe {
        CloseHandle(process);
    }
    if !valid
        || !same_executable(
            &String::from_utf16_lossy(&path[..length as usize]),
            &expected,
        )
    {
        return Err(fail());
    }
    let application = RM_UNIQUE_PROCESS {
        dwProcessId: pids[0],
        ProcessStartTime: created,
    };
    let mut handle = 0;
    let mut key = [0u16; 33];
    if unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) } != 0 {
        return Err(fail());
    }
    struct Session(u32);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                RmEndSession(self.0);
            }
        }
    }
    let session = Session(handle);
    let registered =
        unsafe { RmRegisterResources(session.0, 0, ptr::null(), 1, &application, 0, ptr::null()) };
    if registered != 0 {
        return Err(fail());
    }
    // Flags=0 deliberately excludes RmForceShutdown. A refusal must leave the
    // configuration unchanged, rather than corrupting ima's session on disk.
    let result = unsafe { RmShutdown(session.0, 0, None) };
    if result != 0 {
        log::warn!("ima Restart Manager shutdown failed: code={result}");
        return Err(fail());
    }
    wait_for_windows_process_exit(&installation.path, "ima")?;
    Ok(true)
}

fn same_executable(actual: &str, expected: &str) -> bool {
    actual
        .trim_start_matches(r"\\?\")
        .eq_ignore_ascii_case(expected.trim_start_matches(r"\\?\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_identity_rejects_other_installations() {
        assert!(same_executable(
            r"\\?\C:\Apps\ima.copilot.exe",
            r"c:\apps\ima.copilot.exe"
        ));
        assert!(!same_executable(
            r"C:\Other\ima.copilot.exe",
            r"C:\Apps\ima.copilot.exe"
        ));
    }

    #[test]
    #[ignore = "requires explicit live Windows ima restart authorization"]
    fn ima_live_windows_clean_restart() {
        assert_eq!(
            std::env::var("AT_SWITCH_IMA_LIVE_RESTART").as_deref(),
            Ok("1")
        );
        let root =
            std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("local app data"))
                .join("ima.copilot");
        let installation = Installation {
            path: root.join("Application/ima.copilot.exe"),
            version: None,
            kind: super::super::InstallationKind::DesktopApp,
        };
        let stopped = stop(&installation).expect("clean shutdown");
        assert!(stopped, "ima must be running before the live test");
        let preferences = std::fs::read(root.join("User Data/Default/Preferences"))
            .ok()
            .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok());
        let clean = preferences
            .as_ref()
            .and_then(|v| v.pointer("/profile/exit_type"))
            .and_then(|v| v.as_str())
            .is_some_and(|kind| kind == "Normal" || kind == "SessionEnded");
        super::super::launch_desktop_app(&installation, "ima").expect("relaunch");
        assert!(clean, "ima must persist a clean session exit itself");
        println!("IMA_WINDOWS_CLEAN_RESTART=passed");
    }
}
