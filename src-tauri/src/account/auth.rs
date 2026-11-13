use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::domain::CommandError;

use super::types::StoredTokens;

/// 本地回环授权回调端口（与本地代理 54187 同段；Windows 首次可能弹防火墙提示，
/// 仅监听 127.0.0.1，见设计文档 §3.2）。
pub const GOOGLE_LOOPBACK_PORT: u16 = 54190;

/// 一次性 Google 登录结果登记表：授权回调任务写入，前端轮询命令领取。
#[derive(Default)]
pub struct GoogleLoginRegistry(Mutex<HashMap<String, Result<StoredTokens, CommandError>>>);

impl GoogleLoginRegistry {
    pub fn insert(&self, state: &str, result: Result<StoredTokens, CommandError>) {
        self.0
            .lock()
            .expect("google registry poisoned")
            .insert(state.to_owned(), result);
    }

    pub fn take(&self, state: &str) -> Option<Result<StoredTokens, CommandError>> {
        self.0
            .lock()
            .expect("google registry poisoned")
            .remove(state)
    }
}

/// 生成 PKCE code_verifier（43 字符 base64url）与 S256 challenge。
pub fn pkce_pair() -> (String, String) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

/// 生产路径由服务端 `/v1/auth/google/authorize` 拼装授权页；客户端只负责
/// 生成 state/PKCE 并等待回环回调。
/// 打开系统浏览器（授权页）。macOS / Windows / Linux 各走系统默认方式。
pub fn open_browser(url: &str) -> Result<(), CommandError> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command.spawn().map(|_| ()).map_err(|error| {
        log::warn!("failed to open browser: {error}");
        CommandError::new("auth_login_failed", "无法打开浏览器完成授权")
            .with_recovery("请重试，或检查系统默认浏览器设置。")
    })
}

/// 一次性本地 HTTP 监听：等待 Google 带 code 回跳 `/callback`，返回 code。
/// 用户关闭浏览器页面即无回调；外层轮询超时后关闭监听。
pub async fn wait_for_google_code(state: &str) -> Result<String, CommandError> {
    let listener = TcpListener::bind(("127.0.0.1", GOOGLE_LOOPBACK_PORT))
        .await
        .map_err(|error| {
            log::warn!("google loopback bind failed: {error}");
            CommandError::new("auth_login_failed", "本地授权回调端口被占用")
                .with_recovery("请稍后重试；若持续失败请反馈给管理员。")
        })?;
    loop {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|_| CommandError::new("auth_login_failed", "本地授权回调中断"))?;
        let mut buffer = [0u8; 2048];
        let read = socket.read(&mut buffer).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let Some(path) = request.split_whitespace().nth(1) else {
            continue;
        };
        if let Some(code) = extract_query_param(path, "code") {
            let body = if extract_query_param(path, "state").as_deref() == Some(state) {
                "AT-Switch 授权成功，请返回应用。"
            } else {
                "AT-Switch 授权失败：state 不匹配。"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\
                 Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let matched = extract_query_param(path, "state").as_deref() == Some(state);
            return if matched {
                Ok(code)
            } else {
                Err(CommandError::new(
                    "auth_login_failed",
                    "授权回调 state 不匹配，已拒绝",
                ))
            };
        }
        // 非 /callback 请求（如 favicon）：返回空响应并继续等待。
        let _ = socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await;
    }
}

fn extract_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_length_matches_rfc() {
        let (verifier, challenge) = pkce_pair();
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn extracts_code_and_state() {
        let path = "/callback?state=abc&code=xyz";
        assert_eq!(extract_query_param(path, "code").as_deref(), Some("xyz"));
        assert_eq!(extract_query_param(path, "state").as_deref(), Some("abc"));
        assert_eq!(extract_query_param(path, "other"), None);
    }
}
