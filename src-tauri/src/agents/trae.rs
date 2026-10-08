use std::{collections::HashSet, path::Path, sync::Arc};

use futures_util::future::BoxFuture;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    domain::{
        AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol, AppResult,
        CommandError,
    },
    services::{BaselineSnapshot, ConfigTransaction},
};

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    service_adapter::{CommitBinding, ServiceConfigAdapter, ServiceConfigOutcome},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

#[path = "trae_ui.rs"]
mod ui;

const CHECKPOINT_VERSION: u8 = 1;
const LEGACY_MANAGED_PREFIX: &str = "AT-Switch · ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraeKind {
    Code,
    Work,
}

impl TraeKind {
    fn id(self) -> &'static str {
        match self {
            Self::Code => "traecode",
            Self::Work => "traework",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Code => "TraeCode",
            Self::Work => "TraeWork",
        }
    }

    fn app_data_name(self) -> &'static str {
        match self {
            Self::Code => "Trae CN",
            Self::Work => "TRAE SOLO CN",
        }
    }
}

pub(super) struct TraeAdapter {
    kind: TraeKind,
    ui: Arc<dyn ui::TraeUi>,
}

impl TraeAdapter {
    pub(super) fn code() -> Self {
        Self::new(TraeKind::Code)
    }

    pub(super) fn work() -> Self {
        Self::new(TraeKind::Work)
    }

    fn new(kind: TraeKind) -> Self {
        Self {
            kind,
            ui: Arc::new(ui::SystemTraeUi),
        }
    }

    #[cfg(test)]
    fn with_ui(kind: TraeKind, ui: Arc<dyn ui::TraeUi>) -> Self {
        Self { kind, ui }
    }
}

impl AgentAdapter for TraeAdapter {
    fn id(&self) -> &'static str {
        self.kind.id()
    }

    fn display_name(&self) -> &'static str {
        self.kind.display_name()
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let (mac_names, bundle_ids, windows_paths): (&[&str], &[&str], &[&str]) = match self.kind {
            TraeKind::Code => (
                &["Trae CN.app"],
                &["cn.trae.app"],
                &[
                    "Programs/Trae CN/Trae CN.exe",
                    "Programs/TraeCode CN/Trae CN.exe",
                    "Trae CN/Trae CN.exe",
                ],
            ),
            TraeKind::Work => (
                &["TRAE SOLO CN.app"],
                &["cn.trae.solo.app"],
                &[
                    "Programs/TRAE SOLO CN/TRAE SOLO CN.exe",
                    "Programs/TraeWork CN/TRAE SOLO CN.exe",
                    "TRAE SOLO CN/TRAE SOLO CN.exe",
                ],
            ),
        };
        let installation = locate_desktop_app(context, mac_names, bundle_ids, windows_paths);
        let runtime_data_dir = context.application_data_dir.join(self.kind.app_data_name());
        let state_database = runtime_data_dir.join("User/globalStorage/state.vscdb");
        let mut detection = AgentDetection::manual(
            self.id(),
            self.display_name(),
            installation,
            "请先打开并登录应用，再使用 AT-Switch 切换模型。",
        );
        detection.config_path = Some(state_database.clone());
        detection.runtime_data_dir = Some(runtime_data_dir);
        if detection.installation.is_none() {
            return detection;
        }
        match active_account(&state_database, self.kind) {
            Ok(_) if cfg!(any(target_os = "macos", target_os = "windows")) => {
                detection.install_status = AgentInstallStatus::Installed;
                detection.config_health = AgentConfigHealth::Healthy;
                detection.write_supported = true;
                detection.needs_restart = false;
                detection.message = Some(format!(
                    "{} 已识别；AT-Switch 通过官方自定义模型界面完成直连切换，不修改私有数据库。",
                    self.display_name()
                ));
            }
            Ok(_) => {
                detection.message = Some(format!(
                    "{} 界面自动化目前仅支持 macOS 和 Windows。",
                    self.display_name()
                ));
            }
            Err(error) => {
                detection.install_status = AgentInstallStatus::InstalledUninitialized;
                detection.config_health = AgentConfigHealth::Unreadable;
                detection.message = Some(error.message);
            }
        }
        detection
    }

    fn source_protocol(&self, mode: AgentBindingMode, upstream: ApiProtocol) -> ApiProtocol {
        if mode == AgentBindingMode::Direct {
            upstream
        } else {
            ApiProtocol::OpenaiChatCompletions
        }
    }

    fn validate_binding(&self, desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        if desired.mode != AgentBindingMode::Direct {
            return Err(CommandError::new(
                "trae_proxy_unsupported",
                format!("{} 当前仅支持直连模式。", self.display_name()),
            )
            .with_recovery("请选择直连；请求会由 Trae 直接发送到所选 Provider。"));
        }
        if desired.source_protocol != desired.upstream_protocol {
            return Err(CommandError::new(
                "trae_direct_protocol_mismatch",
                format!("{} 直连要求使用 Provider 的原生协议。", self.display_name()),
            ));
        }
        Ok(())
    }

    fn build_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<Vec<u8>> {
        Err(ui_required(self.display_name()))
    }

    fn build_native_config(&self, _: &AgentDetection, _: &BaselineSnapshot) -> AppResult<Vec<u8>> {
        Err(ui_required(self.display_name()))
    }

    fn verify_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<()> {
        Err(ui_required(self.display_name()))
    }

    fn service_config(&self) -> Option<&dyn ServiceConfigAdapter> {
        Some(self)
    }
}

impl ServiceConfigAdapter for TraeAdapter {
    fn account_scope(&self, detection: &AgentDetection) -> AppResult<String> {
        let account = active_account(state_database(detection)?, self.kind)?;
        Ok(format!("{}:{account}", self.id()))
    }

    fn checkpoint_status(
        &self,
        detection: &AgentDetection,
        transaction: &ConfigTransaction,
    ) -> AppResult<Option<(bool, bool)>> {
        let resource = self.account_scope(detection)?;
        let Some(checkpoint) = load_checkpoint(transaction, self.id(), &resource)? else {
            return Ok(None);
        };
        Ok(Some((
            checkpoint.active.is_some(),
            checkpoint.pending || (checkpoint.active.is_none() && !checkpoint.owned.is_empty()),
        )))
    }

    fn apply<'a>(
        &'a self,
        detection: &'a AgentDetection,
        desired: &'a DesiredAgentBinding<'a>,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            self.validate_binding(desired)?;
            let resource = self.account_scope(detection)?;
            let ui = Arc::clone(&self.ui);
            let kind = self.kind;
            let detection = detection.clone();
            let input = ManagedInput::from_desired(desired);
            let mut checkpoint = match load_checkpoint(transaction, self.id(), &resource)? {
                Some(checkpoint) => checkpoint,
                None => {
                    let detection_for_snapshot = detection.clone();
                    let snapshot = tokio::task::spawn_blocking(move || {
                        ui.snapshot_interactive(kind, &detection_for_snapshot)
                    })
                    .await
                    .map_err(|_| CommandError::internal("Trae 界面读取任务异常终止"))??;
                    Checkpoint {
                        version: CHECKPOINT_VERSION,
                        account_scope: resource.clone(),
                        baseline_selection: snapshot.selection,
                        owned: Vec::new(),
                        borrowed: Vec::new(),
                        active: None,
                        pending: false,
                    }
                }
            };
            validate_checkpoint(&checkpoint, &resource)?;
            let historical_owned = historical_owned_rows(transaction, self.id(), &resource)?;
            let ui = Arc::clone(&self.ui);
            let detection_for_ui = detection.clone();
            let existing_names = checkpoint
                .owned
                .iter()
                .map(|row| row.display_name.clone())
                .collect::<HashSet<_>>();
            let matching = checkpoint
                .owned
                .iter()
                .find(|row| row.input.matches(&input) && !is_legacy_managed_row(row))
                .cloned()
                .or_else(|| {
                    historical_owned
                        .iter()
                        .find(|row| row.input.matches(&input) && !is_legacy_managed_row(row))
                        .cloned()
                });
            let replacement = checkpoint
                .owned
                .iter()
                .find(|row| row.input.matches(&input) && is_legacy_managed_row(row))
                .or_else(|| {
                    let expected_name = managed_display_name_base(&input);
                    checkpoint
                        .owned
                        .iter()
                        .find(|row| row.display_name == expected_name)
                })
                .cloned()
                .or_else(|| {
                    let expected_name = managed_display_name_base(&input);
                    historical_owned
                        .iter()
                        .find(|row| {
                            (row.input.matches(&input) && is_legacy_managed_row(row))
                                || row.display_name == expected_name
                        })
                        .cloned()
                });
            let plan = tokio::task::spawn_blocking(move || {
                plan_ui_change(
                    ui.as_ref(),
                    kind,
                    &detection_for_ui,
                    &input,
                    matching.as_ref(),
                    replacement.as_ref(),
                    &existing_names,
                )
            })
            .await
            .map_err(|_| CommandError::internal("Trae 界面规划任务异常终止"))??;

            let previous_active = checkpoint.active.clone();
            let previous_borrowed = checkpoint.borrowed.clone();
            if let Some(replaced) = plan.replaced_row.as_ref() {
                if !checkpoint
                    .owned
                    .iter()
                    .any(|row| row.display_name == replaced.display_name)
                {
                    checkpoint.owned.push(replaced.clone());
                }
            }
            if plan.borrowed {
                upsert_managed_row(&mut checkpoint.borrowed, &plan.row);
            } else {
                upsert_managed_row(&mut checkpoint.owned, &plan.row);
            }
            // Record the exact cleanup target before mutating the official UI.
            // If the process stops later, recovery can restore the previous
            // selection and remove only this AT-Switch-managed row.
            checkpoint.pending = true;
            save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;

            let ui = Arc::clone(&self.ui);
            let detection_for_ui = detection.clone();
            let plan_for_ui = plan.clone();
            let attempt = tokio::task::spawn_blocking(move || {
                apply_ui_change(ui.as_ref(), kind, &detection_for_ui, &plan_for_ui)
            })
            .await
            .map_err(|_| CommandError::internal("Trae 界面切换任务异常终止"))?;
            let ui_mutation = attempt.mutation;

            if let Err(error) = attempt.result {
                let rollback = rollback_ui_change(
                    Arc::clone(&self.ui),
                    kind,
                    detection.clone(),
                    plan.clone(),
                    ui_mutation,
                )
                .await;
                record_rollback_checkpoint(
                    &mut checkpoint,
                    &previous_active,
                    &previous_borrowed,
                    &plan,
                    &rollback,
                );
                let _ = save_checkpoint(transaction, self.id(), &resource, &checkpoint);
                return Err(error);
            }

            if plan.borrowed {
                upsert_managed_row(&mut checkpoint.borrowed, &plan.row);
            } else {
                upsert_managed_row(&mut checkpoint.owned, &plan.row);
            }
            checkpoint.active = Some(plan.row.display_name.clone());
            checkpoint.pending = false;
            if let Err(error) = save_checkpoint(transaction, self.id(), &resource, &checkpoint) {
                let rollback = rollback_ui_change(
                    Arc::clone(&self.ui),
                    kind,
                    detection.clone(),
                    plan.clone(),
                    ui_mutation,
                )
                .await;
                record_rollback_checkpoint(
                    &mut checkpoint,
                    &previous_active,
                    &previous_borrowed,
                    &plan,
                    &rollback,
                );
                let _ = save_checkpoint(transaction, self.id(), &resource, &checkpoint);
                return Err(error);
            }
            if let Err(error) = commit() {
                let rollback = rollback_ui_change(
                    Arc::clone(&self.ui),
                    kind,
                    detection.clone(),
                    plan.clone(),
                    ui_mutation,
                )
                .await;
                record_rollback_checkpoint(
                    &mut checkpoint,
                    &previous_active,
                    &previous_borrowed,
                    &plan,
                    &rollback,
                );
                let _ = save_checkpoint(transaction, self.id(), &resource, &checkpoint);
                return Err(error);
            }

            // Older builds exposed an implementation prefix in Trae's model
            // menu. Once the new binding and its recovery checkpoint are both
            // durable, remove only the exact legacy rows recorded as owned by
            // this adapter. A cleanup failure does not invalidate the active
            // model; the retained row remains available to a later restore.
            let legacy_cleanup_complete = cleanup_legacy_rows(
                Arc::clone(&self.ui),
                kind,
                detection.clone(),
                &mut checkpoint,
            )
            .await;
            if let Err(error) = save_checkpoint(transaction, self.id(), &resource, &checkpoint) {
                log::warn!(
                    "unable to persist Trae legacy model cleanup for {}: {}",
                    self.id(),
                    error.message
                );
            }
            Ok(ServiceConfigOutcome {
                needs_restart: false,
                message: if plan.borrowed {
                    format!(
                        "{} 已直接切换到 Trae 中已有的模型配置；请求不经过 AT-Switch。",
                        self.display_name()
                    )
                } else if legacy_cleanup_complete {
                    format!(
                        "{} 已通过官方自定义模型界面切换为直连模型；请求不经过 AT-Switch。",
                        self.display_name()
                    )
                } else {
                    format!(
                        "{} 已切换为直连模型；旧版模型项将在下次切换或恢复时继续清理。",
                        self.display_name()
                    )
                },
            })
        })
    }

    fn restore<'a>(
        &'a self,
        detection: &'a AgentDetection,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            let resource = self.account_scope(detection)?;
            let Some(mut checkpoint) = load_checkpoint(transaction, self.id(), &resource)? else {
                commit()?;
                return Ok(ServiceConfigOutcome {
                    needs_restart: false,
                    message: format!(
                        "{} 尚未被 AT-Switch 接管，已保留原模型。",
                        self.display_name()
                    ),
                });
            };
            validate_checkpoint(&checkpoint, &resource)?;
            checkpoint.pending = true;
            save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;
            let ui = Arc::clone(&self.ui);
            let kind = self.kind;
            let detection_for_ui = detection.clone();
            let baseline = checkpoint.baseline_selection.clone();
            tokio::task::spawn_blocking(move || {
                ui.select_model(kind, &detection_for_ui, &baseline)
            })
            .await
            .map_err(|_| CommandError::internal("Trae 原模型恢复任务异常终止"))??;

            if let Err(error) = commit() {
                let selection_restored = if let Some(active) = checkpoint.active.as_deref() {
                    let ui = Arc::clone(&self.ui);
                    let detection = detection.clone();
                    let active = active.to_owned();
                    tokio::task::spawn_blocking(move || ui.select_model(kind, &detection, &active))
                        .await
                        .is_ok_and(|result| result.is_ok())
                } else {
                    true
                };
                checkpoint.pending = !selection_restored;
                let _ = save_checkpoint(transaction, self.id(), &resource, &checkpoint);
                return Err(error);
            }

            checkpoint.active = None;
            checkpoint.borrowed.clear();
            save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;
            let mut retained = Vec::new();
            for row in checkpoint.owned.drain(..) {
                let ui = Arc::clone(&self.ui);
                let detection = detection.clone();
                let name = row.display_name.clone();
                let deleted =
                    tokio::task::spawn_blocking(move || ui.delete_model(kind, &detection, &name))
                        .await
                        .is_ok_and(|result| result.is_ok());
                if !deleted {
                    retained.push(row);
                }
            }
            checkpoint.owned = retained;
            checkpoint.pending = !checkpoint.owned.is_empty();
            save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;
            Ok(ServiceConfigOutcome {
                needs_restart: false,
                message: if checkpoint.pending {
                    format!(
                        "{} 已恢复接管前的模型；部分 AT-Switch 模型项将在下次恢复时继续清理。",
                        self.display_name()
                    )
                } else {
                    format!("{} 已恢复接管前的模型选择。", self.display_name())
                },
            })
        })
    }

    fn verify_cached(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
    ) -> AppResult<()> {
        let resource = self.account_scope(detection)?;
        let checkpoint = load_checkpoint(transaction, self.id(), &resource)?
            .ok_or_else(|| CommandError::new("trae_binding_unverified", "Trae 尚未完成模型切换"))?;
        validate_checkpoint(&checkpoint, &resource)?;
        if checkpoint.pending {
            return Err(
                CommandError::new("trae_recovery_pending", "Trae 上次模型操作尚未完成")
                    .with_recovery("请重新点击切换或恢复原始模型。"),
            );
        }
        let active = checkpoint
            .active
            .as_deref()
            .ok_or_else(|| CommandError::new("trae_binding_unverified", "Trae 当前使用原始模型"))?;
        let expected = ManagedInput::from_desired(desired);
        if !checkpoint
            .owned
            .iter()
            .chain(checkpoint.borrowed.iter())
            .any(|row| row.display_name == active && row.input.matches(&expected))
        {
            return Err(CommandError::new(
                "trae_binding_changed",
                "Trae 已管理模型与当前绑定不一致",
            ));
        }
        let cached_models = ui::cached_custom_models(detection.config_path.as_deref())?;
        if !cached_models.contains(active) {
            return Err(CommandError::new(
                "trae_binding_changed",
                "Trae 已管理模型不再存在",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct UiApplyPlan {
    row: ManagedRow,
    previous_selection: String,
    created: bool,
    borrowed: bool,
    replaced_row: Option<ManagedRow>,
}

#[derive(Clone, Copy, Default)]
struct UiMutationState {
    replacement_delete_attempted: bool,
    model_creation_attempted: bool,
    model_created: bool,
}

struct UiApplyAttempt {
    result: AppResult<()>,
    mutation: UiMutationState,
}

#[derive(Default)]
struct UiRollback {
    selection_restored: bool,
    model_removed: bool,
    replaced_row_restored: bool,
}

impl UiRollback {
    fn complete(&self) -> bool {
        self.selection_restored && self.model_removed && self.replaced_row_restored
    }
}

fn record_rollback_checkpoint(
    checkpoint: &mut Checkpoint,
    previous_active: &Option<String>,
    previous_borrowed: &[ManagedRow],
    plan: &UiApplyPlan,
    rollback: &UiRollback,
) {
    checkpoint.active = previous_active.clone();
    if plan.created && rollback.model_removed {
        checkpoint
            .owned
            .retain(|row| row.display_name != plan.row.display_name);
    }
    if rollback.complete() {
        checkpoint.borrowed = previous_borrowed.to_vec();
        if let Some(replaced) = plan.replaced_row.as_ref() {
            if let Some(row) = checkpoint
                .owned
                .iter_mut()
                .find(|row| row.display_name == replaced.display_name)
            {
                *row = replaced.clone();
            } else {
                checkpoint.owned.push(replaced.clone());
            }
        }
    }
    checkpoint.pending = !rollback.complete();
}

fn upsert_managed_row(rows: &mut Vec<ManagedRow>, desired: &ManagedRow) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.display_name == desired.display_name)
    {
        *row = desired.clone();
    } else {
        rows.push(desired.clone());
    }
}

fn plan_ui_change(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    input: &ManagedInput,
    matching: Option<&ManagedRow>,
    replacement: Option<&ManagedRow>,
    known_names: &HashSet<String>,
) -> AppResult<UiApplyPlan> {
    let snapshot = ui.snapshot_interactive(kind, detection)?;
    if let Some(row) = matching {
        if snapshot.custom_models.contains(&row.display_name) {
            return Ok(UiApplyPlan {
                row: row.clone(),
                previous_selection: snapshot.selection,
                created: false,
                borrowed: false,
                replaced_row: None,
            });
        }
    }

    let discovered_legacy_name =
        if is_legacy_managed_display_name_for_input(&snapshot.selection, input) {
            Some(snapshot.selection.clone())
        } else {
            snapshot
                .custom_models
                .iter()
                .filter(|name| is_legacy_managed_display_name_for_input(name, input))
                .min()
                .cloned()
        };
    let replaced_row = replacement
        .filter(|row| snapshot.custom_models.contains(&row.display_name))
        .cloned()
        .or_else(|| {
            discovered_legacy_name.map(|display_name| ManagedRow {
                display_name,
                input: input.clone(),
            })
        });
    let expected_name = managed_display_name_base(input);
    if replaced_row.is_none() {
        let existing_name = snapshot
            .custom_models_by_id
            .get(&input.model_id)
            .and_then(|names| {
                names
                    .contains(&expected_name)
                    .then_some(expected_name.clone())
                    .or_else(|| names.iter().next().cloned())
            })
            .or_else(|| {
                snapshot
                    .custom_models
                    .contains(&expected_name)
                    .then_some(expected_name.clone())
            });
        if let Some(display_name) = existing_name {
            return Ok(UiApplyPlan {
                row: ManagedRow {
                    display_name,
                    input: input.clone(),
                },
                previous_selection: snapshot.selection,
                created: false,
                borrowed: true,
                replaced_row: None,
            });
        }
    }
    let display_name = if replaced_row
        .as_ref()
        .is_some_and(|row| row.display_name == expected_name)
    {
        expected_name
    } else {
        available_display_name(input, known_names, &snapshot.custom_models)
    };
    Ok(UiApplyPlan {
        row: ManagedRow {
            display_name,
            input: input.clone(),
        },
        previous_selection: snapshot.selection,
        created: true,
        borrowed: false,
        replaced_row,
    })
}

fn apply_ui_change(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    plan: &UiApplyPlan,
) -> UiApplyAttempt {
    let mut mutation = UiMutationState::default();
    if let Some(replaced) = plan.replaced_row.as_ref() {
        // Trae rejects two custom rows with the same model ID even when their
        // display names differ. Remove the exact previously managed row first;
        // rollback can recreate it from the current credential if a later step
        // fails.
        mutation.replacement_delete_attempted = true;
        if let Err(error) = ui.delete_model(kind, detection, &replaced.display_name) {
            return UiApplyAttempt {
                result: Err(error),
                mutation,
            };
        }
    }
    if plan.created {
        mutation.model_creation_attempted = true;
        if let Err(error) = ui.add_model(kind, detection, &ui_model_input(&plan.row, None)) {
            // Trae performs its connectivity test and persists the custom
            // model asynchronously. In some releases the official form can
            // disappear between those two UI states, causing the immediate
            // UI confirmation to fail even though the exact model is already
            // durable. Re-read through the adapter contract and finish this
            // operation instead of making the user click Switch again.
            let persisted = ui
                .snapshot_interactive(kind, detection)
                .is_ok_and(|snapshot| snapshot_contains_plan(&snapshot, plan));
            if !persisted {
                return UiApplyAttempt {
                    result: Err(error),
                    mutation,
                };
            }
        }
        mutation.model_created = true;
    }
    let mut result = ui.select_model(kind, detection, &plan.row.display_name);
    if result.is_err() {
        let selected = ui
            .snapshot_interactive(kind, detection)
            .is_ok_and(|snapshot| snapshot.selection == plan.row.display_name);
        if selected {
            result = Ok(());
        }
    }
    // select_model performs an exact selector reread on both platforms. Do
    // not immediately gate that verified result on Trae's asynchronously
    // persisted model-list cache: the UI can already be using the new model
    // while that cache still contains the previous catalog.
    UiApplyAttempt { result, mutation }
}

fn snapshot_contains_plan(snapshot: &ui::TraeUiSnapshot, plan: &UiApplyPlan) -> bool {
    snapshot
        .custom_models_by_id
        .get(&plan.row.input.model_id)
        .is_some_and(|names| names.contains(&plan.row.display_name))
        || snapshot.custom_models.contains(&plan.row.display_name)
}

fn ui_model_input(row: &ManagedRow, credential: Option<&Zeroizing<String>>) -> ui::TraeModelInput {
    ui::TraeModelInput {
        display_name: row.display_name.clone(),
        model_id: row.input.model_id.clone(),
        protocol: row.input.protocol,
        base_url: row.input.base_url.clone(),
        credential: credential
            .cloned()
            .unwrap_or_else(|| row.input.credential.clone()),
    }
}

async fn cleanup_legacy_rows(
    ui: Arc<dyn ui::TraeUi>,
    kind: TraeKind,
    detection: AgentDetection,
    checkpoint: &mut Checkpoint,
) -> bool {
    let legacy_names = checkpoint
        .owned
        .iter()
        .filter(|row| is_legacy_managed_row(row))
        .map(|row| row.display_name.clone())
        .collect::<Vec<_>>();
    if legacy_names.is_empty() {
        return true;
    }

    let attempted_names = legacy_names.clone();
    let deleted = tokio::task::spawn_blocking(move || {
        legacy_names
            .into_iter()
            .filter(|name| ui.delete_model(kind, &detection, name).is_ok())
            .collect::<HashSet<_>>()
    })
    .await
    .unwrap_or_default();
    checkpoint
        .owned
        .retain(|row| !deleted.contains(&row.display_name));
    deleted.len() == attempted_names.len()
}

async fn rollback_ui_change(
    ui: Arc<dyn ui::TraeUi>,
    kind: TraeKind,
    detection: AgentDetection,
    plan: UiApplyPlan,
    mutation: UiMutationState,
) -> UiRollback {
    tokio::task::spawn_blocking(move || {
        let model_absent_after_failed_creation = !mutation.model_created
            && ui::cached_custom_models(detection.config_path.as_deref())
                .is_ok_and(|models| !models.contains(&plan.row.display_name));
        let model_removed = !mutation.model_creation_attempted
            || model_absent_after_failed_creation
            || ui
                .delete_model(kind, &detection, &plan.row.display_name)
                .is_ok();
        let replaced_row_restored = !mutation.replacement_delete_attempted
            || plan.replaced_row.as_ref().is_none_or(|replaced| {
                ui::cached_custom_models(detection.config_path.as_deref())
                    .is_ok_and(|models| models.contains(&replaced.display_name))
                    || ui
                        .add_model(
                            kind,
                            &detection,
                            &ui_model_input(replaced, Some(&plan.row.input.credential)),
                        )
                        .is_ok()
                    || ui
                        .select_model(kind, &detection, &replaced.display_name)
                        .is_ok()
            });
        let selection_restored = replaced_row_restored
            && ui
                .select_model(kind, &detection, &plan.previous_selection)
                .is_ok();
        UiRollback {
            selection_restored,
            model_removed,
            replaced_row_restored,
        }
    })
    .await
    .unwrap_or_default()
}

fn available_display_name(
    input: &ManagedInput,
    known_names: &HashSet<String>,
    actual_names: &HashSet<String>,
) -> String {
    let base = managed_display_name_base(input);
    if !known_names.contains(&base) && !actual_names.contains(&base) {
        return base;
    }
    (2..100)
        .map(|index| compact_label(&format!("{base} · {index}"), 64))
        .find(|candidate| !known_names.contains(candidate) && !actual_names.contains(candidate))
        .unwrap_or_else(|| compact_label(&format!("{base} · 新"), 64))
}

fn managed_display_name_base(input: &ManagedInput) -> String {
    let provider = compact_label(&input.provider_name, 18);
    let model = compact_label(&input.model_id, 24);
    compact_label(&format!("{provider} · {model}"), 58)
}

fn legacy_managed_display_name_base(input: &ManagedInput) -> String {
    let provider = compact_label(&input.provider_name, 18);
    let model = compact_label(&input.model_id, 24);
    compact_label(&format!("{LEGACY_MANAGED_PREFIX}{provider} · {model}"), 58)
}

fn is_legacy_managed_display_name_for_input(name: &str, input: &ManagedInput) -> bool {
    let base = legacy_managed_display_name_base(input);
    name == base
        || name
            .strip_prefix(&format!("{base} · "))
            .and_then(|suffix| suffix.parse::<u8>().ok())
            .is_some_and(|suffix| (2..=99).contains(&suffix))
}

fn is_legacy_managed_row(row: &ManagedRow) -> bool {
    is_legacy_managed_display_name_for_input(&row.display_name, &row.input)
}

fn compact_label(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut output = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() && max_chars > 1 {
        output.pop();
        output.push('…');
    }
    output
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedInput {
    provider_name: String,
    model_id: String,
    protocol: ApiProtocol,
    base_url: String,
    credential_hash: String,
    #[serde(skip)]
    credential: Zeroizing<String>,
}

impl ManagedInput {
    fn from_desired(desired: &DesiredAgentBinding<'_>) -> Self {
        Self {
            provider_name: desired.provider_name.to_owned(),
            model_id: desired.model_id.to_owned(),
            protocol: desired.source_protocol,
            base_url: desired.base_url.trim_end_matches('/').to_owned(),
            credential_hash: hex::encode(Sha256::digest(desired.credential.as_bytes())),
            credential: Zeroizing::new(desired.credential.to_owned()),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.provider_name == other.provider_name
            && self.model_id == other.model_id
            && self.protocol == other.protocol
            && self.base_url == other.base_url
            && self.credential_hash == other.credential_hash
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRow {
    display_name: String,
    input: ManagedInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Checkpoint {
    version: u8,
    account_scope: String,
    baseline_selection: String,
    owned: Vec<ManagedRow>,
    #[serde(default)]
    borrowed: Vec<ManagedRow>,
    active: Option<String>,
    pending: bool,
}

fn load_checkpoint(
    transaction: &ConfigTransaction,
    agent_id: &str,
    resource: &str,
) -> AppResult<Option<Checkpoint>> {
    transaction
        .read_service_checkpoint(agent_id, resource)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| checkpoint_invalid()))
        .transpose()
}

fn save_checkpoint(
    transaction: &ConfigTransaction,
    agent_id: &str,
    resource: &str,
    checkpoint: &Checkpoint,
) -> AppResult<()> {
    let bytes = serde_json::to_vec(checkpoint)
        .map_err(|_| CommandError::internal("无法序列化 Trae 恢复记录"))?;
    transaction.save_service_checkpoint(agent_id, resource, &bytes)
}

fn historical_owned_rows(
    transaction: &ConfigTransaction,
    agent_id: &str,
    resource: &str,
) -> AppResult<Vec<ManagedRow>> {
    Ok(transaction
        .read_service_checkpoint_history(agent_id, resource)?
        .into_iter()
        .filter_map(|bytes| serde_json::from_slice::<Checkpoint>(&bytes).ok())
        .filter(|checkpoint| {
            checkpoint.version == CHECKPOINT_VERSION && checkpoint.account_scope == resource
        })
        .flat_map(|checkpoint| checkpoint.owned)
        .collect())
}

fn validate_checkpoint(checkpoint: &Checkpoint, resource: &str) -> AppResult<()> {
    if checkpoint.version != CHECKPOINT_VERSION || checkpoint.account_scope != resource {
        return Err(checkpoint_invalid());
    }
    Ok(())
}

fn checkpoint_invalid() -> CommandError {
    CommandError::new(
        "trae_checkpoint_invalid",
        "Trae 模型恢复记录无效，已阻止继续修改",
    )
    .with_recovery("请保留现有备份并联系维护者确认后再操作。")
}

fn active_account(path: &Path, kind: TraeKind) -> AppResult<String> {
    if !path.is_file() {
        return Err(CommandError::new(
            "trae_profile_uninitialized",
            format!(
                "尚未找到 {} 用户数据，请先打开并登录。",
                kind.display_name()
            ),
        ));
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 用户状态"))?;
    let mut statement = connection
        .prepare("SELECT key FROM ItemTable WHERE key LIKE '%AI.agent.model.model_list_map'")
        .map_err(|_| CommandError::new("trae_profile_unreadable", "Trae 用户状态格式无法识别"))?;
    let keys = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| CommandError::new("trae_profile_unreadable", "无法读取 Trae 当前账号"))?;
    for key in keys.flatten() {
        let suffix = if key.contains(':') {
            ":AI.agent.model.model_list_map"
        } else {
            "_AI.agent.model.model_list_map"
        };
        if let Some(account) = key.strip_suffix(suffix) {
            if !account.is_empty() && account.chars().all(|character| character.is_ascii_digit()) {
                return Ok(account.to_owned());
            }
        }
    }
    Err(CommandError::new(
        "trae_account_missing",
        format!("未识别到 {} 当前登录账号", kind.display_name()),
    )
    .with_recovery("请打开应用并完成登录后，在 AT-Switch 中刷新状态。"))
}

fn state_database(detection: &AgentDetection) -> AppResult<&Path> {
    detection
        .config_path
        .as_deref()
        .ok_or_else(|| CommandError::new("trae_profile_missing", "无法定位 Trae 用户状态数据库"))
}

fn ui_required(display_name: &str) -> CommandError {
    CommandError::new(
        "trae_ui_config_required",
        format!("{display_name} 模型配置必须通过官方界面完成"),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    use crate::infrastructure::MemorySecretStore;

    use super::*;

    #[derive(Default)]
    struct FakeUiState {
        selection: String,
        models: HashSet<String>,
        model_ids: HashMap<String, String>,
        cache_visible: bool,
        fail_after_persisting_next_add: bool,
        additions: usize,
        deletions: usize,
    }

    struct FakeUi {
        state: Mutex<FakeUiState>,
    }

    impl FakeUi {
        fn new(selection: &str) -> Self {
            Self {
                state: Mutex::new(FakeUiState {
                    selection: selection.to_owned(),
                    cache_visible: true,
                    ..Default::default()
                }),
            }
        }

        fn snapshot_value(&self) -> ui::TraeUiSnapshot {
            let state = self.state.lock().expect("fake UI state");
            let mut custom_models_by_id =
                std::collections::HashMap::<String, std::collections::BTreeSet<String>>::new();
            for (display_name, model_id) in &state.model_ids {
                custom_models_by_id
                    .entry(model_id.clone())
                    .or_default()
                    .insert(display_name.clone());
            }
            ui::TraeUiSnapshot {
                selection: state.selection.clone(),
                custom_models: if state.cache_visible {
                    state.models.clone()
                } else {
                    HashSet::new()
                },
                custom_models_by_id: if state.cache_visible {
                    custom_models_by_id
                } else {
                    Default::default()
                },
            }
        }
    }

    impl ui::TraeUi for FakeUi {
        fn snapshot_interactive(
            &self,
            _: TraeKind,
            _: &AgentDetection,
        ) -> AppResult<ui::TraeUiSnapshot> {
            Ok(self.snapshot_value())
        }

        fn add_model(
            &self,
            _: TraeKind,
            _: &AgentDetection,
            input: &ui::TraeModelInput,
        ) -> AppResult<()> {
            let mut state = self.state.lock().expect("fake UI state");
            if state
                .model_ids
                .iter()
                .any(|(name, model_id)| name != &input.display_name && model_id == &input.model_id)
            {
                return Err(CommandError::new(
                    "test_duplicate_model_id",
                    "test duplicate model id",
                ));
            }
            state.additions += 1;
            state.models.insert(input.display_name.clone());
            state
                .model_ids
                .insert(input.display_name.clone(), input.model_id.clone());
            if state.fail_after_persisting_next_add {
                state.fail_after_persisting_next_add = false;
                return Err(CommandError::new(
                    "test_add_confirmation_lost",
                    "test add confirmation lost",
                ));
            }
            Ok(())
        }

        fn select_model(
            &self,
            _: TraeKind,
            _: &AgentDetection,
            display_name: &str,
        ) -> AppResult<()> {
            let mut state = self.state.lock().expect("fake UI state");
            if display_name == "Auto Mode" || state.models.contains(display_name) {
                state.selection = display_name.to_owned();
                Ok(())
            } else {
                Err(CommandError::new(
                    "test_model_missing",
                    "test model missing",
                ))
            }
        }

        fn delete_model(
            &self,
            _: TraeKind,
            _: &AgentDetection,
            display_name: &str,
        ) -> AppResult<()> {
            let mut state = self.state.lock().expect("fake UI state");
            if state.models.remove(display_name) {
                state.deletions += 1;
            }
            state.model_ids.remove(display_name);
            Ok(())
        }
    }

    fn test_detection(temp: &tempfile::TempDir) -> AgentDetection {
        let database = temp.path().join("state.vscdb");
        let connection = Connection::open(&database).expect("state database");
        connection
            .execute("CREATE TABLE ItemTable (key TEXT, value TEXT)", [])
            .expect("item table");
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                ["12345:AI.agent.model.model_list_map", "{}"],
            )
            .expect("account row");
        let mut detection = AgentDetection::manual("traecode", "TraeCode", None, "test detection");
        detection.config_path = Some(database);
        detection
    }

    fn desired<'a>(credential: &'a str) -> DesiredAgentBinding<'a> {
        DesiredAgentBinding {
            mode: AgentBindingMode::Direct,
            provider_name: "Fictional Provider",
            model_id: "fictional-model",
            supports_tools: true,
            source_protocol: ApiProtocol::OpenaiResponses,
            upstream_protocol: ApiProtocol::OpenaiResponses,
            base_url: "https://provider.example.test/v1",
            credential,
        }
    }

    #[test]
    fn display_names_are_bounded_and_do_not_duplicate_existing_rows() {
        let input = ManagedInput {
            provider_name: "A very long fictional provider name".into(),
            model_id: "a-very-long-fictional-model-identifier-for-testing".into(),
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://provider.example.test/v1".into(),
            credential_hash: "hash".into(),
            credential: Zeroizing::new("secret".into()),
        };
        let first = available_display_name(&input, &HashSet::new(), &HashSet::new());
        assert!(!first.starts_with(LEGACY_MANAGED_PREFIX));
        assert!(first.chars().count() <= 64);
        let next = available_display_name(&input, &HashSet::from([first.clone()]), &HashSet::new());
        assert_ne!(first, next);
        assert!(next.chars().count() <= 64);
    }

    #[test]
    fn interrupted_checkpointed_row_is_reused_without_duplication() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let orphan = format!("{} · 2", managed_display_name_base(&input));
        let fake = FakeUi::new(&orphan);
        fake.state
            .lock()
            .expect("fake UI state")
            .models
            .insert(orphan.clone());

        let plan = plan_ui_change(
            &fake,
            TraeKind::Work,
            &detection,
            &input,
            Some(&ManagedRow {
                display_name: orphan.clone(),
                input: input.clone(),
            }),
            None,
            &HashSet::from([orphan.clone()]),
        )
        .expect("interrupted plan");

        assert!(!plan.created);
        assert_eq!(plan.row.display_name, orphan);
        assert!(plan.row.input.matches(&input));
    }

    #[test]
    fn credential_is_hashed_in_serialized_checkpoint() {
        let input = ManagedInput {
            provider_name: "Fictional".into(),
            model_id: "fictional-model".into(),
            protocol: ApiProtocol::AnthropicMessages,
            base_url: "https://provider.example.test/v1".into(),
            credential_hash: "expected-hash".into(),
            credential: Zeroizing::new("fictional-secret".into()),
        };
        let encoded = serde_json::to_string(&input).unwrap();
        assert!(encoded.contains("expected-hash"));
        assert!(!encoded.contains("fictional-secret"));
    }

    #[test]
    fn rejects_proxy_and_cross_protocol_direct_bindings() {
        let adapter = TraeAdapter::code();
        let mut proxy = desired("fictional-secret");
        proxy.mode = AgentBindingMode::Proxy;
        assert_eq!(
            adapter.validate_binding(&proxy).unwrap_err().code,
            "trae_proxy_unsupported"
        );
        let mut mismatch = desired("fictional-secret");
        mismatch.source_protocol = ApiProtocol::OpenaiChatCompletions;
        assert_eq!(
            adapter.validate_binding(&mismatch).unwrap_err().code,
            "trae_direct_protocol_mismatch"
        );
    }

    #[tokio::test]
    async fn direct_apply_is_idempotent_and_restore_removes_only_owned_model() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("User custom model"));
        fake.state
            .lock()
            .expect("fake UI state")
            .models
            .insert("User custom model".to_owned());
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        let commits = AtomicUsize::new(0);
        let commit = || {
            commits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };

        adapter
            .apply(
                &detection,
                &desired("fictional-secret"),
                &transaction,
                &commit,
            )
            .await
            .expect("first apply");
        adapter
            .apply(
                &detection,
                &desired("fictional-secret"),
                &transaction,
                &commit,
            )
            .await
            .expect("repeat apply");
        {
            let state = fake.state.lock().expect("fake UI state");
            assert_eq!(state.additions, 1);
            assert!(!state.selection.starts_with(LEGACY_MANAGED_PREFIX));
            assert_eq!(state.models.len(), 2);
        }
        let checkpoint = transaction
            .read_service_checkpoint("traecode", "traecode:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert!(!checkpoint
            .windows("fictional-secret".len())
            .any(|part| part == b"fictional-secret"));

        adapter
            .restore(&detection, &transaction, &commit)
            .await
            .expect("restore");
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "User custom model");
        assert_eq!(
            state.models,
            HashSet::from(["User custom model".to_owned()])
        );
        assert_eq!(state.deletions, 1);
        assert_eq!(commits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn existing_user_model_id_is_selected_without_creation_or_deletion() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let desired = desired("fictional-secret");
        let display_name = "User configured fictional model".to_owned();
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(display_name.clone());
            state
                .model_ids
                .insert(display_name.clone(), desired.model_id.to_owned());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Work, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );

        adapter
            .apply(&detection, &desired, &transaction, &|| Ok(()))
            .await
            .expect("reuse existing model");
        {
            let state = fake.state.lock().expect("fake UI state");
            assert_eq!(state.selection, display_name);
            assert_eq!(state.additions, 0);
            assert_eq!(state.deletions, 0);
        }
        let checkpoint = load_checkpoint(&transaction, "traework", "traework:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert!(checkpoint.owned.is_empty());
        assert_eq!(checkpoint.borrowed.len(), 1);
        assert_eq!(checkpoint.active.as_deref(), Some(display_name.as_str()));

        adapter
            .restore(&detection, &transaction, &|| Ok(()))
            .await
            .expect("restore selection");
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Auto Mode");
        assert_eq!(state.models, HashSet::from([display_name]));
        assert_eq!(state.additions, 0);
        assert_eq!(state.deletions, 0);
        drop(state);
        let checkpoint = load_checkpoint(&transaction, "traework", "traework:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert!(checkpoint.borrowed.is_empty());
        assert!(checkpoint.active.is_none());
        assert!(!checkpoint.pending);
    }

    #[tokio::test]
    async fn verified_selector_change_is_not_rejected_by_a_lagging_model_cache() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        fake.state.lock().expect("fake UI state").cache_visible = false;
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );

        adapter
            .apply(
                &detection,
                &desired("fictional-secret"),
                &transaction,
                &|| Ok(()),
            )
            .await
            .expect("selector-verified apply");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Fictional Provider · fictional-model");
        drop(state);
        assert_eq!(
            adapter
                .checkpoint_status(&detection, &transaction)
                .expect("checkpoint status"),
            Some((true, false))
        );
    }

    #[tokio::test]
    async fn persisted_model_survives_a_lost_add_confirmation_without_a_second_click() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        fake.state
            .lock()
            .expect("fake UI state")
            .fail_after_persisting_next_add = true;
        let adapter = TraeAdapter::with_ui(TraeKind::Work, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );

        adapter
            .apply(
                &detection,
                &desired("fictional-secret"),
                &transaction,
                &|| Ok(()),
            )
            .await
            .expect("same-operation recovery");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Fictional Provider · fictional-model");
        assert_eq!(state.additions, 1);
        assert_eq!(state.deletions, 0);
        drop(state);
        assert_eq!(
            adapter
                .checkpoint_status(&detection, &transaction)
                .expect("checkpoint status"),
            Some((true, false))
        );
    }

    #[tokio::test]
    async fn rollback_treats_an_uncreated_new_row_as_already_removed() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let legacy = ManagedRow {
            display_name: legacy_managed_display_name_base(&input),
            input: input.clone(),
        };
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        fake.state
            .lock()
            .expect("fake UI state")
            .models
            .insert(legacy.display_name.clone());

        let rollback = rollback_ui_change(
            fake.clone(),
            TraeKind::Work,
            detection,
            UiApplyPlan {
                row: ManagedRow {
                    display_name: managed_display_name_base(&input),
                    input,
                },
                previous_selection: "Auto Mode".to_owned(),
                created: true,
                borrowed: false,
                replaced_row: Some(legacy),
            },
            UiMutationState::default(),
        )
        .await;

        assert!(rollback.complete());
        assert_eq!(fake.state.lock().expect("fake UI state").deletions, 0);
    }

    #[tokio::test]
    async fn successful_switch_migrates_only_checkpoint_owned_legacy_rows() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let desired = desired("fictional-secret");
        let input = ManagedInput::from_desired(&desired);
        let legacy_name = legacy_managed_display_name_base(&input);
        let user_model = "AT-Switch · user-created model".to_owned();
        let fake = Arc::new(FakeUi::new(&legacy_name));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(legacy_name.clone());
            state
                .model_ids
                .insert(legacy_name.clone(), input.model_id.clone());
            state.models.insert(user_model.clone());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "traecode",
            "traecode:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "traecode:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                owned: vec![ManagedRow {
                    display_name: legacy_name.clone(),
                    input: input.clone(),
                }],
                borrowed: Vec::new(),
                active: Some(legacy_name.clone()),
                pending: false,
            },
        )
        .expect("legacy checkpoint");

        adapter
            .apply(&detection, &desired, &transaction, &|| Ok(()))
            .await
            .expect("legacy migration");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Fictional Provider · fictional-model");
        assert!(!state.models.contains(&legacy_name));
        assert!(state.models.contains(&user_model));
        assert_eq!(state.additions, 1);
        assert_eq!(state.deletions, 1);
        drop(state);

        let checkpoint = load_checkpoint(&transaction, "traecode", "traecode:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert!(!checkpoint.pending);
        assert_eq!(
            checkpoint.active.as_deref(),
            Some("Fictional Provider · fictional-model")
        );
        assert_eq!(checkpoint.owned.len(), 1);
        assert!(!is_legacy_managed_row(&checkpoint.owned[0]));
    }

    #[tokio::test]
    async fn legacy_prefix_recovers_ownership_when_an_old_checkpoint_missed_the_row() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let desired = desired("fictional-secret");
        let input = ManagedInput::from_desired(&desired);
        let legacy_name = legacy_managed_display_name_base(&input);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(legacy_name.clone());
            state
                .model_ids
                .insert(legacy_name.clone(), input.model_id.clone());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );

        adapter
            .apply(&detection, &desired, &transaction, &|| Ok(()))
            .await
            .expect("legacy discovery migration");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Fictional Provider · fictional-model");
        assert!(!state.models.contains(&legacy_name));
        assert_eq!(state.deletions, 1);
        drop(state);
        let checkpoint = load_checkpoint(&transaction, "traecode", "traecode:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert_eq!(checkpoint.owned.len(), 1);
        assert!(!is_legacy_managed_row(&checkpoint.owned[0]));
    }

    #[tokio::test]
    async fn failed_legacy_migration_recreates_the_old_row_and_selection() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let desired = desired("fictional-secret");
        let input = ManagedInput::from_desired(&desired);
        let legacy_name = legacy_managed_display_name_base(&input);
        let fake = Arc::new(FakeUi::new(&legacy_name));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(legacy_name.clone());
            state
                .model_ids
                .insert(legacy_name.clone(), input.model_id.clone());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "traecode",
            "traecode:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "traecode:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                owned: vec![ManagedRow {
                    display_name: legacy_name.clone(),
                    input,
                }],
                borrowed: Vec::new(),
                active: Some(legacy_name.clone()),
                pending: false,
            },
        )
        .expect("legacy checkpoint");

        let error = match adapter
            .apply(&detection, &desired, &transaction, &|| {
                Err(CommandError::new("test_commit_failed", "commit failed"))
            })
            .await
        {
            Ok(_) => panic!("migration commit must fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "test_commit_failed");
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, legacy_name);
        assert_eq!(state.models, HashSet::from([legacy_name.clone()]));
        assert_eq!(
            state.model_ids.get(&legacy_name).map(String::as_str),
            Some(desired.model_id)
        );
        drop(state);
        assert_eq!(
            adapter
                .checkpoint_status(&detection, &transaction)
                .expect("checkpoint status"),
            Some((true, false))
        );
    }

    #[tokio::test]
    async fn owned_prefixless_row_is_replaced_in_place_when_its_input_changes() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let next_desired = desired("rotated-fictional-secret");
        let next_input = ManagedInput::from_desired(&next_desired);
        let mut previous_input = ManagedInput::from_desired(&desired("previous-secret"));
        previous_input.credential = Zeroizing::new(String::new());
        let display_name = managed_display_name_base(&next_input);
        let fake = Arc::new(FakeUi::new(&display_name));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(display_name.clone());
            state
                .model_ids
                .insert(display_name.clone(), next_input.model_id.clone());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "traecode",
            "traecode:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "traecode:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                owned: vec![ManagedRow {
                    display_name: display_name.clone(),
                    input: previous_input,
                }],
                borrowed: Vec::new(),
                active: Some(display_name.clone()),
                pending: false,
            },
        )
        .expect("previous checkpoint");

        adapter
            .apply(&detection, &next_desired, &transaction, &|| Ok(()))
            .await
            .expect("replace stale managed row");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, display_name);
        assert_eq!(state.models, HashSet::from([display_name.clone()]));
        assert_eq!(state.additions, 1);
        assert_eq!(state.deletions, 1);
        drop(state);
        let checkpoint = load_checkpoint(&transaction, "traecode", "traecode:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert_eq!(checkpoint.owned.len(), 1);
        assert!(checkpoint.owned[0].input.matches(&next_input));
    }

    #[tokio::test]
    async fn encrypted_history_recovers_a_missing_prefixless_owned_row() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let next_desired = desired("rotated-fictional-secret");
        let next_input = ManagedInput::from_desired(&next_desired);
        let mut previous_input = ManagedInput::from_desired(&desired("previous-secret"));
        previous_input.credential = Zeroizing::new(String::new());
        let display_name = managed_display_name_base(&next_input);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(display_name.clone());
            state
                .model_ids
                .insert(display_name.clone(), next_input.model_id.clone());
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "traecode",
            "traecode:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "traecode:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                owned: vec![ManagedRow {
                    display_name: display_name.clone(),
                    input: previous_input,
                }],
                borrowed: Vec::new(),
                active: None,
                pending: false,
            },
        )
        .expect("historical owned row");
        save_checkpoint(
            &transaction,
            "traecode",
            "traecode:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "traecode:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                owned: Vec::new(),
                borrowed: Vec::new(),
                active: None,
                pending: false,
            },
        )
        .expect("current checkpoint without row");

        adapter
            .apply(&detection, &next_desired, &transaction, &|| Ok(()))
            .await
            .expect("recover historical ownership");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, display_name);
        assert_eq!(state.models, HashSet::from([display_name.clone()]));
        assert_eq!(state.additions, 1);
        assert_eq!(state.deletions, 1);
        drop(state);
        let checkpoint = load_checkpoint(&transaction, "traecode", "traecode:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert_eq!(checkpoint.owned.len(), 1);
        assert!(checkpoint.owned[0].input.matches(&next_input));
    }

    #[tokio::test]
    async fn failed_initial_binding_commit_restores_selection_and_removes_new_model() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        let commit = || Err(CommandError::new("test_commit_failed", "commit failed"));

        let error = match adapter
            .apply(
                &detection,
                &desired("fictional-secret"),
                &transaction,
                &commit,
            )
            .await
        {
            Ok(_) => panic!("commit must fail"),
            Err(error) => error,
        };
        assert_eq!(error.code, "test_commit_failed");
        assert_eq!(
            fake.state.lock().expect("fake UI state").selection,
            "Auto Mode"
        );
        {
            let state = fake.state.lock().expect("fake UI state");
            assert!(state.models.is_empty());
            assert_eq!(state.deletions, 1);
        }
        assert_eq!(
            adapter
                .checkpoint_status(&detection, &transaction)
                .expect("checkpoint status"),
            Some((false, false))
        );
    }

    #[tokio::test]
    async fn failed_second_binding_commit_restores_the_previous_managed_model() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        let adapter = TraeAdapter::with_ui(TraeKind::Code, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );

        adapter
            .apply(
                &detection,
                &desired("fictional-secret-a"),
                &transaction,
                &|| Ok(()),
            )
            .await
            .expect("first apply");
        let first_selection = fake.state.lock().expect("fake UI state").selection.clone();

        let mut second = desired("fictional-secret-b");
        second.model_id = "fictional-model-b";
        let error = match adapter
            .apply(&detection, &second, &transaction, &|| {
                Err(CommandError::new("test_commit_failed", "commit failed"))
            })
            .await
        {
            Ok(_) => panic!("second commit must fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "test_commit_failed");
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, first_selection);
        assert_eq!(state.models, HashSet::from([first_selection]));
        assert_eq!(state.additions, 2);
        assert_eq!(state.deletions, 1);
        drop(state);
        assert_eq!(
            adapter
                .checkpoint_status(&detection, &transaction)
                .expect("checkpoint status"),
            Some((true, false))
        );
    }

    #[test]
    fn missing_or_uninitialized_profile_is_not_treated_as_writable() {
        let temp = tempfile::tempdir().expect("temp");
        let missing = temp.path().join("missing.vscdb");
        assert_eq!(
            active_account(&missing, TraeKind::Work).unwrap_err().code,
            "trae_profile_uninitialized"
        );
        let empty = temp.path().join("empty.vscdb");
        Connection::open(&empty).expect("empty database");
        assert_eq!(
            active_account(&empty, TraeKind::Work).unwrap_err().code,
            "trae_profile_unreadable"
        );
    }

    #[test]
    #[ignore = "requires installed, running and signed-in Trae desktop applications"]
    fn live_macos_snapshots_read_both_official_model_selectors() {
        if std::env::var("AT_SWITCH_TRAE_LIVE").as_deref() != Ok("1") {
            return;
        }
        let context = DiscoveryContext::native();
        for adapter in [TraeAdapter::code(), TraeAdapter::work()] {
            let detection = adapter.detect(&context);
            assert!(
                matches!(detection.config_health, AgentConfigHealth::Healthy),
                "{} detection: {:?}",
                adapter.display_name(),
                detection.message
            );
            let snapshot = adapter
                .ui
                .snapshot_interactive(adapter.kind, &detection)
                .expect("live model selector snapshot");
            assert!(
                !snapshot.selection.is_empty(),
                "{} selection",
                adapter.display_name()
            );
        }
    }
}
