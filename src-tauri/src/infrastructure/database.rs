use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, MutexGuard},
};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{
    AgentSummary, ApiProtocol, AppResult, AppSettings, CommandError, ModelDraft,
    ModelOutputModality, ModelSummary, ProviderDraft, ProviderKind, ProviderSummary,
    VerificationStatus,
};

const MODEL_VERIFICATION_MIGRATION: i64 = 2026080101;
const MODEL_OUTPUT_MODALITY_MIGRATION: i64 = 2026080201;
/// 一次性清理：删除早期版本遗留的「占位 Provider」——既没有模型，也没有保存
/// 真实 API Key。这些条目会让首页出现一条无法切换的空壳卡片，干扰用户。本次
/// 迁移把它们及其可能存在的 agent_bindings 一并物理删除，让首页回到「无任何
/// 默认供应商」的状态，完全交给用户自行添加。
const PLACEHOLDER_PROVIDER_PURGE_MIGRATION: i64 = 2026080801;
const CUSTOM_AGENT_INSTALL_PATH_MIGRATION: i64 = 2026081101;
const PROVIDER_RECOMMENDATION_REMOVAL_MIGRATION: i64 = 2026090401;

pub struct Database {
    connection: Mutex<Connection>,
}

pub(crate) struct StoredProvider {
    pub summary: ProviderSummary,
    pub api_key_ref: Option<String>,
    pub api_key_revision: i64,
}

pub(crate) struct SecretMetadata {
    pub reference: Option<String>,
    pub revision: i64,
    pub masked: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredAgentBinding {
    pub agent_id: String,
    pub mode: String,
    pub provider_id: String,
    pub default_model_id: String,
    pub request_protocol: ApiProtocol,
    pub local_token_ref: Option<String>,
    pub local_token_revision: i64,
}

#[derive(Debug, Clone)]
struct StoredModelVerification {
    output_modality: ModelOutputModality,
    supports_streaming: bool,
    supports_tools: bool,
    status: VerificationStatus,
    fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRuntimeSelection {
    pub scope_id: String,
    pub original_value: Option<String>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.initialize()?;
        Ok(database)
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        let database = Self {
            connection: Mutex::new(Connection::open_in_memory()?),
        };
        database.initialize()?;
        Ok(database)
    }

    fn connection(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| CommandError::internal("本地数据库锁已损坏"))
    }

    fn initialize(&self) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                protocol TEXT NOT NULL,
                base_url TEXT NOT NULL,
                is_recommended INTEGER NOT NULL DEFAULT 0,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                api_key_ref TEXT,
                api_key_revision INTEGER NOT NULL DEFAULT 0,
                masked_api_key TEXT,
                verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                verification_fingerprint TEXT,
                verified_model_id TEXT,
                default_model_id TEXT,
                allow_insecure_http INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS models (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                model_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                output_modality TEXT NOT NULL DEFAULT 'text',
                supports_streaming INTEGER NOT NULL DEFAULT 1,
                supports_tools INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'custom',
                verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                verification_fingerprint TEXT,
                UNIQUE(provider_id, model_id)
            );

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                custom_config_path TEXT,
                custom_install_path TEXT,
                detected_version TEXT,
                install_status TEXT NOT NULL DEFAULT 'not_installed',
                config_health TEXT NOT NULL DEFAULT 'unsupported_version',
                resource_fingerprint TEXT,
                adapter_verified INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_bindings (
                agent_id TEXT PRIMARY KEY REFERENCES agents(id) ON DELETE CASCADE,
                mode TEXT NOT NULL,
                provider_id TEXT NOT NULL REFERENCES providers(id),
                default_model_id TEXT NOT NULL,
                request_protocol TEXT NOT NULL,
                local_token_ref TEXT,
                local_token_revision INTEGER NOT NULL DEFAULT 0,
                verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_runtime_selections (
                agent_id TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                original_value TEXT,
                created_at TEXT NOT NULL,
                PRIMARY KEY(agent_id, scope_id)
            );

            CREATE TABLE IF NOT EXISTS proxy_settings (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                port INTEGER NOT NULL,
                desired_running INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS operation_records (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                target_id TEXT,
                phase TEXT NOT NULL,
                outcome TEXT,
                safe_error_code TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            INSERT OR IGNORE INTO proxy_settings(singleton, port, desired_running)
            VALUES (1, 54187, 0);
            "#,
        )?;
        ensure_column(&connection, "providers", "verified_model_id", "TEXT")?;
        let custom_install_path_migrated = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [CUSTOM_AGENT_INSTALL_PATH_MIGRATION],
            |row| row.get::<_, bool>(0),
        )?;
        if !custom_install_path_migrated {
            ensure_column(&connection, "agents", "custom_install_path", "TEXT")?;
            connection.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![CUSTOM_AGENT_INSTALL_PATH_MIGRATION, Utc::now().to_rfc3339()],
            )?;
        }
        ensure_column(
            &connection,
            "models",
            "verification_status",
            "TEXT NOT NULL DEFAULT 'draft_unverified'",
        )?;
        ensure_column(&connection, "models", "verification_fingerprint", "TEXT")?;
        let output_modality_migrated = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [MODEL_OUTPUT_MODALITY_MIGRATION],
            |row| row.get::<_, bool>(0),
        )?;
        if !output_modality_migrated {
            ensure_column(
                &connection,
                "models",
                "output_modality",
                "TEXT NOT NULL DEFAULT 'text'",
            )?;
            connection.execute(
                r#"
                UPDATE models
                SET output_modality = 'text'
                WHERE output_modality NOT IN ('text', 'image', 'audio', 'video')
                "#,
                [],
            )?;
            connection.execute(
                r#"
                UPDATE models
                SET output_modality = 'image'
                WHERE model_id = 'doubao-seedream-5.0-lite'
                "#,
                [],
            )?;
            connection.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![MODEL_OUTPUT_MODALITY_MIGRATION, Utc::now().to_rfc3339()],
            )?;
        }
        connection.execute(
            r#"
            UPDATE providers
            SET verification_status = 'stale', verification_fingerprint = NULL
            WHERE verification_status = 'verified' AND verified_model_id IS NULL
            "#,
            [],
        )?;
        let model_verification_migrated = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [MODEL_VERIFICATION_MIGRATION],
            |row| row.get::<_, bool>(0),
        )?;
        if !model_verification_migrated {
            let now = Utc::now().to_rfc3339();
            connection.execute(
                r#"
                UPDATE models
                SET verification_status = COALESCE(
                        (SELECT providers.verification_status
                         FROM providers
                         WHERE providers.id = models.provider_id),
                        'draft_unverified'
                    ),
                    verification_fingerprint = (
                        SELECT providers.verification_fingerprint
                        FROM providers
                        WHERE providers.id = models.provider_id
                    )
                "#,
                [],
            )?;
            connection.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![MODEL_VERIFICATION_MIGRATION, now],
            )?;
        }

        let placeholder_purge_migrated = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [PLACEHOLDER_PROVIDER_PURGE_MIGRATION],
            |row| row.get::<_, bool>(0),
        )?;
        if !placeholder_purge_migrated {
            // 占位 Provider 指早期版本写入的、既无模型也未保存 API Key 的空壳记录。
            // 它们无法被切换，只会让首页出现一条干扰卡片。这里一次性物理删除，
            // 同时清理可能挂在它们上面的 agent_bindings（正常情况下不会存在，因为
            // 没有模型无法绑定，但为了外键安全仍然显式清理）。
            let placeholder_ids: Vec<String> = {
                let mut stmt = connection.prepare(
                    r#"
                    SELECT p.id
                    FROM providers p
                    LEFT JOIN models m ON m.provider_id = p.id
                    WHERE p.api_key_ref IS NULL
                      AND p.masked_api_key IS NULL
                      AND m.id IS NULL
                    "#,
                )?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for id in &placeholder_ids {
                connection.execute("DELETE FROM agent_bindings WHERE provider_id = ?1", [id])?;
                connection.execute("DELETE FROM providers WHERE id = ?1", [id])?;
            }
            connection.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![
                    PLACEHOLDER_PROVIDER_PURGE_MIGRATION,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }

        let provider_recommendation_removed = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [PROVIDER_RECOMMENDATION_REMOVAL_MIGRATION],
            |row| row.get::<_, bool>(0),
        )?;
        if !provider_recommendation_removed {
            connection.execute("UPDATE providers SET is_recommended = 0", [])?;
            connection.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![
                    PROVIDER_RECOMMENDATION_REMOVAL_MIGRATION,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }

        Ok(())
    }

    pub(crate) fn provider_secret_metadata(&self, provider_id: &str) -> AppResult<SecretMetadata> {
        let connection = self.connection()?;
        let metadata = connection
            .query_row(
                "SELECT api_key_ref, api_key_revision, masked_api_key FROM providers WHERE id = ?1",
                [provider_id],
                |row| {
                    Ok(SecretMetadata {
                        reference: row.get(0)?,
                        revision: row.get(1)?,
                        masked: row.get(2)?,
                    })
                },
            )
            .optional()?
            .unwrap_or(SecretMetadata {
                reference: None,
                revision: 0,
                masked: None,
            });
        Ok(metadata)
    }

    #[cfg(test)]
    pub(crate) fn save_provider(
        &self,
        id: &str,
        draft: &ProviderDraft,
        secret_ref: Option<&str>,
        secret_revision: i64,
        masked_secret: Option<&str>,
    ) -> AppResult<()> {
        self.save_provider_merging(id, draft, secret_ref, secret_revision, masked_secret, &[])
    }

    pub(crate) fn save_provider_merging(
        &self,
        id: &str,
        draft: &ProviderDraft,
        secret_ref: Option<&str>,
        secret_revision: i64,
        masked_secret: Option<&str>,
        duplicate_provider_ids: &[String],
    ) -> AppResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let now = Utc::now().to_rfc3339();
        let previous_connection = transaction
            .query_row(
                r#"
                SELECT kind, protocol, base_url, api_key_revision
                FROM providers WHERE id = ?1
                "#,
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let connection_unchanged = previous_connection.is_some_and(
            |(kind, protocol, base_url, previous_secret_revision)| {
                kind == draft.kind.as_str()
                    && protocol == draft.protocol.as_str()
                    && base_url.trim().trim_end_matches('/')
                        == draft.base_url.trim().trim_end_matches('/')
                    && previous_secret_revision == secret_revision
            },
        );
        let previous_models = {
            let mut statement = transaction.prepare(
                r#"
                SELECT model_id, output_modality, supports_streaming, supports_tools,
                       verification_status, verification_fingerprint
                FROM models WHERE provider_id = ?1
                "#,
            )?;
            let rows = statement.query_map([id], |row| {
                let output_modality: String = row.get(1)?;
                let status: String = row.get(4)?;
                Ok((
                    row.get::<_, String>(0)?,
                    StoredModelVerification {
                        output_modality: ModelOutputModality::parse(&output_modality),
                        supports_streaming: row.get::<_, i64>(2)? != 0,
                        supports_tools: row.get::<_, i64>(3)? != 0,
                        status: VerificationStatus::parse(&status),
                        fingerprint: row.get(5)?,
                    },
                ))
            })?;
            rows.collect::<Result<HashMap<_, _>, _>>()?
        };

        transaction.execute(
            r#"
            INSERT INTO providers(
                id, name, kind, protocol, base_url, is_recommended, is_enabled,
                api_key_ref, api_key_revision, masked_api_key, verification_status,
                default_model_id, allow_insecure_http, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, 'draft_unverified',
                      ?10, ?11, ?12, ?12)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                protocol = excluded.protocol,
                base_url = excluded.base_url,
                is_recommended = excluded.is_recommended,
                api_key_ref = excluded.api_key_ref,
                api_key_revision = excluded.api_key_revision,
                masked_api_key = excluded.masked_api_key,
                default_model_id = excluded.default_model_id,
                allow_insecure_http = excluded.allow_insecure_http,
                updated_at = excluded.updated_at
            "#,
            params![
                id,
                draft.name.trim(),
                draft.kind.as_str(),
                draft.protocol.as_str(),
                draft.base_url.trim(),
                0_i64,
                secret_ref,
                secret_revision,
                masked_secret,
                draft.default_model_id,
                i64::from(draft.allow_insecure_http),
                now
            ],
        )?;

        transaction.execute("DELETE FROM models WHERE provider_id = ?1", [id])?;
        for model in &draft.models {
            let previous = previous_models.get(model.model_id.trim());
            let capabilities_unchanged = previous.is_some_and(|previous| {
                previous.output_modality == model.output_modality
                    && previous.supports_streaming == model.supports_streaming
                    && previous.supports_tools == model.supports_tools
            });
            let (status, fingerprint) = match previous {
                Some(previous) if connection_unchanged && capabilities_unchanged => {
                    (previous.status, previous.fingerprint.as_deref())
                }
                Some(previous) if previous.status != VerificationStatus::DraftUnverified => {
                    (VerificationStatus::Stale, None)
                }
                _ => (VerificationStatus::DraftUnverified, None),
            };
            insert_model(&transaction, id, model, status, fingerprint)?;
        }
        for duplicate_id in duplicate_provider_ids {
            if duplicate_id == id {
                continue;
            }
            transaction.execute(
                "UPDATE agent_bindings SET provider_id = ?1 WHERE provider_id = ?2",
                params![id, duplicate_id],
            )?;
            transaction.execute("DELETE FROM providers WHERE id = ?1", [duplicate_id])?;
        }
        sync_provider_verification_summary(&transaction, id, &now)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn affected_agent_ids_for_provider(
        &self,
        provider_id: &str,
    ) -> AppResult<Vec<String>> {
        let connection = self.connection()?;
        let mut stmt =
            connection.prepare("SELECT agent_id FROM agent_bindings WHERE provider_id = ?1")?;
        let ids = stmt
            .query_map([provider_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    pub(crate) fn delete_provider(&self, id: &str) -> AppResult<(Option<String>, Vec<String>)> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        let secret_ref: Option<String> = transaction
            .query_row(
                "SELECT api_key_ref FROM providers WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let token_refs: Vec<String> = {
            let mut token_stmt = transaction
                .prepare("SELECT local_token_ref FROM agent_bindings WHERE provider_id = ?1 AND local_token_ref IS NOT NULL")?;
            let result = token_stmt
                .query_map([id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            result
        };

        transaction.execute("DELETE FROM agent_bindings WHERE provider_id = ?1", [id])?;
        transaction.execute("DELETE FROM models WHERE provider_id = ?1", [id])?;
        transaction.execute("DELETE FROM providers WHERE id = ?1", [id])?;

        transaction.commit()?;
        Ok((secret_ref, token_refs))
    }

    pub(crate) fn list_providers(&self) -> AppResult<Vec<ProviderSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT id, name, kind, protocol, base_url, is_recommended, is_enabled,
                   api_key_ref IS NOT NULL, masked_api_key, verification_status,
                   verified_model_id, default_model_id
            FROM providers
            ORDER BY is_recommended DESC, created_at ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let kind: String = row.get(2)?;
            let protocol: String = row.get(3)?;
            let verification: String = row.get(9)?;
            Ok(ProviderSummary {
                id,
                name: row.get(1)?,
                kind: ProviderKind::parse(&kind).unwrap_or(ProviderKind::Custom),
                protocol: ApiProtocol::parse(&protocol)
                    .unwrap_or(ApiProtocol::OpenaiChatCompletions),
                base_url: row.get(4)?,
                is_recommended: row.get::<_, i64>(5)? != 0,
                is_enabled: row.get::<_, i64>(6)? != 0,
                has_api_key: row.get::<_, i64>(7)? != 0,
                masked_api_key: row.get(8)?,
                verification_status: VerificationStatus::parse(&verification),
                verified_model_id: row.get(10)?,
                default_model_id: row.get(11)?,
                models: Vec::new(),
            })
        })?;

        let mut providers = rows.collect::<Result<Vec<_>, _>>()?;
        for provider in &mut providers {
            provider.models = self.list_models_with_connection(&connection, &provider.id)?;
        }
        Ok(providers)
    }

    pub(crate) fn get_provider(&self, provider_id: &str) -> AppResult<StoredProvider> {
        let connection = self.connection()?;
        let mut summary = connection
            .query_row(
                r#"
                SELECT id, name, kind, protocol, base_url, is_recommended, is_enabled,
                       api_key_ref IS NOT NULL, masked_api_key, verification_status,
                       verified_model_id, default_model_id, api_key_ref, api_key_revision
                FROM providers WHERE id = ?1
                "#,
                [provider_id],
                |row| {
                    let kind: String = row.get(2)?;
                    let protocol: String = row.get(3)?;
                    let verification: String = row.get(9)?;
                    Ok(StoredProvider {
                        summary: ProviderSummary {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            kind: ProviderKind::parse(&kind).unwrap_or(ProviderKind::Custom),
                            protocol: ApiProtocol::parse(&protocol)
                                .unwrap_or(ApiProtocol::OpenaiChatCompletions),
                            base_url: row.get(4)?,
                            is_recommended: row.get::<_, i64>(5)? != 0,
                            is_enabled: row.get::<_, i64>(6)? != 0,
                            has_api_key: row.get::<_, i64>(7)? != 0,
                            masked_api_key: row.get(8)?,
                            verification_status: VerificationStatus::parse(&verification),
                            verified_model_id: row.get(10)?,
                            default_model_id: row.get(11)?,
                            models: Vec::new(),
                        },
                        api_key_ref: row.get(12)?,
                        api_key_revision: row.get(13)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| CommandError::new("provider_not_found", "Provider 不存在"))?;
        summary.summary.models =
            self.list_models_with_connection(&connection, &summary.summary.id)?;
        Ok(summary)
    }

    fn list_models_with_connection(
        &self,
        connection: &Connection,
        provider_id: &str,
    ) -> AppResult<Vec<ModelSummary>> {
        let mut statement = connection.prepare(
            r#"
            SELECT id, provider_id, model_id, display_name, output_modality,
                   supports_streaming, supports_tools, source, verification_status
            FROM models WHERE provider_id = ?1 ORDER BY rowid ASC
            "#,
        )?;
        let rows = statement.query_map([provider_id], |row| {
            let output_modality: String = row.get(4)?;
            let verification: String = row.get(8)?;
            Ok(ModelSummary {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model_id: row.get(2)?,
                display_name: row.get(3)?,
                output_modality: ModelOutputModality::parse(&output_modality),
                supports_streaming: row.get::<_, i64>(5)? != 0,
                supports_tools: row.get::<_, i64>(6)? != 0,
                source: row.get(7)?,
                verification_status: VerificationStatus::parse(&verification),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn mark_model_verification(
        &self,
        provider_id: &str,
        model_id: &str,
        status: VerificationStatus,
        fingerprint: Option<&str>,
    ) -> AppResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            r#"
            UPDATE models
            SET verification_status = ?3,
                verification_fingerprint = ?4
            WHERE provider_id = ?1 AND model_id = ?2
            "#,
            params![provider_id, model_id, status.as_str(), fingerprint,],
        )?;
        if updated != 1 {
            return Err(CommandError::new(
                "model_not_found",
                "要验证的模型不存在或已被移除",
            ));
        }
        sync_provider_verification_summary(&transaction, provider_id, &Utc::now().to_rfc3339())?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn proxy_port(&self) -> AppResult<u16> {
        let connection = self.connection()?;
        let port: i64 = connection.query_row(
            "SELECT port FROM proxy_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        u16::try_from(port).map_err(|_| CommandError::internal("数据库中的代理端口无效"))
    }

    pub(crate) fn update_proxy_port(&self, port: u16) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE proxy_settings SET port = ?1 WHERE singleton = 1",
            [i64::from(port)],
        )?;
        Ok(())
    }

    pub(crate) fn upsert_agent_state(&self, agent: &AgentSummary) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO agents(
                id, custom_config_path, detected_version, install_status,
                config_health, adapter_verified, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                custom_config_path = excluded.custom_config_path,
                detected_version = excluded.detected_version,
                install_status = excluded.install_status,
                config_health = excluded.config_health,
                adapter_verified = excluded.adapter_verified,
                updated_at = excluded.updated_at
            "#,
            params![
                agent.id,
                agent.config_path,
                agent.detected_version,
                agent.install_status.as_str(),
                agent.config_health.as_str(),
                i64::from(agent.adapter_verified),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub(crate) fn custom_agent_install_paths(&self) -> AppResult<HashMap<String, String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, custom_install_path FROM agents WHERE custom_install_path IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
    }

    pub(crate) fn set_custom_agent_install_path(
        &self,
        agent_id: &str,
        path: Option<&str>,
    ) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO agents(id, custom_install_path, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                custom_install_path = excluded.custom_install_path,
                updated_at = excluded.updated_at
            "#,
            params![agent_id, path, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub(crate) fn list_agent_bindings(&self) -> AppResult<Vec<StoredAgentBinding>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT agent_id, mode, provider_id, default_model_id,
                   request_protocol, local_token_ref, local_token_revision
            FROM agent_bindings
            ORDER BY agent_id
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            let protocol: String = row.get(4)?;
            Ok(StoredAgentBinding {
                agent_id: row.get(0)?,
                mode: row.get(1)?,
                provider_id: row.get(2)?,
                default_model_id: row.get(3)?,
                request_protocol: ApiProtocol::parse(&protocol)
                    .unwrap_or(ApiProtocol::OpenaiChatCompletions),
                local_token_ref: row.get(5)?,
                local_token_revision: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn get_agent_binding(
        &self,
        agent_id: &str,
    ) -> AppResult<Option<StoredAgentBinding>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT agent_id, mode, provider_id, default_model_id,
                       request_protocol, local_token_ref, local_token_revision
                FROM agent_bindings WHERE agent_id = ?1
                "#,
                [agent_id],
                |row| {
                    let protocol: String = row.get(4)?;
                    Ok(StoredAgentBinding {
                        agent_id: row.get(0)?,
                        mode: row.get(1)?,
                        provider_id: row.get(2)?,
                        default_model_id: row.get(3)?,
                        request_protocol: ApiProtocol::parse(&protocol)
                            .unwrap_or(ApiProtocol::OpenaiChatCompletions),
                        local_token_ref: row.get(5)?,
                        local_token_revision: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn save_agent_binding(&self, binding: &StoredAgentBinding) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO agent_bindings(
                agent_id, mode, provider_id, default_model_id,
                request_protocol, local_token_ref, local_token_revision,
                verification_status, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'verified', ?8)
            ON CONFLICT(agent_id) DO UPDATE SET
                mode = excluded.mode,
                provider_id = excluded.provider_id,
                default_model_id = excluded.default_model_id,
                request_protocol = excluded.request_protocol,
                local_token_ref = excluded.local_token_ref,
                local_token_revision = excluded.local_token_revision,
                verification_status = excluded.verification_status,
                updated_at = excluded.updated_at
            "#,
            params![
                binding.agent_id,
                binding.mode,
                binding.provider_id,
                binding.default_model_id,
                binding.request_protocol.as_str(),
                binding.local_token_ref,
                binding.local_token_revision,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_agent_binding_and_runtime_selections(
        &self,
        agent_id: &str,
    ) -> AppResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM agent_bindings WHERE agent_id = ?1", [agent_id])?;
        transaction.execute(
            "DELETE FROM agent_runtime_selections WHERE agent_id = ?1",
            [agent_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes runtime selections for an agent without touching the binding
    /// row.  Used when the binding has already been removed by a provider
    /// deletion transaction.
    pub(crate) fn delete_runtime_selections(&self, agent_id: &str) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM agent_runtime_selections WHERE agent_id = ?1",
            [agent_id],
        )?;
        Ok(())
    }

    pub(crate) fn remember_runtime_selection(
        &self,
        agent_id: &str,
        scope_id: &str,
        original_value: Option<&str>,
    ) -> AppResult<bool> {
        let connection = self.connection()?;
        let inserted = connection.execute(
            r#"
            INSERT OR IGNORE INTO agent_runtime_selections(
                agent_id, scope_id, original_value, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
            params![agent_id, scope_id, original_value, Utc::now().to_rfc3339()],
        )?;
        Ok(inserted == 1)
    }

    pub(crate) fn list_runtime_selections(
        &self,
        agent_id: &str,
    ) -> AppResult<Vec<StoredRuntimeSelection>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT scope_id, original_value
            FROM agent_runtime_selections
            WHERE agent_id = ?1
            ORDER BY created_at, scope_id
            "#,
        )?;
        let rows = statement.query_map([agent_id], |row| {
            Ok(StoredRuntimeSelection {
                scope_id: row.get(0)?,
                original_value: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn forget_runtime_selection(&self, agent_id: &str, scope_id: &str) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM agent_runtime_selections WHERE agent_id = ?1 AND scope_id = ?2",
            params![agent_id, scope_id],
        )?;
        Ok(())
    }

    pub(crate) fn settings(&self) -> AppResult<AppSettings> {
        let connection = self.connection()?;
        let value: Option<String> = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'app'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| {
                serde_json::from_str(&value).map_err(|_| CommandError::internal("应用设置格式无效"))
            })
            .transpose()
            .map(|settings| settings.unwrap_or_default())
    }

    pub(crate) fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        let connection = self.connection()?;
        let json = serde_json::to_string(settings)
            .map_err(|_| CommandError::internal("无法序列化应用设置"))?;
        connection.execute(
            r#"
            INSERT INTO settings(key, value_json, updated_at)
            VALUES ('app', ?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                                           updated_at = excluded.updated_at
            "#,
            params![json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> AppResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    let exists = columns
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|existing| existing == column);
    if !exists {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

fn insert_model(
    transaction: &Transaction<'_>,
    provider_id: &str,
    model: &ModelDraft,
    verification_status: VerificationStatus,
    verification_fingerprint: Option<&str>,
) -> AppResult<()> {
    let model_id = model.model_id.trim();
    if model_id.is_empty() {
        return Ok(());
    }
    transaction.execute(
        r#"
        INSERT INTO models(
            id, provider_id, model_id, display_name, output_modality,
            supports_streaming, supports_tools, source, verification_status,
            verification_fingerprint
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'custom', ?8, ?9)
        "#,
        params![
            format!("{provider_id}:{model_id}"),
            provider_id,
            model_id,
            if model.display_name.trim().is_empty() {
                model_id
            } else {
                model.display_name.trim()
            },
            model.output_modality.as_str(),
            i64::from(model.supports_streaming),
            i64::from(model.supports_tools),
            verification_status.as_str(),
            verification_fingerprint,
        ],
    )?;
    Ok(())
}

fn sync_provider_verification_summary(
    transaction: &Transaction<'_>,
    provider_id: &str,
    updated_at: &str,
) -> AppResult<()> {
    let statuses = {
        let mut statement = transaction.prepare(
            r#"
            SELECT verification_status
            FROM models
            WHERE provider_id = ?1 AND output_modality = 'text'
            ORDER BY rowid
            "#,
        )?;
        let rows = statement.query_map([provider_id], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let status = if statuses.is_empty() {
        VerificationStatus::DraftUnverified
    } else if statuses.iter().any(|status| status == "verifying") {
        VerificationStatus::Verifying
    } else if statuses.iter().any(|status| status == "failed") {
        VerificationStatus::Failed
    } else if statuses.iter().all(|status| status == "verified") {
        VerificationStatus::Verified
    } else if statuses.iter().any(|status| status == "stale") {
        VerificationStatus::Stale
    } else {
        VerificationStatus::DraftUnverified
    };
    let verified = transaction
        .query_row(
            r#"
            SELECT model_id, verification_fingerprint
            FROM models
            WHERE provider_id = ?1
              AND output_modality = 'text'
              AND verification_status = 'verified'
            ORDER BY CASE
                WHEN model_id = (SELECT default_model_id FROM providers WHERE id = ?1) THEN 0
                ELSE 1
            END, rowid
            LIMIT 1
            "#,
            [provider_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let (verified_model_id, fingerprint) = verified
        .map(|(model_id, fingerprint)| (Some(model_id), fingerprint))
        .unwrap_or((None, None));
    transaction.execute(
        r#"
        UPDATE providers
        SET verification_status = ?2,
            verification_fingerprint = ?3,
            verified_model_id = ?4,
            updated_at = ?5
        WHERE id = ?1
        "#,
        params![
            provider_id,
            status.as_str(),
            fingerprint,
            verified_model_id,
            updated_at,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_with_empty_providers() {
        let database = Database::in_memory().expect("database");
        let providers = database.list_providers().expect("providers");
        assert!(providers.is_empty());
    }

    #[test]
    fn custom_agent_install_path_round_trips_and_can_be_cleared() {
        let database = Database::in_memory().expect("database");
        database
            .set_custom_agent_install_path("workbuddy", Some("D:/Agents/WorkBuddy"))
            .expect("save path");
        assert_eq!(
            database
                .custom_agent_install_paths()
                .expect("custom paths")
                .get("workbuddy")
                .map(String::as_str),
            Some("D:/Agents/WorkBuddy")
        );

        database
            .set_custom_agent_install_path("workbuddy", None)
            .expect("clear path");
        assert!(database
            .custom_agent_install_paths()
            .expect("custom paths")
            .is_empty());
    }

    #[test]
    fn migrates_an_existing_agents_table_for_custom_install_paths() {
        let connection = Connection::open_in_memory().expect("legacy database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                CREATE TABLE agents (
                    id TEXT PRIMARY KEY,
                    custom_config_path TEXT,
                    detected_version TEXT,
                    install_status TEXT NOT NULL DEFAULT 'not_installed',
                    config_health TEXT NOT NULL DEFAULT 'unsupported_version',
                    resource_fingerprint TEXT,
                    adapter_verified INTEGER NOT NULL DEFAULT 0,
                    updated_at TEXT NOT NULL
                );
                "#,
            )
            .expect("legacy schema");
        let database = Database {
            connection: Mutex::new(connection),
        };

        database.initialize().expect("migration");

        database
            .set_custom_agent_install_path("qclaw", Some("D:/QClaw"))
            .expect("save migrated field");
        assert_eq!(
            database
                .custom_agent_install_paths()
                .expect("custom paths")
                .get("qclaw")
                .map(String::as_str),
            Some("D:/QClaw")
        );
        let connection = database.connection().expect("connection");
        let migration_recorded: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [CUSTOM_AGENT_INSTALL_PATH_MIGRATION],
                |row| row.get(0),
            )
            .expect("migration marker");
        assert!(migration_recorded);
    }

    #[test]
    fn migration_removes_legacy_provider_recommendations() {
        let database = Database::in_memory().expect("database");
        {
            let connection = database.connection().expect("connection");
            connection
                .execute(
                    r#"
                    INSERT INTO providers(
                        id, name, kind, protocol, base_url, is_recommended,
                        created_at, updated_at
                    ) VALUES (
                        'legacy-recommended', 'Legacy Provider', 'mongyun',
                        'openai_chat_completions', 'https://api.example.test/v1', 1,
                        '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                    )
                    "#,
                    [],
                )
                .expect("seed legacy recommendation");
            connection
                .execute(
                    "DELETE FROM schema_migrations WHERE version = ?1",
                    [PROVIDER_RECOMMENDATION_REMOVAL_MIGRATION],
                )
                .expect("reset recommendation migration");
        }

        database.initialize().expect("rerun migration");

        let provider = database
            .get_provider("legacy-recommended")
            .expect("provider after migration");
        assert!(!provider.summary.is_recommended);
        let connection = database.connection().expect("connection");
        let migration_recorded: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [PROVIDER_RECOMMENDATION_REMOVAL_MIGRATION],
                |row| row.get(0),
            )
            .expect("migration marker");
        assert!(migration_recorded);
    }

    #[test]
    fn migrates_legacy_models_to_text_output_modality() {
        let connection = Connection::open_in_memory().expect("legacy database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE models (
                    id TEXT PRIMARY KEY,
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    supports_streaming INTEGER NOT NULL DEFAULT 1,
                    supports_tools INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'custom',
                    UNIQUE(provider_id, model_id)
                );
                INSERT INTO models(
                    id, provider_id, model_id, display_name,
                    supports_streaming, supports_tools, source
                ) VALUES (
                    'legacy:model-a', 'legacy', 'model-a', 'Model A', 1, 1, 'custom'
                ), (
                    'legacy:seedream', 'legacy', 'doubao-seedream-5.0-lite',
                    'Doubao Seedream 5.0 Lite', 0, 0, 'custom'
                );
                "#,
            )
            .expect("legacy models");
        let database = Database {
            connection: Mutex::new(connection),
        };

        database.initialize().expect("migrate database");

        let connection = database.connection().expect("connection");
        let output_modality: String = connection
            .query_row(
                "SELECT output_modality FROM models WHERE id = 'legacy:model-a'",
                [],
                |row| row.get(0),
            )
            .expect("migrated output modality");
        let seedream_output_modality: String = connection
            .query_row(
                "SELECT output_modality FROM models WHERE id = 'legacy:seedream'",
                [],
                |row| row.get(0),
            )
            .expect("migrated preset output modality");
        let migration_recorded: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [MODEL_OUTPUT_MODALITY_MIGRATION],
                |row| row.get(0),
            )
            .expect("migration marker");
        assert_eq!(output_modality, "text");
        assert_eq!(seedream_output_modality, "image");
        assert!(migration_recorded);
    }

    #[test]
    fn provider_models_round_trip_without_secret_values() {
        let database = Database::in_memory().expect("database");
        let draft = ProviderDraft {
            id: None,
            name: "Test".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiResponses,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Image,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-1",
                &draft,
                Some("provider/provider-1/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save");
        let provider = database.get_provider("provider-1").expect("provider");
        assert_eq!(provider.summary.models[0].model_id, "model-a");
        assert_eq!(
            provider.summary.models[0].output_modality,
            ModelOutputModality::Image
        );
        assert_eq!(provider.api_key_revision, 1);
        assert_eq!(
            provider.api_key_ref.as_deref(),
            Some("provider/provider-1/api-key/v1")
        );
    }

    #[test]
    fn saving_a_provider_never_marks_it_as_recommended() {
        let database = Database::in_memory().expect("database");
        let draft = ProviderDraft {
            id: None,
            name: "Preset Provider".to_owned(),
            kind: ProviderKind::Mongyun,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://api.example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };

        database
            .save_provider("provider-preset", &draft, None, 0, None)
            .expect("save preset provider");

        let provider = database
            .get_provider("provider-preset")
            .expect("preset provider");
        assert!(!provider.summary.is_recommended);
    }

    #[test]
    fn model_verification_updates_the_provider_summary() {
        let database = Database::in_memory().expect("database");
        let draft = ProviderDraft {
            id: None,
            name: "Test".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-verified",
                &draft,
                Some("provider/provider-verified/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save");
        database
            .mark_model_verification(
                "provider-verified",
                "model-a",
                VerificationStatus::Verified,
                Some("fingerprint"),
            )
            .expect("mark verified");

        let provider = database
            .get_provider("provider-verified")
            .expect("provider");
        assert_eq!(
            provider.summary.verification_status,
            VerificationStatus::Verified
        );
        assert_eq!(
            provider.summary.verified_model_id.as_deref(),
            Some("model-a")
        );
    }

    #[test]
    fn provider_verification_summary_ignores_non_text_models() {
        let database = Database::in_memory().expect("database");
        let draft = ProviderDraft {
            id: Some("provider-mixed-output".to_owned()),
            name: "Mixed".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("text-model".to_owned()),
            models: vec![
                ModelDraft {
                    model_id: "text-model".to_owned(),
                    display_name: "Text Model".to_owned(),
                    output_modality: ModelOutputModality::Text,
                    supports_streaming: true,
                    supports_tools: true,
                },
                ModelDraft {
                    model_id: "image-model".to_owned(),
                    display_name: "Image Model".to_owned(),
                    output_modality: ModelOutputModality::Image,
                    supports_streaming: false,
                    supports_tools: false,
                },
            ],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-mixed-output",
                &draft,
                Some("provider/provider-mixed-output/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save");
        database
            .mark_model_verification(
                "provider-mixed-output",
                "text-model",
                VerificationStatus::Verified,
                Some("fingerprint"),
            )
            .expect("verify text model");

        let provider = database
            .get_provider("provider-mixed-output")
            .expect("provider");
        assert_eq!(
            provider.summary.verification_status,
            VerificationStatus::Verified
        );
        assert_eq!(
            provider.summary.verified_model_id.as_deref(),
            Some("text-model")
        );
        assert_eq!(
            provider.summary.models[1].verification_status,
            VerificationStatus::DraftUnverified
        );
    }

    #[test]
    fn adding_a_model_preserves_unchanged_model_verification() {
        let database = Database::in_memory().expect("database");
        let mut draft = ProviderDraft {
            id: Some("provider-model-status".to_owned()),
            name: "Test".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-model-status",
                &draft,
                Some("provider/provider-model-status/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("initial save");
        database
            .mark_model_verification(
                "provider-model-status",
                "model-a",
                VerificationStatus::Verified,
                Some("fingerprint-a"),
            )
            .expect("verify model a");

        draft.models.push(ModelDraft {
            model_id: "model-b".to_owned(),
            display_name: "Model B".to_owned(),
            output_modality: ModelOutputModality::Text,
            supports_streaming: true,
            supports_tools: true,
        });
        database
            .save_provider(
                "provider-model-status",
                &draft,
                Some("provider/provider-model-status/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save with new model");

        let provider = database
            .get_provider("provider-model-status")
            .expect("provider");
        assert_eq!(
            provider.summary.models[0].verification_status,
            VerificationStatus::Verified
        );
        assert_eq!(
            provider.summary.models[1].verification_status,
            VerificationStatus::DraftUnverified
        );
        assert_eq!(
            provider.summary.verification_status,
            VerificationStatus::DraftUnverified
        );
    }

    #[test]
    fn changing_output_modality_marks_a_verified_model_stale() {
        let database = Database::in_memory().expect("database");
        let mut draft = ProviderDraft {
            id: Some("provider-output-modality".to_owned()),
            name: "Test".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-output-modality",
                &draft,
                Some("provider/provider-output-modality/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("initial save");
        database
            .mark_model_verification(
                "provider-output-modality",
                "model-a",
                VerificationStatus::Verified,
                Some("fingerprint-a"),
            )
            .expect("verify model");

        draft.models[0].output_modality = ModelOutputModality::Image;
        draft.models[0].supports_streaming = false;
        draft.models[0].supports_tools = false;
        database
            .save_provider(
                "provider-output-modality",
                &draft,
                Some("provider/provider-output-modality/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save changed modality");

        let provider = database
            .get_provider("provider-output-modality")
            .expect("provider");
        assert_eq!(
            provider.summary.models[0].output_modality,
            ModelOutputModality::Image
        );
        assert_eq!(
            provider.summary.models[0].verification_status,
            VerificationStatus::Stale
        );
    }

    #[test]
    fn migrates_legacy_provider_verification_to_existing_models() {
        let database = Database::in_memory().expect("database");
        let draft = ProviderDraft {
            id: Some("provider-legacy-verification".to_owned()),
            name: "Legacy".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://example.test/v1".to_owned(),
            api_key: None,
            default_model_id: Some("model-a".to_owned()),
            models: vec![
                ModelDraft {
                    model_id: "model-a".to_owned(),
                    display_name: "Model A".to_owned(),
                    output_modality: ModelOutputModality::Text,
                    supports_streaming: true,
                    supports_tools: true,
                },
                ModelDraft {
                    model_id: "model-b".to_owned(),
                    display_name: "Model B".to_owned(),
                    output_modality: ModelOutputModality::Text,
                    supports_streaming: true,
                    supports_tools: false,
                },
            ],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "provider-legacy-verification",
                &draft,
                Some("provider/provider-legacy-verification/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save");
        {
            let connection = database.connection().expect("connection");
            connection
                .execute(
                    r#"
                    UPDATE providers
                    SET verification_status = 'verified',
                        verification_fingerprint = 'legacy-fingerprint',
                        verified_model_id = 'model-a'
                    WHERE id = 'provider-legacy-verification'
                    "#,
                    [],
                )
                .expect("legacy provider status");
            connection
                .execute(
                    r#"
                    UPDATE models
                    SET verification_status = 'draft_unverified',
                        verification_fingerprint = NULL
                    WHERE provider_id = 'provider-legacy-verification'
                    "#,
                    [],
                )
                .expect("legacy models");
            connection
                .execute(
                    "DELETE FROM schema_migrations WHERE version = ?1",
                    [MODEL_VERIFICATION_MIGRATION],
                )
                .expect("reset model migration");
        }

        database.initialize().expect("rerun migration");
        let provider = database
            .get_provider("provider-legacy-verification")
            .expect("provider");
        assert!(provider
            .summary
            .models
            .iter()
            .all(|model| model.verification_status == VerificationStatus::Verified));
    }

    #[test]
    fn runtime_selection_preserves_the_first_original_value() {
        let database = Database::in_memory().expect("database");
        assert!(database
            .remember_runtime_selection("workbuddy", "session-1", Some("auto"))
            .expect("remember"));
        assert!(!database
            .remember_runtime_selection("workbuddy", "session-1", Some("custom-local:at-switch"),)
            .expect("remember again"));

        assert_eq!(
            database
                .list_runtime_selections("workbuddy")
                .expect("selections"),
            vec![StoredRuntimeSelection {
                scope_id: "session-1".to_owned(),
                original_value: Some("auto".to_owned()),
            }]
        );
    }

    #[test]
    fn purge_drops_placeholder_providers_without_models_or_api_key() {
        // 模拟早期版本遗留的旧库：三条 provider
        // - placeholder-empty：无 model、无 api key  -> 应被删除
        // - with-model：有 model、无 api key        -> 保留
        // - with-api-key：无 model、有 api key      -> 保留
        let connection = Connection::open_in_memory().expect("legacy database");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                CREATE TABLE providers (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    protocol TEXT NOT NULL,
                    base_url TEXT NOT NULL,
                    is_recommended INTEGER NOT NULL DEFAULT 0,
                    is_enabled INTEGER NOT NULL DEFAULT 1,
                    api_key_ref TEXT,
                    api_key_revision INTEGER NOT NULL DEFAULT 0,
                    masked_api_key TEXT,
                    verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                    verification_fingerprint TEXT,
                    verified_model_id TEXT,
                    default_model_id TEXT,
                    allow_insecure_http INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE models (
                    id TEXT PRIMARY KEY,
                    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                    model_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    output_modality TEXT NOT NULL DEFAULT 'text',
                    supports_streaming INTEGER NOT NULL DEFAULT 1,
                    supports_tools INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'custom',
                    verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                    verification_fingerprint TEXT,
                    UNIQUE(provider_id, model_id)
                );
                CREATE TABLE agent_bindings (
                    agent_id TEXT PRIMARY KEY,
                    mode TEXT NOT NULL,
                    provider_id TEXT NOT NULL REFERENCES providers(id),
                    default_model_id TEXT NOT NULL,
                    request_protocol TEXT NOT NULL,
                    local_token_ref TEXT,
                    local_token_revision INTEGER NOT NULL DEFAULT 0,
                    verification_status TEXT NOT NULL DEFAULT 'draft_unverified',
                    updated_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations(version, applied_at)
                VALUES (2026080101, '2026-08-01T00:00:00Z'),
                       (2026080201, '2026-08-02T00:00:00Z');

                INSERT INTO providers(
                    id, name, kind, protocol, base_url, is_recommended, is_enabled,
                    api_key_ref, api_key_revision, masked_api_key, verification_status,
                    verified_model_id, default_model_id, allow_insecure_http,
                    created_at, updated_at
                ) VALUES
                    ('placeholder-empty', '豪云智算', 'custom', 'openai_chat_completions',
                     'https://api.example.test/v1', 0, 1,
                     NULL, 0, NULL, 'draft_unverified',
                     NULL, NULL, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('with-model', '有模型', 'custom', 'openai_chat_completions',
                     'https://api.example.test/v1', 0, 1,
                     NULL, 0, NULL, 'draft_unverified',
                     NULL, 'model-a', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('with-api-key', '有密钥', 'custom', 'openai_chat_completions',
                     'https://api.example.test/v1', 0, 1,
                     'provider/with-api-key/api-key/v1', 1, '••••test',
                     'draft_unverified', NULL, NULL, 0,
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');

                INSERT INTO models(
                    id, provider_id, model_id, display_name, output_modality,
                    supports_streaming, supports_tools, source, verification_status
                ) VALUES
                    ('with-model:model-a', 'with-model', 'model-a', 'Model A', 'text',
                     1, 1, 'custom', 'draft_unverified');
                "#,
            )
            .expect("seed legacy database");

        let database = Database {
            connection: Mutex::new(connection),
        };
        database.initialize().expect("run migration");

        let providers = database.list_providers().expect("providers");
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert!(
            !ids.contains(&"placeholder-empty"),
            "占位 provider 应被删除"
        );
        assert!(ids.contains(&"with-model"), "有模型的 provider 应保留");
        assert!(
            ids.contains(&"with-api-key"),
            "有 API Key 的 provider 应保留"
        );

        let connection = database.connection().expect("connection");
        let purge_recorded: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [PLACEHOLDER_PROVIDER_PURGE_MIGRATION],
                |row| row.get(0),
            )
            .expect("migration marker");
        assert!(purge_recorded, "迁移应被记录为已执行");
    }

    #[test]
    fn purge_is_idempotent_on_fresh_database() {
        // 全新数据库反复 initialize 不应报错，也不应误删用户后续添加的 provider。
        let database = Database::in_memory().expect("database");
        database.initialize().expect("first initialize");
        database.initialize().expect("second initialize");

        let draft = ProviderDraft {
            id: None,
            name: "用户添加".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://api.example.test/v1".to_owned(),
            api_key: Some("sk-test".to_owned()),
            default_model_id: Some("model-a".to_owned()),
            models: vec![ModelDraft {
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        };
        database
            .save_provider(
                "user-provider",
                &draft,
                Some("provider/user-provider/api-key/v1"),
                1,
                Some("••••test"),
            )
            .expect("save");
        database.initialize().expect("reinitialize after user data");
        let providers = database.list_providers().expect("providers");
        assert!(
            providers.iter().any(|p| p.id == "user-provider"),
            "用户添加的 provider 不应被清理"
        );
    }
}
