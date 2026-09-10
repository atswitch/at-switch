use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::{
    domain::{AppResult, CommandError},
    services::{endpoint_url, ConfigTransaction, FileChange},
};

use super::super::{
    ima_local::{self, LocalSelectionSnapshot, Selection},
    lifecycle::{self, RestartOutcome},
    AgentDetection, DesiredAgentBinding,
};
use super::{
    active_account, ImaClient, ImaHomePage, ImaModel, ImaModelInput, ImaSceneModel, ImaSceneModels,
    ImaSecret, ImaSessionProvider, ImaSnapshot,
};

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) struct ImaBindingOutcome {
    pub needs_restart: bool,
    pub message: String,
}

#[derive(Default)]
pub(crate) struct ImaBindingManager {
    sessions: ImaSessionProvider,
    operation: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelectionState {
    preferred: [String; 2],
    local: LocalSelectionSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedModel {
    customize_id: String,
    selections: [Selection; 2],
    input: ImaModelInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOperation {
    before: SelectionState,
    previous: Option<ManagedModel>,
    before_ids: Vec<String>,
    add_input: Option<ImaModelInput>,
    created: Option<ManagedModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Checkpoint {
    version: u8,
    account_key: String,
    baseline: SelectionState,
    managed: Option<ManagedModel>,
    pending: Option<PendingOperation>,
}

impl ImaBindingManager {
    pub(super) async fn connect(
        &self,
        detection: &AgentDetection,
        web_version: &str,
    ) -> AppResult<ImaClient> {
        let session = self
            .sessions
            .session(preferences_path(detection)?, web_version)
            .await?;
        ImaClient::new(session)
    }

    pub(crate) fn checkpoint_status(
        &self,
        detection: &AgentDetection,
        transaction: &ConfigTransaction,
    ) -> AppResult<Option<(bool, bool)>> {
        let account = active_account(preferences_path(detection)?)?;
        let Some(bytes) = transaction.read_service_checkpoint("ima", &resource_key(&account))?
        else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(bytes);
        let record: Checkpoint =
            serde_json::from_slice(&bytes).map_err(|_| checkpoint_invalid())?;
        if record.version != 1 || record.account_key != account {
            return Err(checkpoint_invalid());
        }
        Ok(Some((record.managed.is_some(), record.pending.is_some())))
    }

    /// Passive verification reads only AT-Switch's encrypted checkpoint and
    /// ima's local preferences. It never opens ima's Keychain item or network.
    pub(crate) fn verify_cached(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
    ) -> AppResult<()> {
        let path = preferences_path(detection)?;
        let account = active_account(path)?;
        let bytes = transaction
            .read_service_checkpoint("ima", &resource_key(&account))?
            .ok_or_else(|| {
                CommandError::new("ima_binding_unverified", "ima 当前账号尚未完成模型连接")
                    .with_recovery("请选择模型并点击切换。")
            })?;
        let bytes = Zeroizing::new(bytes);
        let record: Checkpoint =
            serde_json::from_slice(&bytes).map_err(|_| checkpoint_invalid())?;
        if record.version != 1 || record.account_key != account || record.pending.is_some() {
            return Err(
                CommandError::new("ima_recovery_pending", "ima 模型切换尚未完成")
                    .with_recovery("请点击切换或恢复原配置，AT-Switch 会先完成恢复。"),
            );
        }
        let managed = record
            .managed
            .ok_or_else(|| CommandError::new("ima_binding_unverified", "ima 当前使用原始配置"))?;
        if managed.input.model_name != desired.model_id
            || managed.input.api_key.expose() != desired.credential
            || managed.input.api_uri != desired_endpoint(desired.base_url)?
        {
            return Err(CommandError::new(
                "ima_binding_changed",
                "ima 已管理的模型与当前绑定不一致",
            )
            .with_recovery("请重新点击目标模型的切换。"));
        }
        ima_local::verify_selection(path, &managed.selections)
    }

    pub(crate) async fn apply(
        &self,
        detection: &AgentDetection,
        web_version: &str,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
        commit: &(dyn Fn() -> AppResult<()> + Send + Sync),
    ) -> AppResult<ImaBindingOutcome> {
        let _operation = self.operation.lock().await;
        let path = preferences_path(detection)?;
        let client = self.connect(detection, web_version).await?;
        // Authenticate before stopping ima, so a native permission dialog does
        // not leave the user waiting with their application closed.
        let pause = lifecycle::pause_for_config_update(detection)?;
        self.apply_paused(&client, path, desired, transaction, commit)
            .await?;
        Ok(resume_outcome(pause.resume(), false))
    }

    pub(crate) async fn restore(
        &self,
        detection: &AgentDetection,
        web_version: &str,
        transaction: &ConfigTransaction,
        commit: &(dyn Fn() -> AppResult<()> + Send + Sync),
    ) -> AppResult<ImaBindingOutcome> {
        let _operation = self.operation.lock().await;
        let path = preferences_path(detection)?;
        let key = active_account(path)?;
        // Restoring an untouched account is a local no-op and must not prompt
        // for access to another application's credentials.
        if !transaction.service_checkpoint_exists("ima", &resource_key(&key)) {
            commit()?;
            return Ok(ImaBindingOutcome {
                needs_restart: false,
                message: "ima 当前账号尚未被接管，已保留原始配置。".to_owned(),
            });
        }
        let client = self.connect(detection, web_version).await?;
        let pause = lifecycle::pause_for_config_update(detection)?;
        self.restore_paused(&client, path, transaction, commit)
            .await?;
        Ok(resume_outcome(pause.resume(), true))
    }

    async fn apply_paused(
        &self,
        client: &ImaClient,
        path: &Path,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
        commit: &(dyn Fn() -> AppResult<()> + Send + Sync),
    ) -> AppResult<()> {
        let snapshot = client.snapshot().await?;
        let mut record = load_or_create(transaction, client.account_key(), &snapshot, path)?;
        recover_pending(client, path, transaction, &mut record).await?;
        let before = client.snapshot().await?;
        let input = desired_input(desired, &before.homepage)?;
        let previous = existing_managed(&record, &before)?;
        record.pending = Some(PendingOperation {
            before: selection_state(&before, path)?,
            previous: previous.clone(),
            before_ids: before
                .homepage
                .models
                .iter()
                .map(|model| model.customize_id.clone())
                .collect(),
            add_input: previous.is_none().then(|| input.clone()),
            created: None,
        });
        save(transaction, &record)?;

        let result = async {
            let managed = match previous {
                Some(mut managed) => {
                    if managed.input != input {
                        client.modify_model(&managed.customize_id, &input).await?;
                    }
                    managed.input = input.clone();
                    managed
                }
                None => create_owned(client, transaction, &mut record, &input).await?,
            };
            set_preferences(
                client,
                &managed
                    .selections
                    .each_ref()
                    .map(|selection| selection.model_id.clone()),
            )
            .await?;
            transaction.apply_file(
                "ima",
                FileChange {
                    path: path.to_path_buf(),
                    new_content: ima_local::build_selection(path, &managed.selections)?,
                },
            )?;
            ima_local::verify_selection(path, &managed.selections)?;
            let current = client.snapshot().await?;
            verify_managed(&current, &managed)?;
            verify_preferences(
                &current.scenes,
                &managed
                    .selections
                    .each_ref()
                    .map(|selection| selection.model_id.clone()),
            )?;
            record.managed = Some(managed);
            commit_checkpoint(transaction, &mut record, commit)
        }
        .await;
        finish_or_rollback(result, client, path, transaction, &mut record).await
    }

    async fn restore_paused(
        &self,
        client: &ImaClient,
        path: &Path,
        transaction: &ConfigTransaction,
        commit: &(dyn Fn() -> AppResult<()> + Send + Sync),
    ) -> AppResult<()> {
        let snapshot = client.snapshot().await?;
        let mut record = load_or_create(transaction, client.account_key(), &snapshot, path)?;
        recover_pending(client, path, transaction, &mut record).await?;
        if record.managed.is_none() {
            commit()?;
            return Ok(());
        }
        let before = client.snapshot().await?;
        validate_preference_targets(&before.scenes, &record.baseline.preferred)?;
        let previous = existing_managed(&record, &before)?;
        validate_empty_restore(
            &before.scenes,
            &record.baseline.preferred,
            previous.as_ref(),
        )?;
        record.pending = Some(PendingOperation {
            before: selection_state(&before, path)?,
            previous: previous.clone(),
            before_ids: before
                .homepage
                .models
                .iter()
                .map(|model| model.customize_id.clone())
                .collect(),
            add_input: previous.as_ref().map(|managed| managed.input.clone()),
            created: None,
        });
        save(transaction, &record)?;
        let result = async {
            set_preferences(client, &record.baseline.preferred).await?;
            if let Some(managed) = &previous {
                client.delete_model(&managed.customize_id).await?;
            }
            // ima rejects an empty set_preferred_model. Deleting our selected
            // custom entry must reset it; verify that behavior rather than
            // silently substituting a different official model.
            let restored = client.snapshot().await?;
            verify_preferences(&restored.scenes, &record.baseline.preferred)?;
            restore_local(transaction, path, &record.baseline.local)?;
            record.managed = None;
            commit_checkpoint(transaction, &mut record, commit)
        }
        .await;
        finish_or_rollback(result, client, path, transaction, &mut record).await
    }
}

fn preferences_path(detection: &AgentDetection) -> AppResult<&Path> {
    detection.config_path.as_deref().ok_or_else(|| {
        CommandError::new("ima_config_missing", "无法定位 ima 当前账号配置")
            .with_recovery("请先打开 ima 并登录后重试。")
    })
}

fn resource_key(account: &str) -> String {
    account.to_owned()
}

fn commit_checkpoint(
    transaction: &ConfigTransaction,
    record: &mut Checkpoint,
    commit: &(dyn Fn() -> AppResult<()> + Send + Sync),
) -> AppResult<()> {
    // Keep the recovery journal through the metadata commit. A terminated
    // process must never leave a change with neither metadata nor a journal.
    save(transaction, record)?;
    commit()?;
    let pending = record.pending.take();
    if let Err(error) = save(transaction, record) {
        record.pending = pending;
        return Err(error);
    }
    Ok(())
}

fn load_or_create(
    transaction: &ConfigTransaction,
    account: &str,
    snapshot: &ImaSnapshot,
    path: &Path,
) -> AppResult<Checkpoint> {
    if let Some(bytes) = transaction.read_service_checkpoint("ima", &resource_key(account))? {
        let bytes = Zeroizing::new(bytes);
        let record: Checkpoint =
            serde_json::from_slice(&bytes).map_err(|_| checkpoint_invalid())?;
        if record.version != 1 || record.account_key != account {
            return Err(checkpoint_invalid());
        }
        return Ok(record);
    }
    let record = Checkpoint {
        version: 1,
        account_key: account.to_owned(),
        baseline: selection_state(snapshot, path)?,
        managed: None,
        pending: None,
    };
    save(transaction, &record)?;
    Ok(record)
}

fn save(transaction: &ConfigTransaction, record: &Checkpoint) -> AppResult<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(record).map_err(|_| checkpoint_invalid())?);
    transaction.save_service_checkpoint("ima", &resource_key(&record.account_key), &bytes)
}

fn selection_state(snapshot: &ImaSnapshot, path: &Path) -> AppResult<SelectionState> {
    Ok(SelectionState {
        preferred: snapshot.scenes.each_ref().map(effective_preferred_id),
        local: ima_local::snapshot(path)?,
    })
}

fn effective_preferred_id(scene: &ImaSceneModels) -> String {
    scene
        .preferred_model_id
        .as_deref()
        .filter(|id| scene.find(id).is_some())
        .unwrap_or_default()
        .to_owned()
}

fn desired_input(
    desired: &DesiredAgentBinding<'_>,
    homepage: &ImaHomePage,
) -> AppResult<ImaModelInput> {
    let limits = &homepage.customize_model_config;
    if limits.default_input_tokens == 0 || limits.default_output_tokens == 0 {
        return Err(CommandError::new(
            "ima_model_limits_unknown",
            "无法读取 ima 的自定义模型容量设置",
        )
        .with_recovery("请在 ima 中打开模型设置后重试；若仍失败，请更新 AT-Switch。"));
    }
    let endpoint = desired_endpoint(desired.base_url)?;
    Ok(ImaModelInput {
        api_uri: endpoint,
        api_key: ImaSecret::new(desired.credential),
        model_name: desired.model_id.to_owned(),
        max_input_tokens: limits.default_input_tokens,
        max_output_tokens: limits.default_output_tokens,
    })
}

fn desired_endpoint(base_url: &str) -> AppResult<String> {
    let url = url::Url::parse(base_url)
        .map_err(|_| CommandError::new("base_url_invalid", "Base URL 格式无效"))?;
    let endpoint = if url
        .path()
        .trim_end_matches('/')
        .ends_with("/chat/completions")
    {
        base_url.trim_end_matches('/').to_owned()
    } else {
        endpoint_url(base_url, "chat/completions")?.to_string()
    };
    Ok(endpoint)
}

fn existing_managed(
    record: &Checkpoint,
    snapshot: &ImaSnapshot,
) -> AppResult<Option<ManagedModel>> {
    let Some(managed) = &record.managed else {
        return Ok(None);
    };
    let Some(model) = snapshot
        .homepage
        .models
        .iter()
        .find(|model| model.customize_id == managed.customize_id)
    else {
        return Ok(None);
    };
    let mut current = managed.clone();
    // Snapshot even user edits to our own row so a failed switch restores what
    // existed immediately before this operation, not a stale remembered value.
    current.input = model.input();
    current.selections = selections_for(model, &snapshot.scenes)?;
    Ok(Some(current))
}

fn selections_for(model: &ImaModel, scenes: &[ImaSceneModels; 2]) -> AppResult<[Selection; 2]> {
    fn linked<'a>(
        models: &'a [ImaSceneModel],
        customize_id: &str,
        found: &mut Vec<&'a ImaSceneModel>,
    ) {
        for model in models {
            if !model.model_id.is_empty() && model.customize_id.as_deref() == Some(customize_id) {
                found.push(model);
            }
            linked(&model.sub_model_infos, customize_id, found);
        }
    }
    let select = |scene: &ImaSceneModels| -> AppResult<Selection> {
        let direct = model
            .model_id
            .as_deref()
            .and_then(|id| scene.find(id))
            .or_else(|| scene.find(&model.customize_id))
            .filter(|candidate| {
                candidate
                    .customize_id
                    .as_deref()
                    .is_none_or(|id| id == model.customize_id)
            });
        let found = if let Some(direct) = direct {
            Some(direct)
        } else {
            let mut candidates = Vec::new();
            linked(&scene.models, &model.customize_id, &mut candidates);
            let unique: HashSet<(&str, i64)> = candidates
                .iter()
                .map(|candidate| (candidate.model_id.as_str(), candidate.model_type))
                .collect();
            if unique.len() > 1 {
                return Err(CommandError::new(
                    "ima_model_mapping_ambiguous",
                    "ima 返回了多个对应模型选项，无法安全确定首选模型",
                )
                .with_recovery("请稍后重试；已有模型及原选择会保留。"));
            }
            candidates.first().copied()
        };
        let found = found
            .filter(|found| !found.model_id.is_empty() && found.model_type >= 0)
            .ok_or_else(|| {
                CommandError::new(
                    "ima_model_not_selectable",
                    "ima 尚未提供所添加模型的可选标识",
                )
                .with_recovery("请稍后重试；未完成的切换会先恢复原配置。")
            })?;
        Ok(Selection {
            model_id: found.model_id.clone(),
            model_type: found.model_type,
        })
    };
    Ok([select(&scenes[0])?, select(&scenes[1])?])
}

async fn create_owned(
    client: &ImaClient,
    transaction: &ConfigTransaction,
    record: &mut Checkpoint,
    input: &ImaModelInput,
) -> AppResult<ManagedModel> {
    let pending = record.pending.as_ref().ok_or_else(checkpoint_invalid)?;
    let before: HashSet<String> = pending.before_ids.iter().cloned().collect();
    let existing = client.snapshot().await?;
    let candidates: Vec<&ImaModel> = existing
        .homepage
        .models
        .iter()
        .filter(|model| !before.contains(&model.customize_id) && model.matches(input))
        .collect();
    if candidates.len() > 1 {
        return Err(CommandError::new(
            "ima_created_model_ambiguous",
            "存在多个可能由中断操作创建的模型，已停止添加",
        ));
    }
    if let Some(model) = candidates.first() {
        let managed = ManagedModel {
            customize_id: model.customize_id.clone(),
            selections: selections_for(model, &existing.scenes)?,
            input: input.clone(),
        };
        record
            .pending
            .as_mut()
            .ok_or_else(checkpoint_invalid)?
            .created = Some(managed.clone());
        save(transaction, record)?;
        return Ok(managed);
    }
    let result = client.add_model(input).await;
    let snapshot = client.snapshot().await?;
    let response_id = result.as_ref().ok().and_then(|result| {
        result.customize_id.as_deref().or_else(|| {
            result
                .model_info
                .as_ref()
                .map(|model| model.customize_id.as_str())
        })
    });
    let candidates: Vec<&ImaModel> = snapshot
        .homepage
        .models
        .iter()
        .filter(|model| {
            !before.contains(&model.customize_id)
                && model.matches(input)
                && response_id.is_none_or(|id| model.customize_id == id)
        })
        .collect();
    if candidates.len() != 1 {
        return Err(result.err().unwrap_or_else(|| {
            CommandError::new(
                "ima_created_model_ambiguous",
                "无法唯一确认本次添加的 ima 模型",
            )
            .with_recovery("请稍后重试；恢复记录已保留，已有模型不会被删除。")
        }));
    }
    let model = candidates[0];
    let managed = ManagedModel {
        customize_id: model.customize_id.clone(),
        selections: selections_for(model, &snapshot.scenes)?,
        input: input.clone(),
    };
    record
        .pending
        .as_mut()
        .ok_or_else(checkpoint_invalid)?
        .created = Some(managed.clone());
    save(transaction, record)?;
    Ok(managed)
}

async fn set_preferences(client: &ImaClient, preferred: &[String; 2]) -> AppResult<()> {
    for (scene, model_id) in preferred.iter().enumerate() {
        if !model_id.is_empty() {
            client.set_preferred_model(scene as u8, model_id).await?;
        }
    }
    Ok(())
}

fn verify_preferences(scenes: &[ImaSceneModels; 2], preferred: &[String; 2]) -> AppResult<()> {
    if scenes.iter().zip(preferred).any(|(scene, id)| {
        let actual = scene.preferred_model_id.as_deref().unwrap_or_default();
        actual != id && !(id.is_empty() && !actual.is_empty() && scene.find(actual).is_none())
    }) {
        return Err(CommandError::new(
            "ima_preference_verify_failed",
            "ima 首选模型回读校验未通过",
        )
        .with_recovery("请稍后重试恢复原配置；AT-Switch 已保留恢复记录。"));
    }
    Ok(())
}

fn verify_managed(snapshot: &ImaSnapshot, managed: &ManagedModel) -> AppResult<()> {
    if !snapshot
        .homepage
        .models
        .iter()
        .any(|model| model.customize_id == managed.customize_id && model.matches(&managed.input))
    {
        return Err(CommandError::new(
            "ima_model_verify_failed",
            "ima 自定义模型配置回读校验未通过",
        )
        .with_recovery("请确认网络和模型设置后重试。"));
    }
    Ok(())
}

fn validate_preference_targets(
    scenes: &[ImaSceneModels; 2],
    preferred: &[String; 2],
) -> AppResult<()> {
    for (scene, id) in scenes.iter().zip(preferred) {
        if !id.is_empty() && scene.find(id).is_none() {
            return Err(CommandError::new(
                "ima_original_model_missing",
                "接管前使用的 ima 模型已不存在",
            )
            .with_recovery("请先在 ima 恢复该模型，再返回 AT-Switch 恢复原配置。"));
        }
    }
    Ok(())
}

fn validate_empty_restore(
    scenes: &[ImaSceneModels; 2],
    preferred: &[String; 2],
    managed: Option<&ManagedModel>,
) -> AppResult<()> {
    for (index, (scene, id)) in scenes.iter().zip(preferred).enumerate() {
        let current = scene.preferred_model_id.as_deref().unwrap_or_default();
        if id.is_empty()
            && !current.is_empty()
            && managed.is_none_or(|managed| managed.selections[index].model_id != current)
        {
            return Err(CommandError::new(
                "ima_restore_selection_conflict",
                "ima 的首选模型已在其他位置修改",
            )
            .with_recovery("请先在 AT-Switch 切换到已管理的模型，再恢复原配置。"));
        }
    }
    Ok(())
}

fn restore_local(
    transaction: &ConfigTransaction,
    path: &Path,
    snapshot: &LocalSelectionSnapshot,
) -> AppResult<()> {
    transaction.apply_file(
        "ima",
        FileChange {
            path: path.to_path_buf(),
            new_content: ima_local::build_restore(path, snapshot)?,
        },
    )?;
    ima_local::verify_snapshot(path, snapshot)
}

async fn finish_or_rollback(
    result: AppResult<()>,
    client: &ImaClient,
    path: &Path,
    transaction: &ConfigTransaction,
    record: &mut Checkpoint,
) -> AppResult<()> {
    let Err(error) = result else {
        return Ok(());
    };
    #[cfg(test)]
    eprintln!(
        "IMA_BINDING_FAILURE stage=before-rollback code={}",
        error.code
    );
    // Re-persist the journal if committing local binding metadata failed after
    // the successful external configuration had already been checkpointed.
    let journal_saved = save(transaction, record);
    if journal_saved.is_err()
        || recover_pending(client, path, transaction, record)
            .await
            .is_err()
    {
        return Err(
            CommandError::new("ima_recovery_pending", "ima 切换未完成，自动恢复仍需重试")
                .with_recovery(
                    "请保持当前 ima 账号登录与网络连接，再点击恢复原配置；加密恢复记录已保留。",
                ),
        );
    }
    Err(error)
}

async fn recover_pending(
    client: &ImaClient,
    path: &Path,
    transaction: &ConfigTransaction,
    record: &mut Checkpoint,
) -> AppResult<()> {
    let Some(pending) = record.pending.clone() else {
        return Ok(());
    };
    let current = client.snapshot().await?;
    let mut before = pending.before.clone();
    let mut previous = pending.previous.clone();
    if let Some(original) = previous.as_mut() {
        let original_row = current
            .homepage
            .models
            .iter()
            .find(|model| model.customize_id == original.customize_id);
        if let Some(model) = original_row {
            if !model.matches(&original.input) {
                client
                    .modify_model(&original.customize_id, &original.input)
                    .await?;
            }
        } else if pending.add_input.is_some() {
            // A restore operation can be interrupted after ima has deleted our
            // row. Recreating it just to roll the restore back is unnecessary;
            // continue restoring the native state instead.
            previous = None;
        } else {
            // A delete may have succeeded before the connection failed during a
            // switch. Recreate only our own row and remap its operation snapshot.
            let replacement = if let Some(created) = &pending.created {
                if current.homepage.models.iter().any(|model| {
                    model.customize_id == created.customize_id && model.matches(&original.input)
                }) {
                    created.clone()
                } else {
                    create_owned(client, transaction, record, &original.input).await?
                }
            } else {
                create_owned(client, transaction, record, &original.input).await?
            };
            let replacements: Vec<(String, Selection)> = original
                .selections
                .iter()
                .zip(&replacement.selections)
                .map(|(old, new)| (old.model_id.clone(), new.clone()))
                .collect();
            for id in &mut before.preferred {
                if let Some((_, new)) = replacements.iter().find(|(old, _)| old == id) {
                    *id = new.model_id.clone();
                }
            }
            before.local = ima_local::remap_models(&before.local, &replacements);
            *original = replacement;
        }
    }
    set_preferences(client, &before.preferred).await?;
    if previous.is_none() {
        let candidate = if let Some(created) = &pending.created {
            Some(created.customize_id.clone())
        } else if let Some(input) = &pending.add_input {
            let matches: Vec<&ImaModel> = current
                .homepage
                .models
                .iter()
                .filter(|model| {
                    !pending.before_ids.contains(&model.customize_id) && model.matches(input)
                })
                .collect();
            match matches.as_slice() {
                [] => None,
                [model] => Some(model.customize_id.clone()),
                _ => {
                    return Err(CommandError::new(
                        "ima_created_model_ambiguous",
                        "存在多个可能由中断操作创建的模型，已停止自动删除",
                    ))
                }
            }
        } else {
            None
        };
        if let Some(id) = candidate {
            client.delete_model(&id).await?;
        }
    }
    let restored = client.snapshot().await?;
    verify_preferences(&restored.scenes, &before.preferred)?;
    if let Some(managed) = &previous {
        verify_managed(&restored, managed)?;
    }
    restore_local(transaction, path, &before.local)?;
    record.managed = previous;
    record.pending = None;
    save(transaction, record)
}

fn checkpoint_invalid() -> CommandError {
    CommandError::new(
        "ima_checkpoint_invalid",
        "ima 的加密恢复记录无效，已停止修改",
    )
    .with_recovery("请保留备份并更新 AT-Switch；不要删除恢复记录。")
}

fn resume_outcome(result: AppResult<RestartOutcome>, restored: bool) -> ImaBindingOutcome {
    let action = if restored {
        "已恢复 ima 接管前的模型设置"
    } else {
        "ima 两个入口的模型均已切换并校验"
    };
    match result {
        Ok(RestartOutcome::Relaunched) => ImaBindingOutcome {
            needs_restart: false,
            message: format!("{action}，ima 已重新打开。"),
        },
        Ok(RestartOutcome::WasNotRunning) => ImaBindingOutcome {
            needs_restart: false,
            message: format!("{action}，下次打开 ima 即可使用。"),
        },
        _ => ImaBindingOutcome {
            needs_restart: true,
            message: format!("{action}，请重新打开 ima 使用。"),
        },
    }
}
