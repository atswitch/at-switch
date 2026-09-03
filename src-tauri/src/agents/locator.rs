use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// CREATE_NO_WINDOW keeps reg.exe / powershell.exe discovery subprocesses from
// flashing a console window in the installed GUI build.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct Installation {
    pub path: PathBuf,
    pub version: Option<String>,
    pub kind: InstallationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationKind {
    DesktopApp,
    Command,
}

/// Resolves platform paths at runtime instead of baking the developer's home
/// directory into the application. Tests can inject an isolated context.
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub home: PathBuf,
    /// Per-user application-data root used by Electron applications.
    /// macOS maps this to `~/Library/Application Support`, while Windows maps
    /// it to `%APPDATA%`. Keeping it in the injectable context makes adapter
    /// path resolution identical and testable on both platforms.
    pub application_data_dir: PathBuf,
    /// macOS application directories searched by `locate_mac_desktop_app`.
    /// Read only on macOS; suppressed on other platforms to avoid false-positive
    /// dead-code warnings from clippy.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub application_dirs: Vec<PathBuf>,
    pub path_entries: Vec<PathBuf>,
    /// Allows native-only discovery through Spotlight and the process table.
    /// Tests disable this so injected paths remain hermetic.
    pub system_application_search: bool,
    /// User-selected installation file or directory for the adapter currently
    /// being detected. Native scans set this per adapter from the local database.
    pub custom_installation_path: Option<PathBuf>,
    #[cfg(target_os = "windows")]
    pub local_app_data: Option<PathBuf>,
    #[cfg(target_os = "windows")]
    pub program_files: Vec<PathBuf>,
}

impl DiscoveryContext {
    pub fn native() -> Self {
        #[cfg(target_os = "windows")]
        let home = env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default();

        #[cfg(not(target_os = "windows"))]
        let home = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default();
        let path_entries = env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default();

        #[cfg(target_os = "macos")]
        let application_data_dir = home.join("Library/Application Support");

        #[cfg(target_os = "windows")]
        let application_data_dir = env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"));

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let application_data_dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));

        #[cfg(target_os = "macos")]
        let application_dirs = vec![PathBuf::from("/Applications"), home.join("Applications")];

        #[cfg(target_os = "windows")]
        let application_dirs = Vec::new();

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let application_dirs = Vec::new();

        Self {
            home,
            application_data_dir,
            application_dirs,
            path_entries,
            system_application_search: true,
            custom_installation_path: None,
            #[cfg(target_os = "windows")]
            local_app_data: env::var_os("LOCALAPPDATA").map(PathBuf::from),
            #[cfg(target_os = "windows")]
            program_files: [
                env::var_os("ProgramFiles"),
                env::var_os("ProgramFiles(x86)"),
            ]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .collect(),
        }
    }

    pub fn refreshed(&self) -> Self {
        if self.system_application_search {
            Self::native()
        } else {
            self.clone()
        }
    }
}

pub fn locate_desktop_app(
    context: &DiscoveryContext,
    _mac_app_names: &[&str],
    _mac_bundle_identifiers: &[&str],
    _windows_relative_paths: &[&str],
) -> Option<Installation> {
    #[cfg(target_os = "macos")]
    {
        // A user-selected location is authoritative when automatic discovery
        // cannot see an app installed outside indexed application folders. The
        // picker may return either the .app bundle itself or its parent folder;
        // keep the lookup bounded to that folder's immediate children.
        if let Some(path) = context.custom_installation_path.as_deref() {
            if let Some(installation) =
                macos_custom_installation_candidate(path, _mac_app_names, _mac_bundle_identifiers)
            {
                return Some(installation);
            }
        }

        // Fast path for conventional installations.
        for root in &context.application_dirs {
            for name in _mac_app_names {
                let path = root.join(name);
                if path.is_dir() {
                    return Some(Installation {
                        version: mac_bundle_version(&path),
                        path,
                        kind: InstallationKind::DesktopApp,
                    });
                }
            }
        }

        // Users can rename an app bundle while keeping it under an Applications
        // directory. Bundle identity is authoritative in that case.
        for root in &context.application_dirs {
            let Ok(entries) = fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(installation) = macos_installation_if_matching(
                    &entry.path(),
                    _mac_app_names,
                    _mac_bundle_identifiers,
                ) {
                    return Some(installation);
                }
            }
        }

        if context.system_application_search {
            // Spotlight covers apps installed in Downloads, custom folders and
            // mounted volumes. The process table provides a reliable fallback
            // for a running app when Spotlight indexing is disabled or stale.
            for path in macos_spotlight_app_candidates(_mac_bundle_identifiers)
                .into_iter()
                .chain(macos_running_app_candidates())
            {
                if let Some(installation) =
                    macos_installation_if_matching(&path, _mac_app_names, _mac_bundle_identifiers)
                {
                    return Some(installation);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = context.custom_installation_path.as_deref() {
            if let Some(path) = windows_custom_installation_candidate(path, _windows_relative_paths)
            {
                return Some(Installation {
                    version: windows_file_version(&path),
                    path,
                    kind: InstallationKind::DesktopApp,
                });
            }
        }
        let mut roots = context.program_files.clone();
        let local_app_data = context
            .local_app_data
            .clone()
            .unwrap_or_else(|| context.home.join("AppData/Local"));
        roots.insert(0, local_app_data);
        for root in roots {
            for relative in _windows_relative_paths {
                let path = root.join(relative);
                if path.is_file() {
                    return Some(Installation {
                        version: windows_file_version(&path),
                        path,
                        kind: InstallationKind::DesktopApp,
                    });
                }
                if let Some(path) = windows_versioned_installation_candidate(&path) {
                    return Some(Installation {
                        version: windows_file_version(&path),
                        path,
                        kind: InstallationKind::DesktopApp,
                    });
                }
            }
        }
        for root in &context.path_entries {
            for relative in _windows_relative_paths {
                let Some(file_name) = Path::new(relative).file_name() else {
                    continue;
                };
                let path = root.join(file_name);
                if path.is_file() {
                    return Some(Installation {
                        version: windows_file_version(&path),
                        path,
                        kind: InstallationKind::DesktopApp,
                    });
                }
            }
        }
        if context.system_application_search {
            let executable_names = _windows_relative_paths
                .iter()
                .filter_map(|relative| Path::new(relative).file_name())
                .filter_map(|name| name.to_str())
                .collect::<Vec<_>>();
            for path in windows_app_path_candidates(&executable_names)
                .into_iter()
                .chain(windows_running_app_candidates(&executable_names))
            {
                if path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            executable_names
                                .iter()
                                .any(|candidate| name.eq_ignore_ascii_case(candidate))
                        })
                {
                    return Some(Installation {
                        version: windows_file_version(&path),
                        path,
                        kind: InstallationKind::DesktopApp,
                    });
                }
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (context, _mac_app_names, _windows_relative_paths);
    }

    None
}

#[cfg(target_os = "macos")]
fn macos_custom_installation_candidate(
    selected_path: &Path,
    app_names: &[&str],
    bundle_identifiers: &[&str],
) -> Option<Installation> {
    if let Some(installation) =
        macos_installation_if_matching(selected_path, app_names, bundle_identifiers)
    {
        return Some(installation);
    }
    if !selected_path.is_dir() {
        return None;
    }

    for app_name in app_names {
        let path = selected_path.join(app_name);
        if let Some(installation) =
            macos_installation_if_matching(&path, app_names, bundle_identifiers)
        {
            return Some(installation);
        }
    }

    let mut children = fs::read_dir(selected_path)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then_some(entry.path()))
        .collect::<Vec<_>>();
    children.sort();
    children
        .into_iter()
        .find_map(|path| macos_installation_if_matching(&path, app_names, bundle_identifiers))
}

#[cfg(target_os = "windows")]
fn windows_custom_installation_candidate(
    selected_path: &Path,
    relative_paths: &[&str],
) -> Option<PathBuf> {
    let executable_names = relative_paths
        .iter()
        .filter_map(|relative| Path::new(relative).file_name())
        .collect::<Vec<_>>();
    if selected_path.is_file() {
        return selected_path
            .file_name()
            .is_some_and(|name| {
                executable_names
                    .iter()
                    .any(|candidate| name.eq_ignore_ascii_case(candidate))
            })
            .then(|| selected_path.to_path_buf());
    }
    if !selected_path.is_dir() {
        return None;
    }
    for executable_name in &executable_names {
        let path = selected_path.join(executable_name);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut directories = fs::read_dir(selected_path)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then_some(entry.path()))
        .collect::<Vec<_>>();
    directories.sort();
    for directory in directories {
        for executable_name in &executable_names {
            let path = directory.join(executable_name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_versioned_installation_candidate(expected_path: &Path) -> Option<PathBuf> {
    let parent = expected_path.parent()?;
    let executable_name = expected_path.file_name()?;
    let mut candidates = fs::read_dir(parent)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path().join(executable_name);
            path.is_file().then_some(path)
        })
        .collect::<Vec<_>>();
    // Installers commonly retain an older version directory during an upgrade.
    // Prefer the numerically newest version while keeping discovery bounded to one level.
    candidates.sort_by_key(|path| {
        std::cmp::Reverse(
            path.parent()
                .and_then(Path::file_name)
                .map(windows_version_directory_key)
                .unwrap_or_default(),
        )
    });
    candidates.into_iter().next()
}

#[cfg(target_os = "windows")]
fn windows_version_directory_key(name: &std::ffi::OsStr) -> Vec<u64> {
    name.to_string_lossy()
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or_default())
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_installation_if_matching(
    path: &Path,
    app_names: &[&str],
    bundle_identifiers: &[&str],
) -> Option<Installation> {
    if !path.is_dir() {
        return None;
    }
    let name_matches = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| app_names.contains(&name));
    let bundle_matches = mac_bundle_identifier(path).is_some_and(|identifier| {
        bundle_identifiers
            .iter()
            .any(|candidate| *candidate == identifier)
    });
    if !name_matches && !bundle_matches {
        return None;
    }
    Some(Installation {
        version: mac_bundle_version(path),
        path: path.to_path_buf(),
        kind: InstallationKind::DesktopApp,
    })
}

#[cfg(target_os = "macos")]
fn macos_spotlight_app_candidates(bundle_identifiers: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for identifier in bundle_identifiers {
        let Ok(output) = Command::new("/usr/bin/mdfind")
            .arg(format!("kMDItemCFBundleIdentifier == '{identifier}'"))
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = PathBuf::from(line.trim());
            if !line.trim().is_empty() && !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    candidates
}

#[cfg(target_os = "macos")]
fn macos_running_app_candidates() -> Vec<PathBuf> {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-ax", "-o", "command="])
        .output()
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for command in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = macos_app_path_from_process_command(command) else {
            continue;
        };
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    candidates
}

#[cfg(target_os = "macos")]
fn macos_app_path_from_process_command(command: &str) -> Option<PathBuf> {
    const MARKER: &str = ".app/Contents/MacOS/";
    let start = command.find('/')?;
    let marker = command[start..].find(MARKER)? + start;
    let end = marker + ".app".len();
    Some(PathBuf::from(command[start..end].trim_matches('"')))
}

#[cfg(target_os = "windows")]
fn windows_app_path_candidates(executable_names: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for executable_name in executable_names {
        for hive in ["HKCU", "HKLM"] {
            let key = format!(
                r"{hive}\Software\Microsoft\Windows\CurrentVersion\App Paths\{executable_name}"
            );
            let Ok(output) = Command::new("reg.exe")
                .args(["query", &key, "/ve"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            else {
                continue;
            };
            if !output.status.success() {
                continue;
            }
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let Some((_, value)) = line
                    .split_once("REG_SZ")
                    .or_else(|| line.split_once("REG_EXPAND_SZ"))
                else {
                    continue;
                };
                let path = PathBuf::from(value.trim().trim_matches('"'));
                if !candidates.contains(&path) {
                    candidates.push(path);
                }
            }
        }
    }
    candidates
}

#[cfg(target_os = "windows")]
fn windows_running_app_candidates(executable_names: &[&str]) -> Vec<PathBuf> {
    let process_names = executable_names
        .iter()
        .filter_map(|name| Path::new(name).file_stem())
        .filter_map(|name| name.to_str())
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(",");
    if process_names.is_empty() {
        return Vec::new();
    }
    let script = format!(
        "Get-Process -Name {process_names} -ErrorAction SilentlyContinue | \
         Select-Object -ExpandProperty Path -Unique"
    );
    let Ok(output) = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn locate_command(context: &DiscoveryContext, names: &[&str]) -> Option<Installation> {
    for root in &context.path_entries {
        for name in names {
            let path = root.join(name);
            if is_executable_candidate(&path) {
                return Some(Installation {
                    version: npm_package_version(&path),
                    path,
                    kind: InstallationKind::Command,
                });
            }
        }
    }

    let mut candidates = vec![
        context.home.join(".local/bin/codex"),
        context.home.join(".cargo/bin/codex"),
        context.home.join(".npm/bin/codex"),
    ];
    #[cfg(target_os = "windows")]
    {
        candidates.extend([
            context.home.join("AppData/Roaming/npm/codex.cmd"),
            context.home.join("AppData/Roaming/npm/codex.exe"),
        ]);
    }
    candidates.extend(nvm_codex_candidates(&context.home));

    candidates.into_iter().find_map(|path| {
        is_executable_candidate(&path).then(|| Installation {
            version: npm_package_version(&path),
            path,
            kind: InstallationKind::Command,
        })
    })
}

fn is_executable_candidate(path: &Path) -> bool {
    path.is_file() || path.is_symlink()
}

fn nvm_codex_candidates(home: &Path) -> Vec<PathBuf> {
    let versions = home.join(".nvm/versions/node");
    let Ok(entries) = fs::read_dir(versions) else {
        return Vec::new();
    };
    let mut candidates = entries
        .flatten()
        .map(|entry| {
            let version = nvm_version_key(&entry.file_name().to_string_lossy());
            #[cfg(target_os = "windows")]
            {
                (version, entry.path().join("codex.cmd"))
            }
            #[cfg(not(target_os = "windows"))]
            {
                (version, entry.path().join("bin/codex"))
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    candidates.into_iter().map(|(_, path)| path).collect()
}

fn nvm_version_key(value: &str) -> (u64, u64, u64) {
    let mut parts = value.trim_start_matches('v').split('.');
    (
        parts.next().and_then(|part| part.parse().ok()).unwrap_or(0),
        parts.next().and_then(|part| part.parse().ok()).unwrap_or(0),
        parts.next().and_then(|part| part.parse().ok()).unwrap_or(0),
    )
}

fn npm_package_version(command: &Path) -> Option<String> {
    let resolved = fs::canonicalize(command).ok()?;
    for ancestor in resolved.ancestors().take(8) {
        let package_path = ancestor.join("package.json");
        let Ok(bytes) = fs::read(&package_path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if value.get("name").and_then(|value| value.as_str()) == Some("@openai/codex") {
            return value
                .get("version")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn mac_bundle_version(app_path: &Path) -> Option<String> {
    let value = plist::Value::from_file(app_path.join("Contents/Info.plist")).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleShortVersionString")
        .and_then(plist::Value::as_string)
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "macos")]
fn mac_bundle_identifier(app_path: &Path) -> Option<String> {
    let value = plist::Value::from_file(app_path.join("Contents/Info.plist")).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "windows")]
pub(super) fn windows_file_version(path: &Path) -> Option<String> {
    use std::{ffi::c_void, iter, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), ptr::null_mut()) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0_u8; size as usize];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buffer.as_mut_ptr().cast::<c_void>()) }
        == 0
    {
        return None;
    }

    let root = ['\\' as u16, 0];
    let mut value_ptr: *mut c_void = ptr::null_mut();
    let mut value_len = 0_u32;
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast::<c_void>(),
            root.as_ptr(),
            &mut value_ptr,
            &mut value_len,
        )
    } == 0
        || value_ptr.is_null()
        || value_len < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32
    {
        return None;
    }
    let info = unsafe { &*value_ptr.cast::<VS_FIXEDFILEINFO>() };
    if info.dwSignature != 0xFEEF04BD {
        return None;
    }
    Some(format!(
        "{}.{}.{}.{}",
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
        info.dwFileVersionLS & 0xffff
    ))
}

pub fn normalized_path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lookup_uses_injected_path_without_spawning_processes() {
        let temp = tempfile::tempdir().expect("temp");
        let command_name = if cfg!(target_os = "windows") {
            "codex.cmd"
        } else {
            "codex"
        };
        fs::write(temp.path().join(command_name), b"placeholder").expect("command");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("application-data"),
            application_dirs: Vec::new(),
            path_entries: vec![temp.path().to_path_buf()],
            system_application_search: false,
            custom_installation_path: None,
            #[cfg(target_os = "windows")]
            local_app_data: None,
            #[cfg(target_os = "windows")]
            program_files: Vec::new(),
        };
        let installation = locate_command(&context, &[command_name]).expect("installed");
        assert_eq!(installation.path, temp.path().join(command_name));
        assert_eq!(installation.kind, InstallationKind::Command);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_lookup_accepts_a_renamed_bundle_with_the_expected_identifier() {
        let temp = tempfile::tempdir().expect("temp");
        let applications = temp.path().join("Applications");
        let app = applications.join("Renamed Work App.app");
        fs::create_dir_all(app.join("Contents")).expect("app bundle");
        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".to_owned(),
            plist::Value::String("com.workbuddy.workbuddy".to_owned()),
        );
        dictionary.insert(
            "CFBundleShortVersionString".to_owned(),
            plist::Value::String("5.3.5".to_owned()),
        );
        plist::to_file_xml(
            app.join("Contents/Info.plist"),
            &plist::Value::Dictionary(dictionary),
        )
        .expect("plist");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("application-data"),
            application_dirs: vec![applications],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
        };

        let installation = locate_desktop_app(
            &context,
            &["WorkBuddy.app"],
            &["com.workbuddy.workbuddy"],
            &[],
        )
        .expect("installed");

        assert_eq!(installation.path, app);
        assert_eq!(installation.version.as_deref(), Some("5.3.5"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn running_process_path_supports_apps_outside_applications() {
        let command =
            "/Users/test/Downloads/Work Buddy.app/Contents/MacOS/Electron --type=renderer";
        assert_eq!(
            macos_app_path_from_process_command(command),
            Some(PathBuf::from("/Users/test/Downloads/Work Buddy.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_lookup_prefers_a_user_selected_macos_application() {
        let temp = tempfile::tempdir().expect("temp");
        let selected_directory = temp.path().join("custom");
        let custom_app = selected_directory.join("WorkBuddy.app");
        let standard_directory = temp.path().join("Applications");
        let standard_app = standard_directory.join("WorkBuddy.app");
        fs::create_dir_all(custom_app.join("Contents")).expect("custom app bundle");
        fs::create_dir_all(standard_app.join("Contents")).expect("standard app bundle");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("application-data"),
            application_dirs: vec![standard_directory],
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: Some(selected_directory),
        };

        let installation = locate_desktop_app(
            &context,
            &["WorkBuddy.app"],
            &["com.workbuddy.workbuddy"],
            &[],
        )
        .expect("installed");

        assert_eq!(installation.path, custom_app);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn custom_macos_installation_scan_is_bounded_to_one_directory_level() {
        let temp = tempfile::tempdir().expect("temp");
        let selected_directory = temp.path().join("custom");
        fs::create_dir_all(selected_directory.join("nested/WorkBuddy.app/Contents"))
            .expect("nested app bundle");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("application-data"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: Some(selected_directory),
        };

        assert!(locate_desktop_app(
            &context,
            &["WorkBuddy.app"],
            &["com.workbuddy.workbuddy"],
            &[],
        )
        .is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn desktop_lookup_uses_the_injected_windows_local_app_data_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let local_app_data = temp.path().join("LocalAppData");
        let executable = local_app_data.join("Programs/WorkBuddy/WorkBuddy.exe");
        fs::create_dir_all(executable.parent().expect("parent")).expect("app directory");
        fs::write(&executable, b"test executable").expect("executable");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: Some(local_app_data),
            program_files: Vec::new(),
        };

        let installation =
            locate_desktop_app(&context, &[], &[], &["Programs/WorkBuddy/WorkBuddy.exe"])
                .expect("installed");

        assert_eq!(installation.path, executable);
        assert_eq!(installation.kind, InstallationKind::DesktopApp);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn desktop_lookup_supports_a_versioned_installation_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let program_files = temp.path().join("ProgramFiles");
        let executable = program_files.join("QClaw/v0.2.35.624/QClaw.exe");
        let older_executable = program_files.join("QClaw/v0.2.9.100/QClaw.exe");
        fs::create_dir_all(executable.parent().expect("parent")).expect("app directory");
        fs::write(&executable, b"test executable").expect("executable");
        fs::create_dir_all(older_executable.parent().expect("parent"))
            .expect("older app directory");
        fs::write(older_executable, b"older test executable").expect("older executable");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: None,
            local_app_data: None,
            program_files: vec![program_files],
        };

        let installation =
            locate_desktop_app(&context, &[], &[], &["QClaw/QClaw.exe"]).expect("installed");

        assert_eq!(installation.path, executable);
        assert_eq!(installation.kind, InstallationKind::DesktopApp);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn desktop_lookup_prefers_a_user_selected_installation_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let custom_directory = temp.path().join("custom/agent-install");
        let custom_executable = custom_directory.join("WorkBuddy.exe");
        let standard_root = temp.path().join("LocalAppData");
        let standard_executable = standard_root.join("Programs/WorkBuddy/WorkBuddy.exe");
        fs::create_dir_all(&custom_directory).expect("custom directory");
        fs::write(&custom_executable, b"custom executable").expect("custom executable");
        fs::create_dir_all(standard_executable.parent().expect("parent"))
            .expect("standard directory");
        fs::write(standard_executable, b"standard executable").expect("standard executable");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: Some(custom_directory),
            local_app_data: Some(standard_root),
            program_files: Vec::new(),
        };

        let installation =
            locate_desktop_app(&context, &[], &[], &["Programs/WorkBuddy/WorkBuddy.exe"])
                .expect("installed");

        assert_eq!(installation.path, custom_executable);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn custom_installation_scan_is_bounded_to_one_directory_level() {
        let temp = tempfile::tempdir().expect("temp");
        let custom_directory = temp.path().join("custom");
        let too_deep = custom_directory.join("nested/version/WorkBuddy.exe");
        fs::create_dir_all(too_deep.parent().expect("parent")).expect("nested directory");
        fs::write(too_deep, b"executable").expect("executable");
        let context = DiscoveryContext {
            home: temp.path().join("home"),
            application_data_dir: temp.path().join("Roaming"),
            application_dirs: Vec::new(),
            path_entries: Vec::new(),
            system_application_search: false,
            custom_installation_path: Some(custom_directory),
            local_app_data: None,
            program_files: Vec::new(),
        };

        assert!(
            locate_desktop_app(&context, &[], &[], &["Programs/WorkBuddy/WorkBuddy.exe"]).is_none()
        );
    }

    #[test]
    fn nvm_versions_sort_numerically() {
        let temp = tempfile::tempdir().expect("temp");
        for version in ["v9.9.0", "v20.1.0", "v18.20.4"] {
            let path = temp.path().join(".nvm/versions/node").join(version);
            fs::create_dir_all(&path).expect("version");
        }
        let candidates = nvm_codex_candidates(temp.path());
        assert!(candidates[0].to_string_lossy().contains("v20.1.0"));
        assert!(candidates[1].to_string_lossy().contains("v18.20.4"));
        assert!(candidates[2].to_string_lossy().contains("v9.9.0"));
    }
}
