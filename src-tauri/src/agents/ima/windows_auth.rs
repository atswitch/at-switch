#[cfg(target_os = "windows")]
use std::{fs::File, io::Read, path::Path};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::{
    domain::{AppResult, CommandError},
    infrastructure::SecretValue,
};

use super::ImaSecret;

const MAX_PREFERENCES_BYTES: usize = 8 * 1024 * 1024;
const MAX_CREDENTIAL_BYTES: usize = 64 * 1024;
const MAX_BASE64_BYTES: usize = MAX_CREDENTIAL_BYTES.div_ceil(3) * 4;

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
    account_secret_encrypted: ImaSecret,
}

#[derive(Deserialize)]
struct CredentialShape {
    token: ImaSecret,
    #[serde(alias = "refreshToken")]
    refresh_token: ImaSecret,
}

#[cfg(target_os = "windows")]
pub(super) fn read_windows_credential(preferences_path: &Path) -> AppResult<SecretValue> {
    let file = File::open(preferences_path).map_err(|_| credential_unavailable())?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take((MAX_PREFERENCES_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| credential_unavailable())?;
    let encrypted = parse_encrypted_credential(&bytes)?;
    let decrypted = decrypt_dpapi(&encrypted)?;
    parse_plaintext_credential(&decrypted)
}

fn parse_encrypted_credential(preferences: &[u8]) -> AppResult<Zeroizing<Vec<u8>>> {
    if preferences.is_empty() || preferences.len() > MAX_PREFERENCES_BYTES {
        return Err(credential_invalid());
    }
    let parsed: Preferences =
        serde_json::from_slice(preferences).map_err(|_| credential_invalid())?;
    let encoded = parsed.tencent.wxlogin.account_secret_encrypted.expose();
    if encoded.is_empty() {
        return Err(credential_unavailable());
    }
    if encoded.len() > MAX_BASE64_BYTES {
        return Err(credential_invalid());
    }
    let encrypted = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| credential_invalid())?);
    if encrypted.is_empty() || encrypted.len() > MAX_CREDENTIAL_BYTES {
        return Err(credential_invalid());
    }
    if encrypted.starts_with(b"v10") {
        return Err(CommandError::new(
            "ima_windows_credential_migration_required",
            "ima 登录凭据仍使用旧版存储格式",
        )
        .with_recovery("请更新并打开 ima，完成登录凭据的自动迁移后再次切换。"));
    }
    Ok(encrypted)
}

fn parse_plaintext_credential(bytes: &[u8]) -> AppResult<SecretValue> {
    if bytes.is_empty() || bytes.len() > MAX_CREDENTIAL_BYTES {
        return Err(credential_invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| credential_invalid())?;
    let parsed: CredentialShape = serde_json::from_str(text).map_err(|_| credential_invalid())?;
    if parsed.token.expose().trim().is_empty() || parsed.refresh_token.expose().trim().is_empty() {
        return Err(credential_invalid());
    }
    Ok(SecretValue::new(text.to_owned()))
}

#[cfg(target_os = "windows")]
fn decrypt_dpapi(bytes: &[u8]) -> AppResult<Zeroizing<Vec<u8>>> {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB},
    };
    use zeroize::Zeroize;

    struct DpapiOutput(CRYPT_INTEGER_BLOB);

    impl Drop for DpapiOutput {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                // CryptUnprotectData owns the allocation contract: cbData is
                // the initialized byte length, not a length from the JSON input.
                // Clear even rejected plaintext before releasing its OS buffer.
                unsafe {
                    let plaintext =
                        std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize);
                    plaintext.zeroize();
                    LocalFree(self.0.pbData.cast());
                }
            }
        }
    }

    if bytes.is_empty() || bytes.len() > MAX_CREDENTIAL_BYTES {
        return Err(credential_invalid());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| credential_invalid())?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = DpapiOutput(CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    });
    // ima's current Windows AccountSecretStore calls DPAPI without entropy,
    // prompt or flags. The input remains alive and unchanged through this call.
    let success = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &mut output.0,
        )
    };
    if success == 0 {
        return Err(credential_unavailable());
    }
    let length = output.0.cbData as usize;
    if output.0.pbData.is_null() || length == 0 || length > MAX_CREDENTIAL_BYTES {
        return Err(credential_invalid());
    }
    // The successful OS call supplies an initialized allocation of cbData bytes;
    // the guard keeps it alive until this separately zeroized copy is complete.
    let plaintext = unsafe { std::slice::from_raw_parts(output.0.pbData, length) };
    Ok(Zeroizing::new(plaintext.to_vec()))
}

fn credential_invalid() -> CommandError {
    CommandError::new("ima_credential_invalid", "ima 登录凭据格式暂不支持")
        .with_recovery("请更新并打开 ima，确认已登录后重试。")
}

fn credential_unavailable() -> CommandError {
    CommandError::new("ima_credential_unavailable", "无法读取 ima 当前登录凭据")
        .with_recovery("请在当前 Windows 用户下打开 ima 并确认登录，然后重试。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preferences(encoded: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "tencent": {"wxlogin": {
                "account_secret_encrypted": encoded,
                "account_meta": "example-private-metadata"
            }},
            "unknown": {"keep": true}
        }))
        .unwrap()
    }

    #[test]
    fn extracts_only_base64_ciphertext_from_typed_preferences() {
        let original = b"example-dpapi-ciphertext";
        let actual = parse_encrypted_credential(&preferences(&STANDARD.encode(original))).unwrap();
        assert_eq!(actual.as_slice(), original);
    }

    #[test]
    fn legacy_oscrypt_requires_ima_native_migration() {
        let error =
            parse_encrypted_credential(&preferences(&STANDARD.encode(b"v10example-legacy")))
                .unwrap_err();
        assert_eq!(error.code, "ima_windows_credential_migration_required");
        assert!(!error.message.contains("example-legacy"));
    }

    #[test]
    fn malformed_empty_and_wrong_type_inputs_fail_without_secret_text() {
        for source in [
            preferences("example-private-invalid-base64!"),
            preferences(""),
            br#"{"tencent":{"wxlogin":{"account_secret_encrypted":123}}}"#.to_vec(),
            br#"{"tencent":{"wxlogin":{}}}"#.to_vec(),
            b"example-private-invalid-json".to_vec(),
            Vec::new(),
        ] {
            let error = parse_encrypted_credential(&source).unwrap_err();
            assert!(!error.message.contains("example-private"));
            assert!(!error
                .recovery
                .unwrap_or_default()
                .contains("example-private"));
        }
    }

    #[test]
    fn bounds_preferences_ciphertext_and_plaintext_before_use() {
        let oversized_file = vec![b' '; MAX_PREFERENCES_BYTES + 1];
        assert_eq!(
            parse_encrypted_credential(&oversized_file)
                .unwrap_err()
                .code,
            "ima_credential_invalid"
        );
        let oversized_ciphertext = vec![0u8; MAX_CREDENTIAL_BYTES + 1];
        assert_eq!(
            parse_encrypted_credential(&preferences(&STANDARD.encode(oversized_ciphertext)))
                .unwrap_err()
                .code,
            "ima_credential_invalid"
        );
        let oversized_plaintext = vec![b' '; MAX_CREDENTIAL_BYTES + 1];
        assert_eq!(
            parse_plaintext_credential(&oversized_plaintext)
                .unwrap_err()
                .code,
            "ima_credential_invalid"
        );
    }

    #[test]
    fn decrypted_payload_requires_nonempty_tokens_and_preserves_optional_metadata() {
        let bytes = br#"{"token":"example-token","refreshToken":"example-refresh","user_id":"example-user","id_type":2}"#;
        let secret = parse_plaintext_credential(bytes).unwrap();
        assert_eq!(secret.expose().as_bytes(), bytes);
        assert!(!format!("{secret:?}").contains("example-token"));
        for invalid in [
            &b""[..],
            &b"\xff"[..],
            &br#"{"token":"example-token"}"#[..],
            &br#"{"token":" ","refresh_token":"example-refresh"}"#[..],
            &br#"{"token":123,"refresh_token":"example-refresh"}"#[..],
            &br#"{"token":"example-token","refresh_token":""}"#[..],
        ] {
            assert_eq!(
                parse_plaintext_credential(invalid).unwrap_err().code,
                "ima_credential_invalid"
            );
        }
    }
}
