use std::fmt;

#[cfg(target_os = "macos")]
use std::{collections::HashMap, sync::Mutex};

#[cfg(test)]
use std::sync::Arc;
#[cfg(all(test, not(target_os = "macos")))]
use std::{collections::HashMap, sync::Mutex};

use keyring::Entry;
use zeroize::Zeroize;

use crate::domain::{AppResult, CommandError};

const SERVICE_NAME: &str = "com.atswitch.desktop";
#[cfg(target_os = "macos")]
const VAULT_ACCOUNT: &str = "at-switch-vault-v1";

/// A secret that redacts itself in diagnostics and clears its owned buffer on
/// drop. Callers should keep it in the narrowest possible scope.
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub trait SecretStore: Send + Sync {
    fn put(&self, reference: &str, secret: &SecretValue) -> AppResult<()>;
    fn get(&self, reference: &str) -> AppResult<SecretValue>;
    fn delete(&self, reference: &str) -> AppResult<()>;
    #[allow(dead_code)]
    fn exists(&self, reference: &str) -> bool;
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct SecretCache(HashMap<String, String>);

#[cfg(target_os = "macos")]
impl Drop for SecretCache {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct NativeSecretStore {
    cache: Mutex<Option<SecretCache>>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct NativeSecretStore;

impl NativeSecretStore {
    fn entry(reference: &str) -> AppResult<Entry> {
        Entry::new(SERVICE_NAME, reference).map_err(|error| {
            log::error!("credential entry initialization failed: {error}");
            CommandError::new("credential_store_unavailable", "系统凭据库不可用")
                .with_recovery("请解锁系统凭据库后重试。")
        })
    }

    #[cfg(target_os = "macos")]
    fn vault_entry() -> AppResult<Entry> {
        Self::entry(VAULT_ACCOUNT)
    }

    #[cfg(target_os = "macos")]
    fn load_vault() -> AppResult<SecretCache> {
        match Self::vault_entry()?.get_password() {
            Ok(mut payload) => {
                let values = serde_json::from_str(&payload).map_err(|_| {
                    CommandError::new(
                        "credential_vault_invalid",
                        "钥匙串中的 AT-Switch 密钥保险库格式无效",
                    )
                    .with_recovery("请重新填写受影响 Provider 的 API Key。")
                });
                payload.zeroize();
                values.map(SecretCache)
            }
            Err(keyring::Error::NoEntry) => Ok(SecretCache::default()),
            Err(error) => Err(map_credential_read_error(error)),
        }
    }

    #[cfg(target_os = "macos")]
    fn persist_vault(cache: &SecretCache) -> AppResult<()> {
        let mut payload = serde_json::to_string(&cache.0)
            .map_err(|_| CommandError::internal("无法序列化本地密钥保险库"))?;
        let result = Self::vault_entry()?
            .set_password(&payload)
            .map_err(|error| {
                log::error!("credential vault write failed: {error}");
                CommandError::new("credential_write_failed", "无法把密钥写入系统钥匙串")
                    .with_recovery("请解锁登录钥匙串，并为 AT-Switch 选择“始终允许”。")
            });
        payload.zeroize();
        result
    }

    #[cfg(target_os = "macos")]
    fn cache(&self) -> AppResult<std::sync::MutexGuard<'_, Option<SecretCache>>> {
        self.cache
            .lock()
            .map_err(|_| CommandError::internal("本地密钥缓存锁已损坏"))
    }
}

impl SecretStore for NativeSecretStore {
    fn put(&self, reference: &str, secret: &SecretValue) -> AppResult<()> {
        #[cfg(target_os = "macos")]
        {
            let mut cache = self.cache()?;
            let values = match cache.as_mut() {
                Some(values) => values,
                None => cache.insert(Self::load_vault()?),
            };
            if let Some(mut previous) = values
                .0
                .insert(reference.to_owned(), secret.expose().to_owned())
            {
                previous.zeroize();
            }
            Self::persist_vault(values)
        }

        #[cfg(not(target_os = "macos"))]
        Self::entry(reference)?
            .set_password(secret.expose())
            .map_err(|error| {
                log::error!("credential write failed: {error}");
                CommandError::new("credential_write_failed", "无法把密钥写入系统凭据库")
                    .with_recovery("请检查系统凭据库权限或解锁状态。")
            })
    }

    fn get(&self, reference: &str) -> AppResult<SecretValue> {
        #[cfg(target_os = "macos")]
        {
            let mut cache = self.cache()?;
            let values = match cache.as_mut() {
                Some(values) => values,
                None => cache.insert(Self::load_vault()?),
            };
            if let Some(secret) = values.0.get(reference) {
                return Ok(SecretValue::new(secret.clone()));
            }

            // Migrate old per-reference Keychain items lazily. Each legacy
            // item can prompt once; all subsequent reads use the single vault.
            let mut legacy = Self::entry(reference)?
                .get_password()
                .map_err(map_credential_read_error)?;
            values.0.insert(reference.to_owned(), legacy.clone());
            if let Err(error) = Self::persist_vault(values) {
                if let Some(mut cached) = values.0.remove(reference) {
                    cached.zeroize();
                }
                legacy.zeroize();
                return Err(error);
            }
            let result = SecretValue::new(legacy.clone());
            legacy.zeroize();
            Ok(result)
        }

        #[cfg(not(target_os = "macos"))]
        Self::entry(reference)?
            .get_password()
            .map(SecretValue::new)
            .map_err(map_credential_read_error)
    }

    fn delete(&self, reference: &str) -> AppResult<()> {
        #[cfg(target_os = "macos")]
        {
            let mut cache = self.cache()?;
            let values = match cache.as_mut() {
                Some(values) => values,
                None => cache.insert(Self::load_vault()?),
            };
            if let Some(mut removed) = values.0.remove(reference) {
                removed.zeroize();
                Self::persist_vault(values)?;
            }
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            let entry = Self::entry(reference)?;
            match entry.delete_credential() {
                Ok(()) => Ok(()),
                Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => {
                    log::error!("credential deletion failed: {error}");
                    Err(CommandError::new(
                        "credential_delete_failed",
                        "无法删除系统凭据",
                    ))
                }
            }
        }
    }

    fn exists(&self, reference: &str) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.get(reference).is_ok()
        }

        #[cfg(not(target_os = "macos"))]
        Self::entry(reference)
            .and_then(|entry| {
                entry.get_password().map(SecretValue::new).map_err(|_| {
                    CommandError::new("credential_missing", "系统凭据库中找不到对应密钥")
                })
            })
            .is_ok()
    }
}

fn map_credential_read_error(error: keyring::Error) -> CommandError {
    match error {
        keyring::Error::NoEntry => {
            CommandError::new("credential_missing", "系统凭据库中找不到对应密钥")
                .with_recovery("请编辑 Provider 并重新填写 API Key。")
        }
        keyring::Error::NoStorageAccess(error) => {
            log::warn!("credential store access was denied: {error}");
            CommandError::new("credential_store_locked", "无法访问系统凭据库")
                .with_recovery("请解锁系统凭据库并允许 AT-Switch 访问后重试。")
        }
        error => {
            log::warn!("credential read failed: {error}");
            CommandError::new("credential_read_failed", "读取系统凭据失败")
                .with_recovery("请检查系统凭据库权限；仍失败时可重新填写 API Key。")
        }
    }
}

/// Deterministic secret store for tests. It deliberately lives in memory so
/// automated tests never touch the developer's Keychain or Credential Manager.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct MemorySecretStore {
    values: Arc<Mutex<HashMap<String, String>>>,
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn put(&self, reference: &str, secret: &SecretValue) -> AppResult<()> {
        self.values
            .lock()
            .map_err(|_| CommandError::internal("测试密钥存储锁已损坏"))?
            .insert(reference.to_owned(), secret.expose().to_owned());
        Ok(())
    }

    fn get(&self, reference: &str) -> AppResult<SecretValue> {
        self.values
            .lock()
            .map_err(|_| CommandError::internal("测试密钥存储锁已损坏"))?
            .get(reference)
            .cloned()
            .map(SecretValue::new)
            .ok_or_else(|| CommandError::new("credential_missing", "测试密钥不存在"))
    }

    fn delete(&self, reference: &str) -> AppResult<()> {
        self.values
            .lock()
            .map_err(|_| CommandError::internal("测试密钥存储锁已损坏"))?
            .remove(reference);
        Ok(())
    }

    fn exists(&self, reference: &str) -> bool {
        self.values
            .lock()
            .map(|values| values.contains_key(reference))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_native_credential_has_a_recoverable_error() {
        let error = map_credential_read_error(keyring::Error::NoEntry);
        assert_eq!(error.code, "credential_missing");
        assert!(error.recovery.is_some());
    }

    #[test]
    fn memory_store_persists_across_separate_operations() {
        let store = MemorySecretStore::default();
        store
            .put(
                "provider/test/api-key/v1",
                &SecretValue::new("secret".into()),
            )
            .expect("put");
        assert_eq!(
            store.get("provider/test/api-key/v1").expect("get").expose(),
            "secret"
        );
    }
}
