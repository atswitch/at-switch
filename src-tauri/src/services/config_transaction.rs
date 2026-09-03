use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{AppResult, CommandError},
    infrastructure::{SecretStore, SecretValue},
};

const BACKUP_MAGIC: &[u8; 5] = b"ATSB1";
const BACKUP_KEY_REF: &str = "backup/master-key/v1";

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub new_content: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupPayload {
    path: String,
    existed: bool,
    original_content: Vec<u8>,
    original_sha256: String,
}

#[derive(Debug, Clone)]
pub struct ConfigTransactionResult {
    pub operation_id: String,
    pub backup_path: PathBuf,
    pub final_sha256: String,
}

#[derive(Debug, Clone)]
pub struct BaselineSnapshot {
    pub existed: bool,
    pub content: Vec<u8>,
}

pub struct ConfigTransaction {
    secret_store: Arc<dyn SecretStore>,
    backup_root: PathBuf,
}

impl ConfigTransaction {
    pub fn new(secret_store: Arc<dyn SecretStore>, backup_root: PathBuf) -> Self {
        Self {
            secret_store,
            backup_root,
        }
    }

    /// Apply one file resource safely.
    ///
    /// The public shape is intentionally small; a verified Agent adapter builds
    /// an ordered resource plan above this layer. Each resource gets a verified
    /// encrypted backup and atomic same-directory replacement.
    pub fn apply_file(
        &self,
        agent_id: &str,
        change: FileChange,
    ) -> AppResult<ConfigTransactionResult> {
        validate_agent_id(agent_id)?;
        let operation_id = Uuid::new_v4().to_string();
        let original = if change.path.exists() {
            fs::read(&change.path)?
        } else {
            Vec::new()
        };
        let payload = BackupPayload {
            path: change.path.to_string_lossy().into_owned(),
            existed: change.path.exists(),
            original_sha256: sha256(&original),
            original_content: original,
        };

        let backup_path = self.write_encrypted_backup(agent_id, &operation_id, &payload)?;
        let verified_payload = self.read_encrypted_backup(&backup_path)?;
        if verified_payload.original_sha256 != payload.original_sha256 {
            return Err(CommandError::new(
                "backup_verification_failed",
                "配置备份校验失败，已阻止写入",
            ));
        }

        if let Err(error) = write_atomic(&change.path, &change.new_content) {
            log::error!("atomic configuration write failed: {error}");
            return Err(CommandError::new(
                "config_write_failed",
                "Agent 配置写入失败，原配置未被替换",
            ));
        }

        let final_content = fs::read(&change.path)?;
        if final_content != change.new_content {
            let restore_result = self.restore_payload(&verified_payload);
            return match restore_result {
                Ok(()) => Err(CommandError::new(
                    "config_validation_failed_rolled_back",
                    "写入后校验失败，已恢复原配置",
                )),
                Err(_) => Err(CommandError::new(
                    "config_validation_failed_manual_recovery",
                    "写入后校验失败且自动恢复失败，需要人工处理",
                )),
            };
        }

        Ok(ConfigTransactionResult {
            operation_id,
            backup_path,
            final_sha256: sha256(&final_content),
        })
    }

    pub fn restore_backup(&self, backup_path: &Path) -> AppResult<()> {
        let payload = self.read_encrypted_backup(backup_path)?;
        self.restore_payload(&payload)
    }

    /// Returns the configuration that existed before AT-Switch first managed
    /// this Agent. The baseline is encrypted with the same OS-keystore-backed
    /// key as normal rollback backups.
    ///
    /// Older AT-Switch builds did not create a named baseline. For upgrades we
    /// recover the earliest matching rollback backup instead of snapshotting an
    /// already-managed configuration as the user's native state.
    pub fn baseline(&self, agent_id: &str, path: &Path) -> AppResult<BaselineSnapshot> {
        validate_agent_id(agent_id)?;
        let legacy_baseline_path = self.backup_root.join(agent_id).join("baseline.atsb");
        let expected_path = path.to_string_lossy().into_owned();
        let mut baseline_path = legacy_baseline_path.clone();

        if legacy_baseline_path.exists() {
            let payload = self.read_encrypted_backup(&legacy_baseline_path)?;
            if payload.path == expected_path {
                return Ok(BaselineSnapshot {
                    existed: payload.existed,
                    content: payload.original_content,
                });
            }

            // An adapter may move from a generated file to the Agent's true
            // source-of-truth file during an AT-Switch upgrade. Keep the old
            // encrypted baseline intact and create a path-scoped baseline for
            // the new resource instead of blocking every future switch.
            baseline_path = self.backup_root.join(agent_id).join(format!(
                "baseline-{}.atsb",
                &sha256(expected_path.as_bytes())[..16]
            ));
        }

        if baseline_path.exists() {
            let payload = self.read_encrypted_backup(&baseline_path)?;
            if payload.path != expected_path {
                return Err(CommandError::new(
                    "baseline_path_mismatch",
                    "Agent 原始配置备份与当前配置路径不匹配",
                ));
            }
            return Ok(BaselineSnapshot {
                existed: payload.existed,
                content: payload.original_content,
            });
        }

        let payload = if let Some(payload) = self.earliest_matching_backup(agent_id, path)? {
            payload
        } else {
            let existed = path.exists();
            let content = if existed { fs::read(path)? } else { Vec::new() };
            BackupPayload {
                path: expected_path.clone(),
                existed,
                original_sha256: sha256(&content),
                original_content: content,
            }
        };

        let operation_id = baseline_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("baseline");
        self.write_encrypted_backup(agent_id, operation_id, &payload)?;
        let verified = self.read_encrypted_backup(&baseline_path)?;
        if verified.path != expected_path || verified.original_sha256 != payload.original_sha256 {
            return Err(CommandError::new(
                "baseline_verification_failed",
                "Agent 原始配置备份校验失败",
            ));
        }
        Ok(BaselineSnapshot {
            existed: verified.existed,
            content: verified.original_content,
        })
    }

    fn earliest_matching_backup(
        &self,
        agent_id: &str,
        path: &Path,
    ) -> AppResult<Option<BackupPayload>> {
        let directory = self.backup_root.join(agent_id);
        let Ok(entries) = fs::read_dir(directory) else {
            return Ok(None);
        };
        let expected_path = path.to_string_lossy();
        let mut candidates = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|value| value.to_str()) == Some("atsb")
                    && path.file_name().and_then(|value| value.to_str()) != Some("baseline.atsb"))
                .then(|| {
                    let modified = entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .ok();
                    (modified, path)
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(modified, path)| (*modified, path.clone()));

        for (_, candidate) in candidates {
            let Ok(payload) = self.read_encrypted_backup(&candidate) else {
                continue;
            };
            if payload.path == expected_path {
                return Ok(Some(payload));
            }
        }
        Ok(None)
    }

    fn restore_payload(&self, payload: &BackupPayload) -> AppResult<()> {
        let path = PathBuf::from(&payload.path);
        if payload.existed {
            write_atomic(&path, &payload.original_content)?;
            let restored = fs::read(&path)?;
            if sha256(&restored) != payload.original_sha256 {
                return Err(CommandError::new(
                    "backup_restore_verification_failed",
                    "恢复后的配置哈希不匹配",
                ));
            }
        } else if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn write_encrypted_backup(
        &self,
        agent_id: &str,
        operation_id: &str,
        payload: &BackupPayload,
    ) -> AppResult<PathBuf> {
        let key = self.load_or_create_backup_key()?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| CommandError::internal("备份加密密钥长度无效"))?;
        let mut nonce_bytes = [0_u8; 24];
        OsRng.fill_bytes(&mut nonce_bytes);
        let plaintext = serde_json::to_vec(payload)
            .map_err(|_| CommandError::internal("无法序列化配置备份"))?;
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .map_err(|_| CommandError::internal("配置备份加密失败"))?;

        let agent_dir = self.backup_root.join(agent_id);
        fs::create_dir_all(&agent_dir)?;
        let backup_path = agent_dir.join(format!("{operation_id}.atsb"));
        let mut file = create_private_file(&backup_path)?;
        file.write_all(BACKUP_MAGIC)?;
        file.write_all(&nonce_bytes)?;
        file.write_all(&ciphertext)?;
        file.sync_all()?;
        Ok(backup_path)
    }

    fn read_encrypted_backup(&self, path: &Path) -> AppResult<BackupPayload> {
        let bytes = fs::read(path)?;
        if bytes.len() < BACKUP_MAGIC.len() + 24 || &bytes[..5] != BACKUP_MAGIC {
            return Err(CommandError::new(
                "backup_format_invalid",
                "配置备份格式无效",
            ));
        }
        let key = self.load_backup_key()?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| CommandError::internal("备份加密密钥长度无效"))?;
        let nonce = XNonce::from_slice(&bytes[5..29]);
        let plaintext = cipher.decrypt(nonce, &bytes[29..]).map_err(|_| {
            CommandError::new(
                "backup_decryption_failed",
                "配置备份无法解密或完整性校验失败",
            )
        })?;
        serde_json::from_slice(&plaintext)
            .map_err(|_| CommandError::new("backup_payload_invalid", "配置备份内容无效"))
    }

    fn load_or_create_backup_key(&self) -> AppResult<[u8; 32]> {
        if self.secret_store.exists(BACKUP_KEY_REF) {
            return self.load_backup_key();
        }
        let mut key = [0_u8; 32];
        OsRng.fill_bytes(&mut key);
        let encoded = SecretValue::new(BASE64.encode(key));
        self.secret_store.put(BACKUP_KEY_REF, &encoded)?;
        Ok(key)
    }

    fn load_backup_key(&self) -> AppResult<[u8; 32]> {
        let encoded = self.secret_store.get(BACKUP_KEY_REF)?;
        let bytes = BASE64
            .decode(encoded.expose())
            .map_err(|_| CommandError::new("backup_key_invalid", "系统凭据库中的备份密钥无效"))?;
        bytes
            .try_into()
            .map_err(|_| CommandError::new("backup_key_invalid", "备份密钥长度无效"))
    }
}

fn validate_agent_id(agent_id: &str) -> AppResult<()> {
    if agent_id.is_empty()
        || !agent_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CommandError::new("invalid_agent_id", "Agent ID 无效"));
    }
    Ok(())
}

fn sha256(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn create_private_file(path: &Path) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub(crate) fn write_atomic(path: &Path, content: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CommandError::new("invalid_config_path", "配置路径无父目录"))?;
    fs::create_dir_all(parent)?;
    let temporary_path = parent.join(format!(
        ".{}.at-switch-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        Uuid::new_v4()
    ));

    let mut temporary = create_private_file(&temporary_path)?;
    let original_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    temporary.write_all(content)?;
    temporary.sync_all()?;
    drop(temporary);
    if let Some(permissions) = original_permissions {
        fs::set_permissions(&temporary_path, permissions)?;
    }

    if let Err(error) = replace_file(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    sync_parent(parent);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> AppResult<()> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> AppResult<()> {
    use std::{iter, os::windows::ffi::OsStrExt, thread, time::Duration};
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    for attempt in 0..5 {
        // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for
        // the complete Win32 call. Optional backup/progress pointers are null
        // exactly as permitted by ReplaceFileW/MoveFileExW.
        let result = unsafe {
            if destination.exists() {
                ReplaceFileW(
                    destination_wide.as_ptr(),
                    source_wide.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_WRITE_THROUGH,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } else {
                MoveFileExW(
                    source_wide.as_ptr(),
                    destination_wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
        };
        if result != 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let file_is_temporarily_locked = matches!(error.raw_os_error(), Some(32) | Some(33));
        if !file_is_temporarily_locked || attempt == 4 {
            return Err(error.into());
        }
        thread::sleep(Duration::from_millis(100 * (attempt + 1)));
    }
    unreachable!("Windows replacement loop always returns")
}

#[cfg(unix)]
fn sync_parent(parent: &Path) {
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::MemorySecretStore;

    #[test]
    fn writes_an_encrypted_backup_and_can_restore_it() {
        let temp = tempfile::tempdir().expect("temp");
        let config_path = temp.path().join("agent.json");
        fs::write(&config_path, br#"{"model":"old"}"#).expect("seed");
        let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let transaction = ConfigTransaction::new(store, temp.path().join("backups"));

        let result = transaction
            .apply_file(
                "codex",
                FileChange {
                    path: config_path.clone(),
                    new_content: br#"{"model":"new"}"#.to_vec(),
                },
            )
            .expect("apply");
        assert_eq!(
            fs::read_to_string(&config_path).expect("read"),
            r#"{"model":"new"}"#
        );
        let backup_bytes = fs::read(&result.backup_path).expect("backup");
        assert!(backup_bytes.starts_with(BACKUP_MAGIC));
        assert!(!backup_bytes
            .windows(b"old".len())
            .any(|window| window == b"old"));

        transaction
            .restore_backup(&result.backup_path)
            .expect("restore");
        assert_eq!(
            fs::read_to_string(&config_path).expect("read"),
            r#"{"model":"old"}"#
        );
    }

    #[test]
    fn baseline_recovers_the_state_before_the_first_managed_write() {
        let temp = tempfile::tempdir().expect("temp");
        let config_path = temp.path().join("agent.json");
        fs::write(&config_path, b"native").expect("seed");
        let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let transaction = ConfigTransaction::new(store, temp.path().join("backups"));

        transaction
            .apply_file(
                "workbuddy",
                FileChange {
                    path: config_path.clone(),
                    new_content: b"managed-v1".to_vec(),
                },
            )
            .expect("first apply");
        let baseline = transaction
            .baseline("workbuddy", &config_path)
            .expect("baseline");
        assert!(baseline.existed);
        assert_eq!(baseline.content, b"native");

        transaction
            .apply_file(
                "workbuddy",
                FileChange {
                    path: config_path.clone(),
                    new_content: b"managed-v2".to_vec(),
                },
            )
            .expect("second apply");
        let stable = transaction
            .baseline("workbuddy", &config_path)
            .expect("stable baseline");
        assert_eq!(stable.content, b"native");
    }

    #[test]
    fn baseline_supports_an_adapter_moving_to_a_new_authoritative_file() {
        let temp = tempfile::tempdir().expect("temp");
        let generated_path = temp.path().join("openclaw.json");
        let authoritative_path = temp.path().join("settings.json");
        fs::write(&generated_path, b"generated-native").expect("generated");
        fs::write(&authoritative_path, b"settings-native").expect("settings");
        let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let transaction = ConfigTransaction::new(store, temp.path().join("backups"));

        let first = transaction
            .baseline("autoclaw", &generated_path)
            .expect("legacy baseline");
        assert_eq!(first.content, b"generated-native");
        let migrated = transaction
            .baseline("autoclaw", &authoritative_path)
            .expect("path-scoped baseline");
        assert_eq!(migrated.content, b"settings-native");
        assert!(
            temp.path()
                .join("backups/autoclaw")
                .read_dir()
                .expect("backups")
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("baseline"))
                .count()
                >= 2
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("config.json");
        fs::write(&path, b"old").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("mode");
        write_atomic(&path, b"new").expect("write");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }
}
