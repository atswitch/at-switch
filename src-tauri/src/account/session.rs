use std::sync::Arc;

use crate::infrastructure::{SecretStore, SecretValue};

use super::types::StoredTokens;

/// 会话凭据存取：token 整体序列化后写入 OS 凭据库（Keychain / Credential Manager），
/// 引用 `account/session/v{n}`；SQLite 与日志中永不出现 token 明文。
pub struct SessionStore {
    store: Arc<dyn SecretStore>,
}

const SESSION_REFERENCE: &str = "account/session/v1";

impl SessionStore {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store }
    }

    pub fn load(&self) -> Option<StoredTokens> {
        let secret = self.store.get(SESSION_REFERENCE).ok()?;
        serde_json::from_str(secret.expose()).ok()
    }

    pub fn save(&self, tokens: &StoredTokens) -> Result<(), crate::domain::CommandError> {
        let payload = serde_json::to_string(tokens)
            .map_err(|_| crate::domain::CommandError::internal("无法序列化会员会话数据"))?;
        self.store
            .put(SESSION_REFERENCE, &SecretValue::new(payload))
    }

    pub fn clear(&self) {
        let _ = self.store.delete(SESSION_REFERENCE);
    }
}
