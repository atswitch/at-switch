use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[cfg(target_os = "macos")]
use std::{
    io::Read,
    process::{Command, Stdio},
};

use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::{
    domain::{AppResult, CommandError},
    infrastructure::SecretValue,
};

use super::ImaSecret;

const MAX_PREFERENCES_BYTES: u64 = 8 * 1024 * 1024;
const CREDENTIAL_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Deserialize)]
struct Preferences {
    tencent: TencentPreferences,
}

#[derive(Deserialize)]
struct TencentPreferences {
    wxlogin: LoginPreferences,
}

#[derive(Deserialize)]
struct LoginPreferences {
    account_meta: String,
}

#[derive(Deserialize)]
struct AccountMetadata {
    #[serde(default)]
    is_login: bool,
    credential_id: String,
    #[serde(default, deserialize_with = "optional_scalar")]
    user_id: Option<String>,
    #[serde(default, deserialize_with = "optional_scalar")]
    id_type: Option<String>,
    #[serde(default, deserialize_with = "optional_scalar")]
    token_type: Option<String>,
}

#[derive(Deserialize)]
struct CredentialPayload {
    token: ImaSecret,
    #[serde(alias = "refreshToken")]
    refresh_token: ImaSecret,
    #[serde(default, deserialize_with = "optional_scalar")]
    user_id: Option<String>,
    #[serde(default, deserialize_with = "optional_scalar")]
    id_type: Option<String>,
    #[serde(default, deserialize_with = "optional_scalar")]
    token_type: Option<String>,
}

fn optional_scalar<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Scalar {
        Text(String),
        Number(serde_json::Number),
    }
    Option::<Scalar>::deserialize(deserializer).map(|value| {
        value.map(|value| match value {
            Scalar::Text(value) => value,
            Scalar::Number(value) => value.to_string(),
        })
    })
}

fn read_metadata(path: &Path) -> AppResult<AccountMetadata> {
    let size = std::fs::metadata(path).map_err(|_| login_required())?.len();
    if size > MAX_PREFERENCES_BYTES {
        return Err(metadata_invalid());
    }
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| login_required())?);
    let prefs: Preferences = serde_json::from_slice(&bytes).map_err(|_| metadata_invalid())?;
    let raw_meta = Zeroizing::new(prefs.tencent.wxlogin.account_meta);
    let account: AccountMetadata =
        serde_json::from_str(&raw_meta).map_err(|_| metadata_invalid())?;
    if !account.is_login || account.credential_id.is_empty() {
        return Err(login_required());
    }
    if account.user_id.as_deref().is_none_or(str::is_empty)
        || account.id_type.as_deref().is_none_or(str::is_empty)
    {
        return Err(metadata_invalid());
    }
    Ok(account)
}

fn account_key(account: &AccountMetadata) -> String {
    let mut hasher = Sha256::new();
    // Length prefixes distinguish identities containing separator characters.
    for part in [
        account.user_id.as_deref().unwrap_or_default(),
        account.id_type.as_deref().unwrap_or_default(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Safe for installation scans: reads only ima's login metadata, never the
/// Keychain, and returns an opaque account fingerprint rather than an account ID.
pub fn active_account(preferences_path: &Path) -> AppResult<String> {
    read_metadata(preferences_path).map(|account| account_key(&account))
}

pub trait ImaCredentialSource: Send + Sync {
    /// Called only by an explicitly requested switch/restore operation.
    fn read(&self, preferences_path: &Path, credential_id: &str) -> AppResult<SecretValue>;
}

#[derive(Default)]
pub struct NativeImaCredentialSource;

impl ImaCredentialSource for NativeImaCredentialSource {
    fn read(&self, preferences_path: &Path, credential_id: &str) -> AppResult<SecretValue> {
        #[cfg(target_os = "macos")]
        {
            let _ = preferences_path;
            read_macos_credential(credential_id)
        }
        #[cfg(target_os = "windows")]
        {
            let _ = credential_id;
            super::windows_auth::read_windows_credential(preferences_path)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (preferences_path, credential_id);
            Err(CommandError::new(
                "ima_platform_unsupported",
                "当前系统尚未支持 ima 登录态连接",
            )
            .with_recovery("此版本支持 macOS；Windows 登录态接入仍待验证。"))
        }
    }
}

#[cfg(target_os = "macos")]
fn read_macos_credential(credential_id: &str) -> AppResult<SecretValue> {
    let mut child = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "com.tencent.ima.account",
            "-a",
            credential_id,
            "-w",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| credential_unavailable())?;
    let deadline = std::time::Instant::now() + CREDENTIAL_READ_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(credential_unavailable());
                }
                let mut output = Zeroizing::new(Vec::new());
                child
                    .stdout
                    .take()
                    .ok_or_else(credential_unavailable)?
                    .read_to_end(&mut output)
                    .map_err(|_| credential_unavailable())?;
                let bytes = std::mem::take(&mut *output);
                let mut value = match String::from_utf8(bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        use zeroize::Zeroize;
                        let mut bytes = error.into_bytes();
                        bytes.zeroize();
                        return Err(credential_invalid());
                    }
                };
                while value.ends_with(['\n', '\r']) {
                    value.pop();
                }
                if value.is_empty() {
                    return Err(credential_unavailable());
                }
                return Ok(SecretValue::new(value));
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(credential_authorization_timeout());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return Err(credential_unavailable()),
        }
    }
}

pub struct ImaSession {
    account_key: String,
    credential_id: String,
    preferences_path: PathBuf,
    token: ImaSecret,
    refresh_token: ImaSecret,
    user_id: ImaSecret,
    id_type: String,
    token_type: String,
    web_version: String,
    invalidated: AtomicBool,
}

impl fmt::Debug for ImaSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ImaSession([REDACTED])")
    }
}

impl ImaSession {
    pub fn account_key(&self) -> &str {
        &self.account_key
    }

    pub fn invalidate(&self) {
        self.invalidated.store(true, Ordering::Release);
    }

    pub fn is_valid(&self) -> bool {
        !self.invalidated.load(Ordering::Acquire)
    }

    pub(super) fn ensure_current_account(&self) -> AppResult<()> {
        if !self.is_valid() {
            return Err(
                CommandError::new("ima_session_expired", "ima 登录连接已失效")
                    .with_recovery("请确认 ima 已登录，再次切换以重新连接。"),
            );
        }
        match read_metadata(&self.preferences_path) {
            Ok(metadata)
                if account_key(&metadata) == self.account_key
                    && metadata.credential_id == self.credential_id =>
            {
                Ok(())
            }
            _ => {
                self.invalidate();
                Err(
                    CommandError::new("ima_account_changed", "ima 当前账号已变化")
                        .with_recovery("请保持 ima 登录在需要切换模型的账号，然后重试。"),
                )
            }
        }
    }

    pub(super) fn headers(&self) -> AppResult<HeaderMap> {
        let client_type = if cfg!(target_os = "windows") {
            "256021"
        } else {
            "256020"
        };
        let cookie = Zeroizing::new(format!(
            "IMA-TOKEN={}; IMA-REFRESH-TOKEN={}; IMA-UID={}; UID-TYPE={}; TOKEN-TYPE={}; PLATFORM=H5; CLIENT-TYPE={}; WEB-VERSION={}",
            self.token.expose(), self.refresh_token.expose(), self.user_id.expose(),
            self.id_type, self.token_type, client_type, self.web_version,
        ));
        let mut value = HeaderValue::from_str(&cookie).map_err(|_| credential_invalid())?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-ima-cookie", value);
        headers.insert(
            "x-ima-bkn",
            HeaderValue::from_str(&token_bkn(self.token.expose()).to_string())
                .map_err(|_| credential_invalid())?,
        );
        headers.insert("from_browser_ima", HeaderValue::from_static("1"));
        headers.insert(
            "extension_version",
            HeaderValue::from_str(&self.web_version).map_err(|_| metadata_invalid())?,
        );
        Ok(headers)
    }
}

/// Memory-only session cache. Account changes and failed authentication evict
/// the cached identity before another operation can reuse it.
pub struct ImaSessionProvider {
    source: Arc<dyn ImaCredentialSource>,
    cached: Mutex<Option<Arc<ImaSession>>>,
}

impl Default for ImaSessionProvider {
    fn default() -> Self {
        Self::new(Arc::new(NativeImaCredentialSource))
    }
}

impl ImaSessionProvider {
    pub fn new(source: Arc<dyn ImaCredentialSource>) -> Self {
        Self {
            source,
            cached: Mutex::new(None),
        }
    }

    pub async fn session(
        &self,
        preferences_path: &Path,
        web_version: &str,
    ) -> AppResult<Arc<ImaSession>> {
        let mut cached = self.cached.lock().await;
        let metadata = match read_metadata(preferences_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if let Some(previous) = cached.take() {
                    previous.invalidate();
                }
                return Err(error);
            }
        };
        let key = account_key(&metadata);
        if let Some(session) = cached.as_ref() {
            if session.account_key == key
                && session.credential_id == metadata.credential_id
                && session.web_version == web_version
                && session.is_valid()
            {
                return Ok(Arc::clone(session));
            }
        }
        if let Some(previous) = cached.take() {
            previous.invalidate();
        }
        if web_version.is_empty()
            || !web_version
                .chars()
                .all(|value| value.is_ascii_digit() || value == '.')
        {
            return Err(metadata_invalid());
        }
        let source = Arc::clone(&self.source);
        let credential_id = metadata.credential_id.clone();
        let source_path = preferences_path.to_path_buf();
        let secret = tokio::time::timeout(
            CREDENTIAL_READ_TIMEOUT,
            tokio::task::spawn_blocking(move || source.read(&source_path, &credential_id)),
        )
        .await
        .map_err(|_| credential_authorization_timeout())?
        .map_err(|_| credential_unavailable())??;
        let payload: CredentialPayload =
            serde_json::from_str(secret.expose()).map_err(|_| credential_invalid())?;
        if payload
            .user_id
            .as_ref()
            .is_some_and(|id| Some(id) != metadata.user_id.as_ref())
            || payload
                .id_type
                .as_ref()
                .is_some_and(|kind| Some(kind) != metadata.id_type.as_ref())
        {
            return Err(credential_invalid());
        }
        let session = Arc::new(ImaSession {
            account_key: key,
            credential_id: metadata.credential_id,
            preferences_path: preferences_path.to_path_buf(),
            token: payload.token,
            refresh_token: payload.refresh_token,
            user_id: ImaSecret::new(
                payload
                    .user_id
                    .or(metadata.user_id)
                    .ok_or_else(credential_invalid)?,
            ),
            id_type: payload
                .id_type
                .or(metadata.id_type)
                .ok_or_else(credential_invalid)?,
            token_type: payload
                .token_type
                .or(metadata.token_type)
                .ok_or_else(credential_invalid)?,
            web_version: web_version.to_owned(),
            invalidated: AtomicBool::new(false),
        });
        // Reject cookie-delimiter injection before constructing authenticated HTTP.
        for value in [
            session.token.expose(),
            session.refresh_token.expose(),
            session.user_id.expose(),
            &session.id_type,
            &session.token_type,
        ] {
            if value.is_empty()
                || value
                    .chars()
                    .any(|value| value.is_control() || value == ';')
            {
                return Err(credential_invalid());
            }
        }
        // A user can switch accounts while the native permission dialog is open.
        if active_account(preferences_path)? != session.account_key {
            return Err(
                CommandError::new("ima_account_changed", "ima 当前账号已变化")
                    .with_recovery("请保持 ima 登录在需要切换模型的账号，然后重试。"),
            );
        }
        *cached = Some(Arc::clone(&session));
        Ok(session)
    }
}

fn token_bkn(token: &str) -> u32 {
    token.encode_utf16().fold(5381_u32, |hash, unit| {
        hash.wrapping_mul(33).wrapping_add(u32::from(unit))
    }) & 0x7fff_ffff
}

fn login_required() -> CommandError {
    CommandError::new("ima_login_required", "请先在 ima 中登录账号")
        .with_recovery("打开 ima 完成登录，再返回 AT-Switch 切换模型。")
}

fn metadata_invalid() -> CommandError {
    CommandError::new("ima_login_format_unsupported", "无法识别当前 ima 登录配置")
        .with_recovery("请打开 ima 确认已正常登录；若仍失败，请更新 AT-Switch 后重试。")
}

fn credential_invalid() -> CommandError {
    CommandError::new(
        "ima_credential_format_unsupported",
        "无法识别当前 ima 登录凭据",
    )
    .with_recovery("请在 ima 中重新登录后重试；若仍失败，请更新 AT-Switch。")
}

fn credential_unavailable() -> CommandError {
    CommandError::new("ima_credential_unavailable", "无法读取 ima 登录凭据")
        .with_recovery("请解锁系统凭据库，确认 ima 已登录，然后重试。")
}

fn credential_authorization_timeout() -> CommandError {
    CommandError::new("ima_authorization_timeout", "等待 ima 登录凭据授权超时")
        .with_recovery("请解锁系统钥匙串并允许访问 ima 凭据，然后重试。")
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[derive(Default)]
    struct MemoryCredentialSource {
        reads: AtomicUsize,
    }

    impl ImaCredentialSource for MemoryCredentialSource {
        fn read(&self, _preferences_path: &Path, _credential_id: &str) -> AppResult<SecretValue> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(SecretValue::new(
                serde_json::json!({
                    "token": "fictional-ima-token", "refresh_token": "fictional-refresh-token",
                })
                .to_string(),
            ))
        }
    }

    fn write_preferences(path: &Path, credential_id: &str, logged_in: bool) {
        let meta = serde_json::json!({
            "is_login": logged_in, "credential_id": credential_id,
            "user_id": format!("fictional-user-{credential_id}"), "id_type": 3, "token_type": 1,
        });
        std::fs::write(
            path,
            serde_json::json!({
                "tencent": { "wxlogin": {"account_meta": meta.to_string()} },
            })
            .to_string(),
        )
        .unwrap();
    }

    pub async fn test_session() -> (tempfile::TempDir, Arc<ImaSession>) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        write_preferences(&path, "fictional-account", true);
        let provider = ImaSessionProvider::new(Arc::new(MemoryCredentialSource::default()));
        let session = provider.session(&path, "5.6.3").await.unwrap();
        (directory, session)
    }

    #[tokio::test]
    async fn scans_do_not_read_credentials_and_switches_reuse_the_memory_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        write_preferences(&path, "fictional-account", true);
        let source = Arc::new(MemoryCredentialSource::default());
        let provider = ImaSessionProvider::new(source.clone());
        assert_eq!(active_account(&path).unwrap().len(), 64);
        assert_eq!(source.reads.load(Ordering::SeqCst), 0);
        let first = provider.session(&path, "5.6.3").await.unwrap();
        let second = provider.session(&path, "5.6.3").await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(source.reads.load(Ordering::SeqCst), 1);
        first.invalidate();
        let third = provider.session(&path, "5.6.3").await.unwrap();
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn account_change_and_logout_prevent_stale_session_reuse() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        write_preferences(&path, "fictional-one", true);
        let source = Arc::new(MemoryCredentialSource::default());
        let provider = ImaSessionProvider::new(source.clone());
        let first = provider.session(&path, "5.6.3").await.unwrap();
        write_preferences(&path, "fictional-two", true);
        assert_eq!(
            first.ensure_current_account().unwrap_err().code,
            "ima_account_changed"
        );
        let second = provider.session(&path, "5.6.3").await.unwrap();
        assert_ne!(first.account_key(), second.account_key());
        assert!(!first.is_valid());
        write_preferences(&path, "fictional-two", false);
        assert_eq!(
            provider.session(&path, "5.6.3").await.unwrap_err().code,
            "ima_login_required"
        );
        assert!(!second.is_valid());
    }

    #[tokio::test]
    async fn credential_rotation_keeps_the_account_baseline_scope_but_reloads_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        write_preferences(&path, "fictional-original", true);
        let source = Arc::new(MemoryCredentialSource::default());
        let provider = ImaSessionProvider::new(source.clone());
        let first = provider.session(&path, "5.6.3").await.unwrap();
        let mut root: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut meta: serde_json::Value =
            serde_json::from_str(root["tencent"]["wxlogin"]["account_meta"].as_str().unwrap())
                .unwrap();
        meta["credential_id"] = serde_json::json!("fictional-rotated");
        root["tencent"]["wxlogin"]["account_meta"] = serde_json::json!(meta.to_string());
        std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
        assert_eq!(active_account(&path).unwrap(), first.account_key());
        assert!(first.ensure_current_account().is_err());
        let second = provider.session(&path, "5.6.3").await.unwrap();
        assert_eq!(first.account_key(), second.account_key());
        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn credential_diagnostics_and_sensitive_headers_are_redacted() {
        let (_directory, session) = test_session().await;
        let debug = format!("{session:?}");
        assert!(!debug.contains("fictional"));
        let headers = session.headers().unwrap();
        assert!(headers["x-ima-cookie"].is_sensitive());
        assert!(!format!("{headers:?}").contains("fictional-ima-token"));
        assert_eq!(token_bkn("abc"), 193485963);
        assert_eq!(
            format!("{:?}", ImaSecret::new("fictional-api-key")),
            "ImaSecret([REDACTED])"
        );
    }

    #[test]
    fn malformed_metadata_has_an_actionable_non_sensitive_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preferences");
        std::fs::write(&path, "invalid fictional secret input").unwrap();
        let error = active_account(&path).unwrap_err();
        assert_eq!(error.code, "ima_login_format_unsupported");
        assert!(error.recovery.is_some());
        assert!(!format!("{error:?}").contains("fictional secret"));
    }
}
