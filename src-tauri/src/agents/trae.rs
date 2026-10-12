use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use futures_util::future::BoxFuture;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    domain::{
        AgentBindingMode, AgentConfigHealth, AgentInstallStatus, AgentRuntimeStatus, ApiProtocol,
        AppResult, CommandError,
    },
    services::{endpoint_url, BaselineSnapshot, ConfigTransaction},
};

use super::{
    lifecycle,
    locator::{locate_desktop_app, DiscoveryContext},
    service_adapter::{CommitBinding, ServiceConfigAdapter, ServiceConfigOutcome},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

#[path = "trae_ui.rs"]
mod ui;

const CHECKPOINT_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraeKind {
    Unified,
}

impl TraeKind {
    fn id(self) -> &'static str {
        "trae"
    }

    fn display_name(self) -> &'static str {
        "TRAE"
    }

    fn app_data_name(self) -> &'static str {
        "Trae CN"
    }

    fn selection_label(self) -> &'static str {
        "solo_agent_lite"
    }

    fn additional_selection_labels(self) -> &'static [&'static str] {
        &["solo_agent", "solo_coder", "ide_composer"]
    }

    fn supports_native_bridge(self, version: Option<&str>) -> bool {
        matches!(version, Some("3.4.2" | "3.4.2.0"))
    }
}

pub(super) struct TraeAdapter {
    kind: TraeKind,
    ui: Arc<dyn ui::TraeUi>,
}

struct TraeOperationGuard(Arc<dyn ui::TraeUi>, TraeKind);

impl Drop for TraeOperationGuard {
    fn drop(&mut self) {
        self.0.finish_operation(self.1);
    }
}

impl TraeAdapter {
    pub(super) fn unified() -> Self {
        Self::new(TraeKind::Unified)
    }

    fn new(kind: TraeKind) -> Self {
        Self {
            kind,
            ui: Arc::new(ui::SystemTraeUi::new()),
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
        let (mac_names, bundle_ids, windows_paths): (&[&str], &[&str], &[&str]) = (
            &["Trae CN.app"],
            &["cn.trae.app"],
            &["Programs/Trae CN/Trae CN.exe", "Trae CN/Trae CN.exe"],
        );
        let installation = locate_desktop_app(context, mac_names, bundle_ids, windows_paths);
        #[cfg(target_os = "windows")]
        let mut installation = installation;
        #[cfg(target_os = "windows")]
        if let Some(found) = installation.as_mut() {
            // Electron's Windows FileVersion identifies the shared runtime
            // build (for example 2.3.x), while ProductVersion identifies the
            // Trae release whose private model-service contract we verified.
            // Version gating must use ProductVersion or every supported
            // Windows installation is incorrectly forced into read-only mode.
            if let Some(version) = super::locator::windows_product_version(&found.path) {
                found.version = Some(version);
            }
        }
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
        if !self.kind.supports_native_bridge(
            detection
                .installation
                .as_ref()
                .and_then(|installation| installation.version.as_deref()),
        ) {
            detection.install_status = AgentInstallStatus::Installed;
            detection.config_health = AgentConfigHealth::UnsupportedVersion;
            detection.message = Some(format!(
                "{} 当前版本尚未通过无界面原生服务接入验证，AT-Switch 不会回退到可见界面操作。",
                self.display_name()
            ));
            return detection;
        }
        match active_account(&state_database, self.kind) {
            Ok(_) if cfg!(any(target_os = "macos", target_os = "windows")) => {
                detection.install_status = AgentInstallStatus::Installed;
                detection.config_health = AgentConfigHealth::Healthy;
                detection.write_supported = true;
                detection.needs_restart = self.ui.requires_initial_restart(self.kind, &detection);
                detection.message = Some(format!(
                    "{} 已识别；AT-Switch 将通过版本限定的 Trae 原生服务无界面切换直连模型。",
                    self.display_name()
                ));
            }
            Ok(_) => {
                detection.message = Some(format!(
                    "{} 原生模型服务接入目前仅支持 macOS 和 Windows。",
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
            let _session_guard = TraeOperationGuard(Arc::clone(&self.ui), self.kind);
            let was_running = detection.installation.as_ref().is_some_and(|installation| {
                lifecycle::runtime_status(installation, self.display_name())
                    == AgentRuntimeStatus::Running
            });
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
                    .map_err(|_| CommandError::internal("Trae 模型状态读取任务异常终止"))??;
                    Checkpoint {
                        version: CHECKPOINT_VERSION,
                        account_scope: resource.clone(),
                        baseline_selection: snapshot.recent_selection,
                        selection_baseline: snapshot.selection_baseline,
                        session_baselines: snapshot
                            .active_session_id
                            .zip(snapshot.active_selection)
                            .into_iter()
                            .collect(),
                        additional_baselines: BTreeMap::new(),
                        additional_selection_baselines: BTreeMap::new(),
                        additional_session_baselines: BTreeMap::new(),
                        ide_baseline: None,
                        selection_label: Some(kind.selection_label().to_owned()),
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
                .find(|row| row.input.matches(&input))
                .cloned()
                .or_else(|| {
                    historical_owned
                        .iter()
                        .find(|row| row.input.matches(&input))
                        .cloned()
                });
            let expected_name = managed_display_name_base(&input);
            let replacement = checkpoint
                .owned
                .iter()
                .find(|row| row.display_name == expected_name)
                .cloned()
                .or_else(|| {
                    let expected_name = managed_display_name_base(&input);
                    historical_owned
                        .iter()
                        .find(|row| row.display_name == expected_name)
                        .cloned()
                });
            let mut plan = tokio::task::spawn_blocking(move || {
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
            .map_err(|_| CommandError::internal("Trae 模型切换规划任务异常终止"))??;

            if let Some((session_id, selection)) = plan.previous_session.as_ref() {
                checkpoint
                    .session_baselines
                    .entry(session_id.clone())
                    .or_insert_with(|| selection.clone());
            }
            for (label, baseline) in &plan.additional_previous {
                if label == "ide_composer" && checkpoint.ide_baseline.is_none() {
                    checkpoint.ide_baseline = baseline.ide.clone();
                }
                checkpoint
                    .additional_baselines
                    .entry(label.clone())
                    .or_insert_with(|| baseline.selection.clone());
                if let Some(storage) = baseline.storage.as_ref() {
                    checkpoint
                        .additional_selection_baselines
                        .entry(label.clone())
                        .or_insert_with(|| storage.clone());
                }
                if let Some((session_id, selection)) = baseline.session.as_ref() {
                    checkpoint
                        .additional_session_baselines
                        .entry(label.clone())
                        .or_default()
                        .entry(session_id.clone())
                        .or_insert_with(|| selection.clone());
                }
            }

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
            // Record the exact cleanup target before mutating Trae's native model service.
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
            .map_err(|_| CommandError::internal("Trae 原生模型切换任务异常终止"))?;
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

            // Code's task route may become available only after native model
            // registration. Re-read the visible composer before committing:
            // writing only the recent default can leave an existing task on
            // Auto while AT-Switch incorrectly reports success.
            let visible: AppResult<()> = async {
                let ui = Arc::clone(&self.ui);
                let detection_for_snapshot = detection.clone();
                let snapshot = tokio::task::spawn_blocking(move || {
                    ui.snapshot_interactive(kind, &detection_for_snapshot)
                })
                .await
                .map_err(|_| CommandError::internal("Trae 当前任务校验任务异常终止"))??;
                if let Some(session_id) = snapshot.active_session_id.as_deref() {
                    if plan
                        .previous_session
                        .as_ref()
                        .is_some_and(|(id, _)| id != session_id)
                    {
                        return Err(CommandError::new(
                            "trae_active_session_changed",
                            "Trae 当前任务在切换期间发生变化，已回滚本次切换",
                        ));
                    }
                    if snapshot.active_selection.as_deref() != Some(plan.row.display_name.as_str())
                    {
                        if plan.previous_session.is_none() {
                            let baseline = snapshot.active_selection.clone().ok_or_else(|| {
                                CommandError::new(
                                    "trae_native_selection_missing",
                                    "Trae 当前任务的原模型尚未加载完成",
                                )
                            })?;
                            checkpoint
                                .session_baselines
                                .entry(session_id.to_owned())
                                .or_insert_with(|| baseline.clone());
                            plan.previous_session = Some((session_id.to_owned(), baseline));
                            save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;
                        }
                        let ui = Arc::clone(&self.ui);
                        let detection_for_select = detection.clone();
                        let display_name = plan.row.display_name.clone();
                        let session_id = session_id.to_owned();
                        tokio::task::spawn_blocking(move || {
                            ui.select_model_scoped(
                                kind,
                                &detection_for_select,
                                &display_name,
                                Some(&session_id),
                            )
                        })
                        .await
                        .map_err(|_| CommandError::internal("Trae 当前任务切换任务异常终止"))??;
                    }
                }
                let ui = Arc::clone(&self.ui);
                let detection_for_snapshot = detection.clone();
                let final_snapshot = tokio::task::spawn_blocking(move || {
                    ui.snapshot_interactive(kind, &detection_for_snapshot)
                })
                .await
                .map_err(|_| CommandError::internal("Trae 当前任务复核任务异常终止"))??;
                if final_snapshot.selection != plan.row.display_name {
                    #[cfg(test)]
                    eprintln!(
                        "TRAE_LIVE_MISMATCH primary expected={} actual={}",
                        plan.row.display_name, final_snapshot.selection
                    );
                    return Err(CommandError::new(
                        "trae_visible_selection_mismatch",
                        "Trae 当前任务没有选中目标模型，已回滚本次切换",
                    ));
                }
                for label in kind.additional_selection_labels() {
                    let previous = plan
                        .additional_previous
                        .get_mut(*label)
                        .ok_or_else(|| CommandError::internal("TRAE 模型选择缺少恢复基线"))?;
                    let ui = Arc::clone(&self.ui);
                    let detection_for_snapshot = detection.clone();
                    let current = tokio::task::spawn_blocking(move || {
                        ui.snapshot_label(kind, &detection_for_snapshot, label)
                    })
                    .await
                    .map_err(|_| CommandError::internal("TRAE 模型校验任务异常终止"))??;
                    if let Some(session_id) = current.active_session_id.as_deref() {
                        if previous
                            .session
                            .as_ref()
                            .is_some_and(|(id, _)| id != session_id)
                        {
                            return Err(CommandError::new(
                                "trae_active_session_changed",
                                "TRAE 当前任务在切换期间发生变化，已回滚本次切换",
                            ));
                        }
                        if current.active_selection.as_deref()
                            != Some(plan.row.display_name.as_str())
                        {
                            if previous.session.is_none() {
                                let baseline =
                                    current.active_selection.clone().ok_or_else(|| {
                                        CommandError::new(
                                            "trae_native_selection_missing",
                                            "TRAE 当前模型尚未加载完成",
                                        )
                                    })?;
                                checkpoint
                                    .additional_session_baselines
                                    .entry((*label).to_owned())
                                    .or_default()
                                    .entry(session_id.to_owned())
                                    .or_insert_with(|| baseline.clone());
                                previous.session = Some((session_id.to_owned(), baseline));
                                save_checkpoint(transaction, self.id(), &resource, &checkpoint)?;
                            }
                            let ui = Arc::clone(&self.ui);
                            let detection_for_select = detection.clone();
                            let display_name = plan.row.display_name.clone();
                            let session_id = session_id.to_owned();
                            tokio::task::spawn_blocking(move || {
                                ui.select_model_label_scoped(
                                    kind,
                                    &detection_for_select,
                                    label,
                                    &display_name,
                                    Some(&session_id),
                                )
                            })
                            .await
                            .map_err(|_| CommandError::internal("TRAE 当前任务切换异常终止"))??;
                        }
                    }
                    let ui = Arc::clone(&self.ui);
                    let detection_for_snapshot = detection.clone();
                    let final_selection = tokio::task::spawn_blocking(move || {
                        ui.snapshot_label(kind, &detection_for_snapshot, label)
                    })
                    .await
                    .map_err(|_| CommandError::internal("TRAE 模型复核异常终止"))??;
                    if final_selection.selection != plan.row.display_name {
                        #[cfg(test)]
                        eprintln!(
                            "TRAE_LIVE_MISMATCH {label} expected={} actual={}",
                            plan.row.display_name, final_selection.selection
                        );
                        return Err(CommandError::new(
                            "trae_visible_selection_mismatch",
                            "TRAE 有模式没有选中目标模型，已回滚本次切换",
                        ));
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = visible {
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

            // The controlled Trae is the final instance. Verify its durable
            // default before committing; a successful operation releases the
            // control channel without starting Trae a second time.
            let detection_for_verification = detection.clone();
            let model_id = plan.row.input.model_id.clone();
            let protocol = plan.row.input.protocol;
            let selected_session = plan.previous_session.as_ref().map(|(id, _)| id.clone());
            let additional_sessions = plan
                .additional_previous
                .iter()
                .filter_map(|(label, baseline)| {
                    baseline
                        .session
                        .as_ref()
                        .map(|(id, _)| (label.clone(), id.clone()))
                })
                .collect::<BTreeMap<_, _>>();
            let persisted = tokio::task::spawn_blocking(move || {
                verify_persisted_default_in_running_session(
                    &detection_for_verification,
                    kind,
                    &model_id,
                    protocol,
                    selected_session.as_deref(),
                    &additional_sessions,
                    was_running,
                )
            })
            .await
            .map_err(|_| CommandError::internal("Trae 重启后模型校验任务异常终止"))?;
            if let Err(error) = persisted {
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

            let ui = Arc::clone(&self.ui);
            tokio::task::spawn_blocking(move || ui.complete_operation(kind))
                .await
                .map_err(|_| CommandError::internal("Trae 原生模型服务交接任务异常终止"))??;
            Ok(ServiceConfigOutcome {
                needs_restart: false,
                message: format!("{} · {}", desired.provider_name, desired.model_id),
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
            let _session_guard = TraeOperationGuard(Arc::clone(&self.ui), self.kind);
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
            let selection_baseline = checkpoint.selection_baseline.clone();
            let session_baselines = checkpoint.session_baselines.clone();
            let additional_baselines = checkpoint.additional_baselines.clone();
            let additional_selection_baselines = checkpoint.additional_selection_baselines.clone();
            let additional_sessions = checkpoint.additional_session_baselines.clone();
            let ide_baseline = checkpoint.ide_baseline.clone();
            tokio::task::spawn_blocking(move || {
                for (session_id, selection) in session_baselines {
                    ui.select_model_scoped(kind, &detection_for_ui, &selection, Some(&session_id))?;
                }
                if let Some(storage) = selection_baseline.as_ref() {
                    ui.restore_selection_baseline(
                        kind,
                        &detection_for_ui,
                        kind.selection_label(),
                        storage,
                    )?;
                } else {
                    select_model_with_transient_retry(
                        ui.as_ref(),
                        kind,
                        &detection_for_ui,
                        &baseline,
                    )?;
                }
                for label in kind.additional_selection_labels() {
                    if *label == "ide_composer" {
                        continue;
                    }
                    let Some(selection) = additional_baselines.get(*label) else {
                        continue;
                    };
                    for (session_id, session_selection) in additional_sessions
                        .get(*label)
                        .into_iter()
                        .flat_map(|map| map.iter())
                    {
                        ui.select_model_label_scoped(
                            kind,
                            &detection_for_ui,
                            label,
                            session_selection,
                            Some(session_id),
                        )?;
                    }
                    if let Some(storage) = additional_selection_baselines.get(*label) {
                        ui.restore_selection_baseline(kind, &detection_for_ui, label, storage)?;
                    } else {
                        ui.select_model_label_scoped(
                            kind,
                            &detection_for_ui,
                            label,
                            selection,
                            None,
                        )?;
                    }
                }
                if let Some(ide_baseline) = ide_baseline.as_ref() {
                    ui.restore_ide_baseline(kind, &detection_for_ui, ide_baseline)?;
                } else if let Some(selection) = additional_baselines.get("ide_composer") {
                    ui.select_model_label_scoped(
                        kind,
                        &detection_for_ui,
                        "ide_composer",
                        selection,
                        None,
                    )?;
                }
                AppResult::Ok(())
            })
            .await
            .map_err(|_| CommandError::internal("Trae 原模型恢复任务异常终止"))??;

            if let Err(error) = commit() {
                let selection_restored = if let Some(active) = checkpoint.active.as_deref() {
                    let ui = Arc::clone(&self.ui);
                    let detection = detection.clone();
                    let active = active.to_owned();
                    tokio::task::spawn_blocking(move || {
                        select_model_with_transient_retry(ui.as_ref(), kind, &detection, &active)?;
                        for label in kind.additional_selection_labels() {
                            select_model_label_scoped_with_transient_retry(
                                ui.as_ref(),
                                kind,
                                &detection,
                                label,
                                &active,
                                None,
                            )?;
                        }
                        AppResult::Ok(())
                    })
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
            let ui = Arc::clone(&self.ui);
            tokio::task::spawn_blocking(move || ui.complete_operation(kind))
                .await
                .map_err(|_| CommandError::internal("Trae 原生模型服务交接任务异常终止"))??;
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
        // A status refresh must never launch a second Trae instance. The
        // model-list cache can lag, but the persisted recent selection is the
        // default used by a newly created task after a normal restart.
        let (mode, model_key) = ui::cached_recent_selection(
            detection.config_path.as_deref(),
            self.kind.selection_label(),
        )?
        .ok_or_else(|| {
            CommandError::new(
                "trae_binding_unverified",
                "Trae 尚未持久化新任务的默认模型选择",
            )
        })?;
        if mode != 0
            || !stable_persisted_key_matches(
                detection.config_path.as_deref(),
                self.kind.selection_label(),
                &expected.model_id,
                expected.protocol,
                &model_key,
            )?
        {
            return Err(CommandError::new(
                "trae_binding_changed",
                "Trae 新任务的默认模型与目标模型不一致",
            ));
        }
        for label in self.kind.additional_selection_labels() {
            verify_persisted_label(
                detection,
                label,
                &expected.model_id,
                expected.protocol,
                None,
            )?;
        }
        Ok(())
    }
}

fn stable_persisted_key_matches(
    path: Option<&Path>,
    label: &str,
    model_id: &str,
    protocol: ApiProtocol,
    selected_key: &str,
) -> AppResult<bool> {
    let provider = match protocol {
        ApiProtocol::OpenaiChatCompletions => "custom_openai_compatible",
        ApiProtocol::OpenaiResponses => "custom_responses_compatible",
        ApiProtocol::AnthropicMessages => "custom_anthropic_compatible",
    };
    let prefix = format!("{label}_3_{provider}_{provider}//{model_id}_");
    let Some(custom_id) = selected_key.strip_prefix(&prefix) else {
        return Ok(false);
    };
    if custom_id.is_empty() || !custom_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(false);
    }
    Ok(ui::cached_model_key_matches(path, label, model_id, selected_key)?.unwrap_or(true))
}

fn verify_persisted_default_in_running_session(
    detection: &AgentDetection,
    kind: TraeKind,
    model_id: &str,
    protocol: ApiProtocol,
    selected_session: Option<&str>,
    additional_sessions: &BTreeMap<String, String>,
    _was_running: bool,
) -> AppResult<()> {
    // The native bridge now awaits the storage flush after updating both live
    // selectors. Retry only actual durable lag; do not delay every success.
    for _retry in 0..12 {
        let primary = verify_persisted_label(
            detection,
            kind.selection_label(),
            model_id,
            protocol,
            selected_session,
        );
        let additional = kind.additional_selection_labels().iter().all(|label| {
            let verified = verify_persisted_label(
                detection,
                label,
                model_id,
                protocol,
                additional_sessions.get(*label).map(String::as_str),
            );
            #[cfg(test)]
            if _retry == 0 && verified.is_err() {
                let cached = if *label == "ide_composer" {
                    ui::cached_ide_selection(detection.config_path.as_deref())
                } else {
                    ui::cached_recent_selection(detection.config_path.as_deref(), label)
                };
                eprintln!("TRAE_LIVE_DURABLE_MISMATCH label={label} actual={cached:?}");
                if let Some(id) = additional_sessions.get(*label) {
                    let cached_session =
                        ui::cached_session_selection(detection.config_path.as_deref(), id, label);
                    eprintln!("TRAE_LIVE_DURABLE_SESSION label={label} actual={cached_session:?}");
                }
            }
            verified.is_ok()
        });
        if primary.is_ok() && additional {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(CommandError::new(
        "trae_persisted_selection_mismatch",
        "Trae 当前实例的默认模型未保持为目标模型，已回滚本次切换",
    ))
}

fn verify_persisted_label(
    detection: &AgentDetection,
    label: &str,
    model_id: &str,
    protocol: ApiProtocol,
    session_id: Option<&str>,
) -> AppResult<()> {
    if label == "ide_composer" {
        let selection = ui::cached_ide_selection(detection.config_path.as_deref())?;
        let matches = selection.is_some_and(|(mode, key)| {
            mode == 0
                && stable_persisted_key_matches(
                    detection.config_path.as_deref(),
                    "solo_coder",
                    model_id,
                    protocol,
                    &format!("solo_coder_{key}"),
                )
                .unwrap_or(false)
        });
        return if matches {
            Ok(())
        } else {
            Err(CommandError::new(
                "trae_persisted_selection_mismatch",
                "TRAE IDE 模型选择未保持为目标模型",
            ))
        };
    }
    let selection = ui::cached_recent_selection(detection.config_path.as_deref(), label)?;
    let matches = |selection: Option<(i64, String)>| {
        selection.is_some_and(|(mode, key)| {
            mode == 0
                && stable_persisted_key_matches(
                    detection.config_path.as_deref(),
                    label,
                    model_id,
                    protocol,
                    &key,
                )
                .unwrap_or(false)
        })
    };
    if !matches(selection)
        || session_id.is_some_and(|id| {
            !ui::cached_session_selection(detection.config_path.as_deref(), id, label)
                .is_ok_and(matches)
        })
    {
        return Err(CommandError::new(
            "trae_persisted_selection_mismatch",
            "Trae 模型选择未保持为目标模型",
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct UiApplyPlan {
    row: ManagedRow,
    previous_selection: String,
    previous_storage: Option<ui::SelectionStorageBaseline>,
    previous_session: Option<(String, String)>,
    additional_previous: BTreeMap<String, LabelBaseline>,
    created: bool,
    borrowed: bool,
    replaced_row: Option<ManagedRow>,
}

#[derive(Clone)]
struct LabelBaseline {
    selection: String,
    storage: Option<ui::SelectionStorageBaseline>,
    session: Option<(String, String)>,
    ide: Option<ui::IdeSelectionBaseline>,
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
    let result = plan_ui_change_inner(
        ui,
        kind,
        detection,
        input,
        matching,
        replacement,
        known_names,
    );
    if result.is_err() {
        // Planning has not changed any target selection or custom model. A
        // transient catalogue/read failure must not discard a healthy pipe
        // owner and force another restart on the next attempt. Retain is an
        // acknowledged native RPC; a broken controller still fails closed.
        let _ = ui.complete_operation(kind);
    }
    result
}

fn plan_ui_change_inner(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    input: &ManagedInput,
    matching: Option<&ManagedRow>,
    replacement: Option<&ManagedRow>,
    known_names: &HashSet<String>,
) -> AppResult<UiApplyPlan> {
    let snapshot = ui.snapshot_interactive(kind, detection)?;
    let mut additional_previous = BTreeMap::new();
    for label in kind.additional_selection_labels() {
        let selection = ui.snapshot_label(kind, detection, label)?;
        additional_previous.insert(
            (*label).to_owned(),
            LabelBaseline {
                selection: selection.recent_selection,
                storage: selection.selection_baseline,
                session: selection.active_session_id.zip(selection.active_selection),
                ide: selection.ide_baseline,
            },
        );
    }
    if let Some(row) = matching {
        if snapshot.custom_models.contains(&row.display_name) {
            ensure_existing_endpoint(&snapshot, &row.display_name, input)?;
            return Ok(UiApplyPlan {
                row: row.clone(),
                previous_selection: snapshot.recent_selection.clone(),
                previous_storage: snapshot.selection_baseline.clone(),
                previous_session: snapshot
                    .active_session_id
                    .clone()
                    .zip(snapshot.active_selection.clone()),
                additional_previous,
                created: false,
                borrowed: false,
                replaced_row: None,
            });
        }
    }

    let replaced_row = replacement
        .filter(|row| snapshot.custom_models.contains(&row.display_name))
        .cloned();
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
            ensure_existing_endpoint(&snapshot, &display_name, input)?;
            return Ok(UiApplyPlan {
                row: ManagedRow {
                    display_name,
                    input: input.clone(),
                },
                previous_selection: snapshot.recent_selection.clone(),
                previous_storage: snapshot.selection_baseline.clone(),
                previous_session: snapshot
                    .active_session_id
                    .clone()
                    .zip(snapshot.active_selection.clone()),
                additional_previous,
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
        previous_selection: snapshot.recent_selection.clone(),
        previous_storage: snapshot.selection_baseline.clone(),
        previous_session: snapshot
            .active_session_id
            .clone()
            .zip(snapshot.active_selection.clone()),
        additional_previous,
        created: true,
        borrowed: false,
        replaced_row,
    })
}

fn ensure_existing_endpoint(
    snapshot: &ui::TraeUiSnapshot,
    display_name: &str,
    input: &ManagedInput,
) -> AppResult<()> {
    let Some(endpoints) = snapshot.custom_endpoints_by_name.get(display_name) else {
        return Err(CommandError::new(
            "trae_existing_model_unverified",
            "无法核对 Trae 已有模型的服务地址",
        )
        .with_recovery("请核对 Trae 中已有模型的服务地址；未验证前不会改动原配置。"));
    };
    let expected_endpoint = endpoint_url(&input.base_url, ui::endpoint_path(input.protocol))?;
    if endpoints.is_empty()
        || !endpoints.iter().all(|endpoint| {
            endpoint.trim_end_matches('/') == input.base_url.trim_end_matches('/')
                || endpoint.trim_end_matches('/') == expected_endpoint.as_str()
        })
    {
        return Err(CommandError::new(
            "trae_existing_model_conflict",
            "Trae 已有同模型 ID，但服务地址与当前供应商不一致",
        )
        .with_recovery("请核对 Trae 中已有模型与 AT-Switch 供应商的服务地址；不会改动原配置。"));
    }
    Ok(())
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
            // Trae persists custom models asynchronously. A timed-out native
            // acknowledgement can still leave the exact model durable; re-read
            // before deciding whether to roll back.
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
    let mut result = select_model_scoped_with_transient_retry(
        ui,
        kind,
        detection,
        &plan.row.display_name,
        plan.previous_session.as_ref().map(|(id, _)| id.as_str()),
    );
    if result.is_err() {
        let selected = ui
            .snapshot_interactive(kind, detection)
            .is_ok_and(|snapshot| {
                snapshot.recent_selection == plan.row.display_name
                    && plan.previous_session.as_ref().is_none_or(|(id, _)| {
                        snapshot.active_session_id.as_deref() == Some(id)
                            && snapshot.active_selection.as_deref()
                                == Some(plan.row.display_name.as_str())
                    })
            });
        if selected {
            result = Ok(());
        }
    }
    if result.is_ok() {
        for label in kind.additional_selection_labels() {
            let session_id = plan
                .additional_previous
                .get(*label)
                .and_then(|baseline| baseline.session.as_ref())
                .map(|(id, _)| id.as_str());
            result = select_model_label_scoped_with_transient_retry(
                ui,
                kind,
                detection,
                label,
                &plan.row.display_name,
                session_id,
            );
            if result.is_err() {
                break;
            }
        }
    }
    // select_model verifies Trae's persisted selection on both platforms.
    // Do not gate it again on Trae's asynchronously refreshed model cache.
    UiApplyAttempt { result, mutation }
}

fn select_model_with_transient_retry(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<()> {
    match ui.select_model(kind, detection, display_name) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.code.as_str(),
                "trae_native_model_missing"
                    | "trae_native_selection_unverified"
                    | "trae_native_selection_mismatch"
            ) =>
        {
            // Trae may refresh the native model catalog or selection storage
            // asynchronously. Re-read before one idempotent retry.
            if ui
                .snapshot_interactive(kind, detection)
                .is_ok_and(|snapshot| snapshot.selection == display_name)
            {
                return Ok(());
            }
            ui.select_model(kind, detection, display_name)
        }
        Err(error) => Err(error),
    }
}

fn select_model_scoped_with_transient_retry(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
    session_id: Option<&str>,
) -> AppResult<()> {
    match ui.select_model_scoped(kind, detection, display_name, session_id) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.code.as_str(),
                "trae_native_model_missing"
                    | "trae_native_selection_unverified"
                    | "trae_native_selection_mismatch"
            ) =>
        {
            ui.select_model_scoped(kind, detection, display_name, session_id)
        }
        Err(error) => Err(error),
    }
}

fn select_model_label_scoped_with_transient_retry(
    ui: &dyn ui::TraeUi,
    kind: TraeKind,
    detection: &AgentDetection,
    label: &str,
    display_name: &str,
    session_id: Option<&str>,
) -> AppResult<()> {
    match ui.select_model_label_scoped(kind, detection, label, display_name, session_id) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.code.as_str(),
                "trae_native_model_missing"
                    | "trae_native_selection_unverified"
                    | "trae_native_selection_mismatch"
            ) =>
        {
            ui.select_model_label_scoped(kind, detection, label, display_name, session_id)
        }
        Err(error) => Err(error),
    }
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
        let model_removed_early = !mutation.model_creation_attempted
            || model_absent_after_failed_creation
            || (mutation.replacement_delete_attempted
                && ui
                    .delete_model(kind, &detection, &plan.row.display_name)
                    .is_ok());
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
                    || select_model_with_transient_retry(
                        ui.as_ref(),
                        kind,
                        &detection,
                        &replaced.display_name,
                    )
                    .is_ok()
            });
        let session_restored = plan
            .previous_session
            .as_ref()
            .is_none_or(|(id, selection)| {
                ui.select_model_scoped(kind, &detection, selection, Some(id))
                    .is_ok()
            });
        // all() short-circuits and would skip remaining rollback mutations.
        #[allow(clippy::unnecessary_fold)]
        let additional_restored = plan
            .additional_previous
            .iter()
            .filter(|(label, _)| label.as_str() != "ide_composer")
            .map(|(label, baseline)| {
                let session_restored = baseline.session.as_ref().is_none_or(|(id, selection)| {
                    ui.select_model_label_scoped(kind, &detection, label, selection, Some(id))
                        .is_ok()
                });
                let default_restored = baseline.storage.as_ref().map_or_else(
                    || {
                        ui.select_model_label_scoped(
                            kind,
                            &detection,
                            label,
                            &baseline.selection,
                            None,
                        )
                        .is_ok()
                    },
                    |storage| {
                        ui.restore_selection_baseline(kind, &detection, label, storage)
                            .is_ok()
                    },
                );
                session_restored && default_restored
            })
            .fold(true, |restored, next| restored && next);
        // Always restore the primary selector, even when another entry failed.
        // Boolean short-circuiting must not leave a deleted QA/managed model active.
        let primary_restored = plan.previous_storage.as_ref().map_or_else(
            || {
                select_model_with_transient_retry(
                    ui.as_ref(),
                    kind,
                    &detection,
                    &plan.previous_selection,
                )
                .is_ok()
            },
            |storage| {
                ui.restore_selection_baseline(kind, &detection, kind.selection_label(), storage)
                    .is_ok()
            },
        );
        let native_restored =
            replaced_row_restored && session_restored && additional_restored && primary_restored;
        let ide_restored = plan
            .additional_previous
            .get("ide_composer")
            .is_none_or(|baseline| {
                baseline.ide.as_ref().map_or_else(
                    || {
                        ui.select_model_label_scoped(
                            kind,
                            &detection,
                            "ide_composer",
                            &baseline.selection,
                            None,
                        )
                        .is_ok()
                    },
                    |state| ui.restore_ide_baseline(kind, &detection, state).is_ok(),
                )
            });
        let selection_restored = native_restored && ide_restored;
        // Restore every selector before removing a newly created model. A
        // catalog refresh cannot preserve a selection whose model was just
        // deleted, and may otherwise persist a dangling current-session key.
        // Replacements are the exception: free their shared name first so
        // the original row can be registered again.
        let model_removed = model_removed_early
            || (!mutation.replacement_delete_attempted
                && ui
                    .delete_model(kind, &detection, &plan.row.display_name)
                    .is_ok());
        if selection_restored && model_removed && replaced_row_restored {
            // A verified compensation is safe to retain in the same process.
            // Dropping a cold bridge here would close/reopen TRAE yet again.
            let _ = ui.complete_operation(kind);
        }
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
    #[serde(default)]
    selection_baseline: Option<ui::SelectionStorageBaseline>,
    #[serde(default)]
    session_baselines: BTreeMap<String, String>,
    #[serde(default)]
    additional_baselines: BTreeMap<String, String>,
    #[serde(default)]
    additional_selection_baselines: BTreeMap<String, ui::SelectionStorageBaseline>,
    #[serde(default)]
    additional_session_baselines: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    ide_baseline: Option<ui::IdeSelectionBaseline>,
    #[serde(default)]
    selection_label: Option<String>,
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
    if checkpoint.version != CHECKPOINT_VERSION
        || checkpoint.account_scope != resource
        || checkpoint.selection_label.as_deref() != Some(TraeKind::Unified.selection_label())
    {
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
        "trae_native_service_required",
        format!("{display_name} 模型配置必须通过原生模型服务完成"),
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
    use rusqlite::OptionalExtension;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn native_service_rejects_unverified_trae_versions() {
        assert!(!TraeKind::Unified.supports_native_bridge(Some("3.4.1")));
        assert!(!TraeKind::Unified.supports_native_bridge(Some("0.1.69")));
        assert!(TraeKind::Unified.supports_native_bridge(Some("3.4.2")));
        assert!(TraeKind::Unified.supports_native_bridge(Some("3.4.2.0")));
        assert!(!TraeKind::Unified.supports_native_bridge(Some("3.4.3")));
        assert!(!TraeKind::Unified.supports_native_bridge(Some("0.1.70")));
        assert!(!TraeKind::Unified.supports_native_bridge(None));
        assert_eq!(TraeKind::Unified.selection_label(), "solo_agent_lite");
        assert_eq!(TraeKind::Unified.selection_label(), "solo_agent_lite");
    }

    #[test]
    fn unified_trae_does_not_import_retired_product_checkpoints() {
        let temp = tempfile::tempdir().expect("temp");
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        for id in ["traecode", "traework"] {
            transaction
                .save_service_checkpoint(id, &format!("{id}:12345"), b"old-format-not-imported")
                .expect("old journal");
        }
        assert!(load_checkpoint(&transaction, "trae", "trae:12345")
            .expect("new journal namespace")
            .is_none());
    }

    #[test]
    fn trae_plan_keeps_distinct_default_and_current_task_baselines() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = FakeUi::new("Auto Mode");
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.recent_selection = Some("Auto Mode".to_owned());
            state.active_session = Some(("session-one".to_owned(), "User model".to_owned()));
        }
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let plan = plan_ui_change(
            &fake,
            TraeKind::Unified,
            &detection,
            &input,
            None,
            None,
            &HashSet::new(),
        )
        .expect("plan TRAE model change");
        assert_eq!(plan.previous_selection, "Auto Mode");
        assert_eq!(
            plan.previous_session,
            Some(("session-one".to_owned(), "User model".to_owned()))
        );
    }

    #[tokio::test]
    async fn delayed_trae_task_route_is_selected_and_restored() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        fake.state
            .lock()
            .expect("fake UI state")
            .reveal_session_at_snapshot = Some((3, "late-task".to_owned(), "Auto Mode".to_owned()));
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
            .expect("late task selected");
        {
            let state = fake.state.lock().expect("fake UI state");
            assert_eq!(
                state.active_session,
                Some((
                    "late-task".to_owned(),
                    "Fictional Provider · fictional-model".to_owned()
                ))
            );
        }
        adapter
            .restore(&detection, &transaction, &|| Ok(()))
            .await
            .expect("late task restored");
        assert_eq!(
            fake.state.lock().expect("fake UI state").active_session,
            Some(("late-task".to_owned(), "Auto Mode".to_owned()))
        );
    }

    #[test]
    fn failed_read_only_plan_retains_runtime_without_mutating_selections() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = FakeUi::new("User model");
        fake.state.lock().expect("fake UI state").fail_next_snapshot = true;
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let result = plan_ui_change(
            &fake,
            TraeKind::Unified,
            &detection,
            &input,
            None,
            None,
            &HashSet::new(),
        );
        assert!(result.is_err());
        {
            let state = fake.state.lock().expect("fake UI state");
            assert_eq!(state.selection, "User model");
            assert_eq!(state.completions, 1);
            assert_eq!(
                state.additions + state.deletions + state.selection_attempts,
                0
            );
        }
        plan_ui_change(
            &fake,
            TraeKind::Unified,
            &detection,
            &input,
            None,
            None,
            &HashSet::new(),
        )
        .expect("read-only retry succeeds");
        assert_eq!(fake.state.lock().expect("fake UI state").completions, 1);
    }

    #[derive(Default)]
    struct FakeUiState {
        selection: String,
        recent_selection: Option<String>,
        active_session: Option<(String, String)>,
        secondary_selection: String,
        ide_selection: String,
        secondary_active_session: Option<(String, String)>,
        coder_selection: String,
        coder_active_session: Option<(String, String)>,
        reveal_session_at_snapshot: Option<(usize, String, String)>,
        models: HashSet<String>,
        model_ids: HashMap<String, String>,
        endpoints: HashMap<String, String>,
        cache_visible: bool,
        fail_next_snapshot: bool,
        fail_after_persisting_next_add: bool,
        transient_selection_failures: usize,
        selection_attempts: usize,
        snapshots: usize,
        additions: usize,
        deletions: usize,
        selections_at_delete: Vec<[String; 4]>,
        completions: usize,
    }

    struct FakeUi {
        state: Mutex<FakeUiState>,
    }

    impl FakeUi {
        fn new(selection: &str) -> Self {
            Self {
                state: Mutex::new(FakeUiState {
                    selection: selection.to_owned(),
                    secondary_selection: "Auto Mode".to_owned(),
                    ide_selection: "Auto Mode".to_owned(),
                    coder_selection: "Auto Mode".to_owned(),
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
                selection: state
                    .active_session
                    .as_ref()
                    .map(|(_, selected)| selected.clone())
                    .unwrap_or_else(|| state.selection.clone()),
                recent_selection: state
                    .recent_selection
                    .clone()
                    .unwrap_or_else(|| state.selection.clone()),
                active_session_id: state.active_session.as_ref().map(|(id, _)| id.clone()),
                active_selection: state
                    .active_session
                    .as_ref()
                    .map(|(_, selected)| selected.clone()),
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
                custom_endpoints_by_name: if state.cache_visible {
                    state
                        .endpoints
                        .iter()
                        .map(|(name, endpoint)| {
                            (
                                name.clone(),
                                std::collections::BTreeSet::from([endpoint.clone()]),
                            )
                        })
                        .collect()
                } else {
                    Default::default()
                },
                ide_baseline: None,
                selection_baseline: None,
            }
        }
    }

    impl ui::TraeUi for FakeUi {
        fn snapshot_interactive(
            &self,
            _: TraeKind,
            _: &AgentDetection,
        ) -> AppResult<ui::TraeUiSnapshot> {
            let mut state = self.state.lock().expect("fake UI state");
            state.snapshots += 1;
            if state.fail_next_snapshot {
                state.fail_next_snapshot = false;
                return Err(CommandError::new(
                    "trae_native_model_list_failed",
                    "test catalogue read failed",
                ));
            }
            if state
                .reveal_session_at_snapshot
                .as_ref()
                .is_some_and(|(threshold, _, _)| state.snapshots >= *threshold)
            {
                if let Some((_, id, selection)) = state.reveal_session_at_snapshot.take() {
                    state.active_session = Some((id, selection));
                }
            }
            drop(state);
            Ok(self.snapshot_value())
        }

        fn snapshot_label(
            &self,
            kind: TraeKind,
            _: &AgentDetection,
            label: &str,
        ) -> AppResult<ui::TraeUiSnapshot> {
            assert!(kind.additional_selection_labels().contains(&label));
            let mut snapshot = self.snapshot_value();
            let state = self.state.lock().expect("fake UI state");
            let (selection, active) = if label == "ide_composer" {
                (&state.ide_selection, &None)
            } else if label == "solo_coder" {
                (&state.coder_selection, &state.coder_active_session)
            } else {
                (&state.secondary_selection, &state.secondary_active_session)
            };
            snapshot.recent_selection = selection.clone();
            snapshot.active_session_id = active.as_ref().map(|(id, _)| id.clone());
            snapshot.active_selection = active.as_ref().map(|(_, selected)| selected.clone());
            snapshot.selection = snapshot
                .active_selection
                .clone()
                .unwrap_or_else(|| selection.clone());
            Ok(snapshot)
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
            state.endpoints.insert(
                input.display_name.clone(),
                endpoint_url(&input.base_url, ui::endpoint_path(input.protocol))?.to_string(),
            );
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
            kind: TraeKind,
            detection: &AgentDetection,
            display_name: &str,
        ) -> AppResult<()> {
            let mut state = self.state.lock().expect("fake UI state");
            state.selection_attempts += 1;
            if state.transient_selection_failures > 0 {
                state.transient_selection_failures -= 1;
                return Err(CommandError::new(
                    "trae_native_selection_unverified",
                    "test transient model selection failure",
                ));
            }
            if display_name == "Auto Mode" || state.models.contains(display_name) {
                state.selection = display_name.to_owned();
                let mode = if display_name == "Auto Mode" { 1 } else { 0 };
                let model_id = state
                    .model_ids
                    .get(display_name)
                    .cloned()
                    .unwrap_or_default();
                let selection_key = if mode == 0 {
                    format!(
                        "{}_3_custom_responses_compatible_custom_responses_compatible//{model_id}_123",
                        kind.selection_label()
                    )
                } else {
                    String::new()
                };
                if let Some(path) = detection.config_path.as_deref() {
                    let connection = Connection::open(path).expect("fake state database");
                    if mode == 0 {
                        let catalog_key = "12345:AI.agent.model.model_list_map";
                        let raw: String = connection
                            .query_row(
                                "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                                [catalog_key],
                                |row| row.get(0),
                            )
                            .expect("fake catalog");
                        let mut catalog: serde_json::Value =
                            serde_json::from_str(&raw).expect("fake catalog JSON");
                        if !catalog[kind.selection_label()].is_array() {
                            catalog[kind.selection_label()] = serde_json::json!([]);
                        }
                        let models = catalog[kind.selection_label()]
                            .as_array_mut()
                            .expect("fake catalog label");
                        models.retain(|model| {
                            model
                                .get("display_name")
                                .and_then(serde_json::Value::as_str)
                                != Some(display_name)
                        });
                        models.push(serde_json::json!({
                            "config_source": 3,
                            "provider": "custom_responses_compatible",
                            "name": format!("custom_responses_compatible//{model_id}"),
                            "custom_model_id": 123,
                            "display_name": display_name,
                        }));
                        connection
                            .execute(
                                "UPDATE ItemTable SET value = ?2 WHERE key = ?1",
                                [catalog_key, catalog.to_string().as_str()],
                            )
                            .expect("fake catalog update");
                    }
                    let storage_key = "12345:AI.agent.model.recent_user_selection_by_agent_label";
                    let mut selected = connection
                        .query_row(
                            "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                            [storage_key],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                        .unwrap_or_else(|| serde_json::json!({}));
                    selected[kind.selection_label()] =
                        serde_json::json!({"mode": mode, "modelId": selection_key});
                    let serialized = selected.to_string();
                    connection
                        .execute("DELETE FROM ItemTable WHERE key = ?1", [storage_key])
                        .expect("remove prior fake selection");
                    connection
                        .execute(
                            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                            [storage_key, serialized.as_str()],
                        )
                        .expect("fake persisted selection");
                }
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
            let selected = [
                state.selection.clone(),
                state.secondary_selection.clone(),
                state.coder_selection.clone(),
                state.ide_selection.clone(),
            ];
            state.selections_at_delete.push(selected);
            if state.models.remove(display_name) {
                state.deletions += 1;
            }
            state.model_ids.remove(display_name);
            state.endpoints.remove(display_name);
            Ok(())
        }

        fn complete_operation(&self, _: TraeKind) -> AppResult<()> {
            self.state.lock().expect("fake UI state").completions += 1;
            Ok(())
        }

        fn select_model_scoped(
            &self,
            kind: TraeKind,
            detection: &AgentDetection,
            display_name: &str,
            session_id: Option<&str>,
        ) -> AppResult<()> {
            self.select_model(kind, detection, display_name)?;
            let Some(session_id) = session_id else {
                return Ok(());
            };
            let mut state = self.state.lock().expect("fake UI state");
            state.active_session = Some((session_id.to_owned(), display_name.to_owned()));
            let mode = if display_name == "Auto Mode" { 1 } else { 0 };
            let model_id = state
                .model_ids
                .get(display_name)
                .cloned()
                .unwrap_or_default();
            drop(state);
            let selection_key = if mode == 0 {
                format!(
                    "{}_3_custom_responses_compatible_custom_responses_compatible//{model_id}_123",
                    kind.selection_label()
                )
            } else {
                String::new()
            };
            if let Some(path) = detection.config_path.as_deref() {
                let connection = Connection::open(path).expect("fake state database");
                let storage_key = "12345:AI.agent.model.session_selected_model";
                let mut selected = connection
                    .query_row(
                        "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                        [storage_key],
                        |row| row.get::<_, String>(0),
                    )
                    .ok()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                selected[session_id][kind.selection_label()] =
                    serde_json::json!({"mode": mode, "modelId": selection_key});
                connection
                    .execute("DELETE FROM ItemTable WHERE key = ?1", [storage_key])
                    .expect("remove prior fake session selection");
                connection
                    .execute(
                        "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                        [storage_key, selected.to_string().as_str()],
                    )
                    .expect("fake persisted session selection");
            }
            Ok(())
        }

        fn select_model_label_scoped(
            &self,
            kind: TraeKind,
            detection: &AgentDetection,
            label: &str,
            display_name: &str,
            session_id: Option<&str>,
        ) -> AppResult<()> {
            assert!(kind.additional_selection_labels().contains(&label));
            let mut state = self.state.lock().expect("fake UI state");
            if display_name != "Auto Mode" && !state.models.contains(display_name) {
                return Err(CommandError::new(
                    "test_model_missing",
                    "test model missing",
                ));
            }
            let model_id = state
                .model_ids
                .get(display_name)
                .cloned()
                .unwrap_or_default();
            let mode = if display_name == "Auto Mode" { 1 } else { 0 };
            if label == "ide_composer" {
                state.ide_selection = display_name.to_owned();
                drop(state);
                if let Some(path) = detection.config_path.as_deref() {
                    let connection = Connection::open(path).expect("fake state database");
                    let model_key = if mode == 0 {
                        format!(
                            "3_custom_responses_compatible_custom_responses_compatible//{model_id}_123"
                        )
                    } else {
                        String::new()
                    };
                    for (suffix, value) in [
                        (
                            "globalModelMap",
                            serde_json::json!({"solo_coder": model_key}),
                        ),
                        ("globalModeMap", serde_json::json!({"solo_coder": mode})),
                    ] {
                        let key = format!("12345_ai-chat:sessionRelation:{suffix}");
                        connection
                            .execute("DELETE FROM ItemTable WHERE key = ?1", [&key])
                            .expect("remove fake IDE state");
                        connection
                            .execute(
                                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                                [&key, &value.to_string()],
                            )
                            .expect("fake IDE state");
                    }
                }
                return Ok(());
            }
            let model_key = if mode == 0 {
                format!(
                    "{label}_3_custom_responses_compatible_custom_responses_compatible//{model_id}_123"
                )
            } else {
                String::new()
            };
            if label == "solo_coder" {
                if let Some(id) = session_id {
                    state.coder_active_session = Some((id.to_owned(), display_name.to_owned()));
                }
                state.coder_selection = display_name.to_owned();
            } else {
                if let Some(id) = session_id {
                    state.secondary_active_session = Some((id.to_owned(), display_name.to_owned()));
                }
                state.secondary_selection = display_name.to_owned();
            }
            drop(state);
            if let Some(path) = detection.config_path.as_deref() {
                let connection = Connection::open(path).expect("fake state database");
                if mode == 0 {
                    let catalog_key = "12345:AI.agent.model.model_list_map";
                    let raw: String = connection
                        .query_row(
                            "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                            [catalog_key],
                            |row| row.get(0),
                        )
                        .expect("fake catalog");
                    let mut catalog: serde_json::Value =
                        serde_json::from_str(&raw).expect("fake catalog JSON");
                    if !catalog[label].is_array() {
                        catalog[label] = serde_json::json!([]);
                    }
                    let models = catalog[label].as_array_mut().expect("fake catalog label");
                    models.retain(|model| {
                        model
                            .get("display_name")
                            .and_then(serde_json::Value::as_str)
                            != Some(display_name)
                    });
                    models.push(serde_json::json!({
                        "config_source": 3,
                        "provider": "custom_responses_compatible",
                        "name": format!("custom_responses_compatible//{model_id}"),
                        "custom_model_id": 123,
                        "display_name": display_name,
                    }));
                    connection
                        .execute(
                            "UPDATE ItemTable SET value = ?2 WHERE key = ?1",
                            [catalog_key, catalog.to_string().as_str()],
                        )
                        .expect("fake catalog update");
                }
                for (storage_key, target_session) in [
                    (
                        "12345:AI.agent.model.recent_user_selection_by_agent_label",
                        None,
                    ),
                    ("12345:AI.agent.model.session_selected_model", session_id),
                ] {
                    if storage_key.ends_with("session_selected_model") && target_session.is_none() {
                        continue;
                    }
                    let mut selected = connection
                        .query_row(
                            "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
                            [storage_key],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                        .unwrap_or_else(|| serde_json::json!({}));
                    let value = serde_json::json!({"mode": mode, "modelId": model_key});
                    if let Some(id) = target_session {
                        selected[id][label] = value;
                    } else {
                        selected[label] = value;
                    }
                    connection
                        .execute("DELETE FROM ItemTable WHERE key = ?1", [storage_key])
                        .expect("remove fake selection");
                    connection
                        .execute(
                            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                            [storage_key, selected.to_string().as_str()],
                        )
                        .expect("fake persisted selection");
                }
            }
            Ok(())
        }

        fn restore_ide_baseline(
            &self,
            kind: TraeKind,
            detection: &AgentDetection,
            baseline: &ui::IdeSelectionBaseline,
        ) -> AppResult<()> {
            let selected = baseline.workspace.mode.or(baseline.app.mode).unwrap_or(1);
            if selected == 1 {
                self.select_model_label_scoped(kind, detection, "ide_composer", "Auto Mode", None)
            } else {
                Ok(())
            }
        }

        fn restore_selection_baseline(
            &self,
            _: TraeKind,
            _: &AgentDetection,
            _: &str,
            _: &ui::SelectionStorageBaseline,
        ) -> AppResult<()> {
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
        let mut detection = AgentDetection::manual("trae", "TRAE", None, "test detection");
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
    fn transient_model_selection_is_retried_once_without_recreating_a_model() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = FakeUi::new("Auto Mode");
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state
                .models
                .insert("Fictional Provider · fictional-model".into());
            state.transient_selection_failures = 1;
        }

        select_model_with_transient_retry(
            &fake,
            TraeKind::Unified,
            &detection,
            "Fictional Provider · fictional-model",
        )
        .expect("transient selection recovers");

        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Fictional Provider · fictional-model");
        assert_eq!(state.selection_attempts, 2);
        assert_eq!(state.additions, 0);
    }

    #[test]
    fn missing_model_is_not_retried_as_a_transient_selection_failure() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = FakeUi::new("Auto Mode");

        let error = select_model_with_transient_retry(
            &fake,
            TraeKind::Unified,
            &detection,
            "Missing model",
        )
        .expect_err("permanent error must surface");

        assert_eq!(error.code, "test_model_missing");
        assert_eq!(
            fake.state.lock().expect("fake UI state").selection_attempts,
            1
        );
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
        assert!(!first.starts_with("AT-Switch · "));
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
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(orphan.clone());
            state.endpoints.insert(
                orphan.clone(),
                endpoint_url(&input.base_url, ui::endpoint_path(input.protocol))
                    .expect("test endpoint")
                    .to_string(),
            );
        }

        let plan = plan_ui_change(
            &fake,
            TraeKind::Unified,
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
        let adapter = TraeAdapter::unified();
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
            assert!(!state.selection.starts_with("AT-Switch · "));
            assert_eq!(state.models.len(), 2);
        }
        let checkpoint = transaction
            .read_service_checkpoint("trae", "trae:12345")
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
    async fn trae_switch_selects_and_restores_agent_and_ide_with_one_checkpoint() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.active_session = Some(("task-one".to_owned(), "Auto Mode".to_owned()));
            state.secondary_active_session = Some(("task-one".to_owned(), "Auto Mode".to_owned()));
            state.coder_active_session = Some(("task-one".to_owned(), "Auto Mode".to_owned()));
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        let desired = desired("fictional-secret");

        let outcome = adapter
            .apply(&detection, &desired, &transaction, &|| Ok(()))
            .await
            .expect("both Code modes selected");
        assert_eq!(outcome.message, "Fictional Provider · fictional-model");
        adapter
            .verify_cached(&detection, &desired, &transaction)
            .expect("both defaults survive restart");
        {
            let state = fake.state.lock().expect("fake UI state");
            let target = "Fictional Provider · fictional-model";
            assert_eq!(state.selection, target);
            assert_eq!(state.secondary_selection, target);
            assert_eq!(state.coder_selection, target);
            assert_eq!(state.ide_selection, target);
            assert_eq!(
                state
                    .active_session
                    .as_ref()
                    .map(|(_, value)| value.as_str()),
                Some(target)
            );
            assert_eq!(
                state
                    .secondary_active_session
                    .as_ref()
                    .map(|(_, value)| value.as_str()),
                Some(target)
            );
            assert_eq!(
                state
                    .coder_active_session
                    .as_ref()
                    .map(|(_, value)| value.as_str()),
                Some(target)
            );
        }
        ui::TraeUi::select_model_label_scoped(
            fake.as_ref(),
            TraeKind::Unified,
            &detection,
            "solo_coder",
            "Auto Mode",
            None,
        )
        .expect("simulate IDE drifting to Auto");
        assert_eq!(
            adapter
                .verify_cached(&detection, &desired, &transaction)
                .expect_err("one-mode success must be rejected")
                .code,
            "trae_persisted_selection_mismatch"
        );

        adapter
            .restore(&detection, &transaction, &|| Ok(()))
            .await
            .expect("both Code modes restored");
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Auto Mode");
        assert_eq!(state.secondary_selection, "Auto Mode");
        assert_eq!(state.coder_selection, "Auto Mode");
        assert_eq!(state.ide_selection, "Auto Mode");
        assert_eq!(
            state
                .active_session
                .as_ref()
                .map(|(_, value)| value.as_str()),
            Some("Auto Mode")
        );
        assert_eq!(
            state
                .coder_active_session
                .as_ref()
                .map(|(_, value)| value.as_str()),
            Some("Auto Mode")
        );
        assert_eq!(
            state
                .secondary_active_session
                .as_ref()
                .map(|(_, value)| value.as_str()),
            Some("Auto Mode")
        );
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
            state.endpoints.insert(
                display_name.clone(),
                endpoint_url(desired.base_url, ui::endpoint_path(desired.source_protocol))
                    .expect("test endpoint")
                    .to_string(),
            );
        }
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
        let checkpoint = load_checkpoint(&transaction, "trae", "trae:12345")
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
        let checkpoint = load_checkpoint(&transaction, "trae", "trae:12345")
            .expect("checkpoint read")
            .expect("checkpoint");
        assert!(checkpoint.borrowed.is_empty());
        assert!(checkpoint.active.is_none());
        assert!(!checkpoint.pending);
    }

    #[test]
    fn existing_model_with_another_endpoint_is_not_misreported_as_the_requested_provider() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let display_name = "User configured fictional model".to_owned();
        let fake = FakeUi::new("Auto Mode");
        {
            let mut state = fake.state.lock().expect("fake UI state");
            state.models.insert(display_name.clone());
            state
                .model_ids
                .insert(display_name.clone(), input.model_id.clone());
            state.endpoints.insert(
                display_name.clone(),
                "https://another-provider.example.test/v1/responses".to_owned(),
            );
        }
        {
            let kind = TraeKind::Unified;
            let error =
                plan_ui_change(&fake, kind, &detection, &input, None, None, &HashSet::new())
                    .err()
                    .expect("conflicting existing model must not be borrowed");
            assert_eq!(error.code, "trae_existing_model_conflict");
        }
        fake.state.lock().expect("fake UI state").endpoints.clear();
        {
            let kind = TraeKind::Unified;
            let error =
                plan_ui_change(&fake, kind, &detection, &input, None, None, &HashSet::new())
                    .err()
                    .expect("unverified existing model must not be borrowed");
            assert_eq!(error.code, "trae_existing_model_unverified");
        }
        let state = fake.state.lock().expect("fake UI state");
        assert_eq!(state.selection, "Auto Mode");
        assert_eq!(state.models, HashSet::from([display_name]));
        assert_eq!(state.additions, 0);
        assert_eq!(state.deletions, 0);
    }

    #[tokio::test]
    async fn verified_selector_change_is_not_rejected_by_a_lagging_model_cache() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        fake.state.lock().expect("fake UI state").cache_visible = false;
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
    async fn cached_verification_never_restarts_trae_and_checks_default_selection() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let fake = Arc::new(FakeUi::new("Auto Mode"));
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
            .expect("native-service apply");
        let database = detection.config_path.as_ref().expect("state database");
        let connection = Connection::open(database).expect("state database");
        let snapshots = fake.state.lock().expect("fake UI state").snapshots;
        adapter
            .verify_cached(&detection, &desired("fictional-secret"), &transaction)
            .expect("canonical persisted selection confirms the binding");
        connection
            .execute(
                "UPDATE ItemTable SET value = ?2 WHERE key = ?1",
                [
                    "12345:AI.agent.model.recent_user_selection_by_agent_label",
                    r#"{"solo_agent_lite":{"mode":0,"modelId":"solo_agent_lite_3_custom_responses_compatible_fictional-model_123"}}"#,
                ],
            )
            .expect("persisted selection");
        assert_eq!(
            adapter
                .verify_cached(&detection, &desired("fictional-secret"), &transaction)
                .expect_err("a transient model key must not be reported as active")
                .code,
            "trae_binding_changed"
        );
        assert_eq!(
            fake.state.lock().expect("fake UI state").snapshots,
            snapshots
        );

        connection
            .execute(
                "UPDATE ItemTable SET value = ?1 WHERE key LIKE '%AI.agent.model.recent_user_selection_by_agent_label'",
                [r#"{"solo_agent_lite":{"mode":1,"modelId":""}}"#],
            )
            .expect("change persisted default");
        assert_eq!(
            adapter
                .verify_cached(&detection, &desired("fictional-secret"), &transaction)
                .expect_err("a changed default must not be reported as active")
                .code,
            "trae_binding_changed"
        );
        assert_eq!(
            fake.state.lock().expect("fake UI state").snapshots,
            snapshots
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
            display_name: "Previous managed model".to_owned(),
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
            TraeKind::Unified,
            detection,
            UiApplyPlan {
                row: ManagedRow {
                    display_name: managed_display_name_base(&input),
                    input,
                },
                previous_selection: "Auto Mode".to_owned(),
                previous_storage: None,
                previous_session: None,
                additional_previous: BTreeMap::new(),
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
    async fn rollback_restores_every_selector_even_when_one_baseline_fails() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let fake = Arc::new(FakeUi::new("Temporary"));
        {
            let mut state = fake.state.lock().expect("fake state");
            state.secondary_selection = "Temporary".to_owned();
            state.coder_selection = "Temporary".to_owned();
            state.ide_selection = "Temporary".to_owned();
        }
        let baseline = |selection: &str| LabelBaseline {
            selection: selection.to_owned(),
            storage: None,
            session: None,
            ide: None,
        };
        let rollback = rollback_ui_change(
            fake.clone(),
            TraeKind::Unified,
            detection,
            UiApplyPlan {
                row: ManagedRow {
                    display_name: "Temporary".to_owned(),
                    input,
                },
                previous_selection: "Auto Mode".to_owned(),
                previous_storage: None,
                previous_session: None,
                additional_previous: BTreeMap::from([
                    ("solo_agent".to_owned(), baseline("Missing baseline")),
                    ("solo_coder".to_owned(), baseline("Auto Mode")),
                    ("ide_composer".to_owned(), baseline("Auto Mode")),
                ]),
                created: false,
                borrowed: false,
                replaced_row: None,
            },
            UiMutationState::default(),
        )
        .await;
        assert!(!rollback.complete());
        let state = fake.state.lock().expect("fake state");
        assert_eq!(state.selection, "Auto Mode");
        assert_eq!(state.coder_selection, "Auto Mode");
        assert_eq!(state.ide_selection, "Auto Mode");
        assert_eq!(state.secondary_selection, "Temporary");
    }

    #[tokio::test]
    async fn rollback_restores_selections_before_deleting_and_retains_verified_runtime() {
        let temp = tempfile::tempdir().expect("temp");
        let detection = test_detection(&temp);
        let input = ManagedInput::from_desired(&desired("fictional-secret"));
        let fake = Arc::new(FakeUi::new("Temporary"));
        {
            let mut state = fake.state.lock().expect("fake state");
            state.models.insert("Temporary".to_owned());
            state.secondary_selection = "Temporary".to_owned();
            state.coder_selection = "Temporary".to_owned();
            state.ide_selection = "Temporary".to_owned();
        }
        let baseline = || LabelBaseline {
            selection: "Auto Mode".to_owned(),
            storage: None,
            session: None,
            ide: None,
        };
        let rollback = rollback_ui_change(
            fake.clone(),
            TraeKind::Unified,
            detection,
            UiApplyPlan {
                row: ManagedRow {
                    display_name: "Temporary".to_owned(),
                    input,
                },
                previous_selection: "Auto Mode".to_owned(),
                previous_storage: None,
                previous_session: None,
                additional_previous: BTreeMap::from([
                    ("solo_agent".to_owned(), baseline()),
                    ("solo_coder".to_owned(), baseline()),
                    ("ide_composer".to_owned(), baseline()),
                ]),
                created: true,
                borrowed: false,
                replaced_row: None,
            },
            UiMutationState {
                model_creation_attempted: true,
                model_created: true,
                ..Default::default()
            },
        )
        .await;
        assert!(rollback.complete());
        let state = fake.state.lock().expect("fake state");
        assert_eq!(
            state.selections_at_delete,
            vec![[
                "Auto Mode".to_owned(),
                "Auto Mode".to_owned(),
                "Auto Mode".to_owned(),
                "Auto Mode".to_owned()
            ]]
        );
        assert_eq!(state.completions, 1);
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "trae",
            "trae:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "trae:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                selection_baseline: None,
                session_baselines: BTreeMap::new(),
                additional_baselines: BTreeMap::new(),
                additional_selection_baselines: BTreeMap::new(),
                additional_session_baselines: BTreeMap::new(),
                ide_baseline: None,
                selection_label: Some(TraeKind::Unified.selection_label().to_owned()),
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
        let checkpoint = load_checkpoint(&transaction, "trae", "trae:12345")
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
        let transaction = ConfigTransaction::new(
            Arc::new(MemorySecretStore::default()),
            temp.path().join("backups"),
        );
        save_checkpoint(
            &transaction,
            "trae",
            "trae:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "trae:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                selection_baseline: None,
                session_baselines: BTreeMap::new(),
                additional_baselines: BTreeMap::new(),
                additional_selection_baselines: BTreeMap::new(),
                additional_session_baselines: BTreeMap::new(),
                ide_baseline: None,
                selection_label: Some(TraeKind::Unified.selection_label().to_owned()),
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
            "trae",
            "trae:12345",
            &Checkpoint {
                version: CHECKPOINT_VERSION,
                account_scope: "trae:12345".to_owned(),
                baseline_selection: "Auto Mode".to_owned(),
                selection_baseline: None,
                session_baselines: BTreeMap::new(),
                additional_baselines: BTreeMap::new(),
                additional_selection_baselines: BTreeMap::new(),
                additional_session_baselines: BTreeMap::new(),
                ide_baseline: None,
                selection_label: Some(TraeKind::Unified.selection_label().to_owned()),
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
        let checkpoint = load_checkpoint(&transaction, "trae", "trae:12345")
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
        let adapter = TraeAdapter::with_ui(TraeKind::Unified, fake.clone());
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
            active_account(&missing, TraeKind::Unified)
                .unwrap_err()
                .code,
            "trae_profile_uninitialized"
        );
        let empty = temp.path().join("empty.vscdb");
        Connection::open(&empty).expect("empty database");
        assert_eq!(
            active_account(&empty, TraeKind::Unified).unwrap_err().code,
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
        let selected_kind = std::env::var("AT_SWITCH_TRAE_KIND").unwrap_or_default();
        for adapter in [TraeAdapter::unified()] {
            if !selected_kind.is_empty() && selected_kind != adapter.id() {
                continue;
            }
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
            if adapter.kind == TraeKind::Unified {
                let ide = adapter
                    .ui
                    .snapshot_label(adapter.kind, &detection, "ide_composer")
                    .expect("IDE selector snapshot");
                assert!(!ide.selection.is_empty());
            }
        }
    }

    #[tokio::test]
    #[ignore = "mutates signed-in Trae account with a temporary local-only QA model"]
    async fn live_native_bridge_applies_and_restores_temporary_model() {
        if std::env::var("AT_SWITCH_TRAE_LIVE_MUTATION").as_deref() != Ok("1") {
            return;
        }
        let selected_kind = std::env::var("AT_SWITCH_TRAE_KIND").unwrap_or_default();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("local mock listener");
        let port = listener.local_addr().expect("local mock address").port();
        listener
            .set_nonblocking(true)
            .expect("local mock nonblocking");
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running_for_server = Arc::clone(&running);
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            while running_for_server.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0_u8; 8192];
                        let count = stream.read(&mut request).unwrap_or(0);
                        let streaming = request[..count]
                            .windows(13)
                            .any(|part| part == b"\"stream\":true");
                        let body = if streaming {
                            "data: {\"id\":\"atswitch-qa\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                        } else {
                            "{\"id\":\"atswitch-qa\",\"object\":\"chat.completion\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}"
                        };
                        let mime = if streaming {
                            "text/event-stream"
                        } else {
                            "application/json"
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        let context = DiscoveryContext::native();
        for adapter in [TraeAdapter::unified()] {
            if !selected_kind.is_empty() && selected_kind != adapter.id() {
                continue;
            }
            let detection = adapter.detect(&context);
            assert!(
                detection.write_supported,
                "{} must be writable",
                adapter.id()
            );
            let installation = detection.installation.as_ref().expect("Trae installation");
            let cold_pause = (std::env::var("AT_SWITCH_TRAE_LIVE_COLD").as_deref() == Ok("1"))
                .then(|| lifecycle::pause_for_config_update(&detection).expect("cold test pause"));
            let original_selection_state =
                live_selection_state(detection.config_path.as_deref().expect("Trae profile"));
            let original_ide_selection = if adapter.kind == TraeKind::Unified {
                ui::cached_ide_selection(detection.config_path.as_deref())
                    .expect("pre-test IDE selection")
            } else {
                None
            };
            let initial_runtime =
                crate::agents::lifecycle::runtime_status(installation, adapter.display_name());
            let temp = tempfile::tempdir().expect("temporary checkpoint");
            let transaction = ConfigTransaction::new(
                Arc::new(MemorySecretStore::default()),
                temp.path().join("backups"),
            );
            let base_url = format!("http://127.0.0.1:{port}/v1");
            let model_id = format!("atswitch-qa-{}", Uuid::new_v4().simple());
            let desired = DesiredAgentBinding {
                mode: AgentBindingMode::Direct,
                provider_name: "ATSwitch QA",
                model_id: &model_id,
                supports_tools: false,
                source_protocol: ApiProtocol::OpenaiChatCompletions,
                upstream_protocol: ApiProtocol::OpenaiChatCompletions,
                base_url: &base_url,
                credential: "sk-atswitch-qa-fake",
            };
            eprintln!("TRAE_LIVE_PHASE apply");
            let apply_started = std::time::Instant::now();
            let apply_result = adapter
                .apply(&detection, &desired, &transaction, &|| Ok(()))
                .await;
            if let Err(error) = apply_result {
                panic!("{} apply failed: {}", adapter.id(), error.code);
            }
            eprintln!("TRAE_LIVE_PHASE applied");
            eprintln!(
                "TRAE_LIVE_TIME apply_ms={}",
                apply_started.elapsed().as_millis()
            );
            assert_eq!(
                lifecycle::runtime_status(installation, adapter.display_name()),
                initial_runtime,
                "apply must preserve the initial runtime state",
            );
            let verification = adapter.verify_cached(&detection, &desired, &transaction);
            let selected = ui::cached_recent_selection(
                detection.config_path.as_deref(),
                adapter.kind.selection_label(),
            )
            .expect("persisted model selection")
            .expect("selected model");
            assert_eq!(selected.0, 0);
            assert!(stable_persisted_key_matches(
                detection.config_path.as_deref(),
                adapter.kind.selection_label(),
                &model_id,
                ApiProtocol::OpenaiChatCompletions,
                &selected.1,
            )
            .expect("stable persisted model key"));
            for label in adapter.kind.additional_selection_labels() {
                if *label == "ide_composer" {
                    let selected = ui::cached_ide_selection(detection.config_path.as_deref())
                        .expect("IDE persisted model selection")
                        .expect("IDE selected model");
                    assert_eq!(selected.0, 0);
                    assert!(stable_persisted_key_matches(
                        detection.config_path.as_deref(),
                        "solo_coder",
                        &model_id,
                        ApiProtocol::OpenaiChatCompletions,
                        &format!("solo_coder_{}", selected.1),
                    )
                    .expect("IDE stable persisted model key"));
                    continue;
                }
                let ide_selection =
                    ui::cached_recent_selection(detection.config_path.as_deref(), label)
                        .expect("IDE persisted model selection")
                        .expect("IDE selected model");
                assert_eq!(ide_selection.0, 0);
                assert!(stable_persisted_key_matches(
                    detection.config_path.as_deref(),
                    label,
                    &model_id,
                    ApiProtocol::OpenaiChatCompletions,
                    &ide_selection.1,
                )
                .expect("IDE stable persisted model key"));
            }
            if std::env::var("AT_SWITCH_TRAE_LIVE_SEQUENCE").as_deref() == Ok("1") {
                let catalog = ui::cached_custom_model_catalog(detection.config_path.as_deref())
                    .expect("existing native model catalogue");
                let existing_id = std::env::var("AT_SWITCH_TRAE_LIVE_EXISTING_MODEL")
                    .expect("existing-model sequence fixture");
                let names = catalog
                    .names_by_id
                    .get(&existing_id)
                    .expect("existing user model");
                assert_eq!(names.len(), 1, "sequence fixture must be unambiguous");
                let name = names.first().expect("existing name");
                let endpoints = catalog
                    .endpoints_by_name
                    .get(name)
                    .expect("existing endpoints");
                assert_eq!(endpoints.len(), 1);
                let endpoint = endpoints.first().expect("existing endpoint");
                let existing = DesiredAgentBinding {
                    provider_name: "ATSwitch QA borrowed",
                    model_id: &existing_id,
                    base_url: endpoint.trim_end_matches("/chat/completions"),
                    ..desired
                };
                for (phase, target) in [("warm_existing", &existing), ("warm_return", &desired)] {
                    assert!(!adapter.detect(&context).needs_restart,
                        "passive refresh must not restore the initial restart warning for a warm session");
                    eprintln!("TRAE_LIVE_PHASE {phase}");
                    let started = std::time::Instant::now();
                    adapter
                        .apply(&detection, target, &transaction, &|| Ok(()))
                        .await
                        .expect("warm sequence switch");
                    adapter
                        .verify_cached(&detection, target, &transaction)
                        .expect("all selectors must immediately match this switch");
                    assert!(
                        !adapter.detect(&context).needs_restart,
                        "confirmed warm switch must stay restart-free after refresh"
                    );
                    eprintln!(
                        "TRAE_LIVE_TIME {phase}_ms={}",
                        started.elapsed().as_millis()
                    );
                }
                let after = ui::cached_custom_model_catalog(detection.config_path.as_deref())
                    .expect("user model preserved during sequence");
                assert_eq!(after.names_by_id.get(&existing_id), Some(names));
            }
            if let Ok(seconds) = std::env::var("AT_SWITCH_TRAE_INSPECT_AFTER_APPLY_SECONDS") {
                let seconds = seconds.parse::<u64>().expect("inspection interval");
                eprintln!(
                    "{} live apply complete; holding for visible inspection",
                    adapter.id()
                );
                std::thread::sleep(Duration::from_secs(seconds));
            }
            eprintln!("TRAE_LIVE_PHASE restore");
            let restore_started = std::time::Instant::now();
            let restore_result = adapter.restore(&detection, &transaction, &|| Ok(())).await;
            verification.expect("managed model verified");
            let restored_outcome = restore_result.expect("restore original selection");
            let restored_checkpoint = load_checkpoint(
                &transaction,
                adapter.id(),
                &adapter.account_scope(&detection).expect("account scope"),
            )
            .expect("restore checkpoint")
            .expect("checkpoint retained");
            assert!(
                !restored_checkpoint.pending,
                "{} restore remains pending: {}",
                adapter.id(),
                restored_outcome.message
            );
            let models = ui::cached_custom_model_catalog(detection.config_path.as_deref())
                .expect("restored custom models");
            assert!(!models.names_by_id.contains_key(&model_id));
            let restored_selection_state =
                live_selection_state(detection.config_path.as_deref().expect("Trae profile"));
            assert!(
                original_selection_state == restored_selection_state,
                "{} must preserve pre-test recent and session selections",
                adapter.id()
            );
            if adapter.kind == TraeKind::Unified {
                assert_eq!(
                    ui::cached_ide_selection(detection.config_path.as_deref())
                        .expect("restored IDE selection"),
                    original_ide_selection,
                    "TRAE must preserve pre-test IDE model and mode"
                );
            }
            assert_eq!(
                crate::agents::lifecycle::runtime_status(installation, adapter.display_name()),
                initial_runtime,
                "{} runtime must be restored",
                adapter.id()
            );
            eprintln!("TRAE_LIVE_PHASE restored");
            eprintln!(
                "TRAE_LIVE_TIME restore_ms={}",
                restore_started.elapsed().as_millis()
            );
            if let Some(pause) = cold_pause {
                pause
                    .resume_and_wait()
                    .expect("restore runtime before cold test");
            }
        }
        running.store(false, std::sync::atomic::Ordering::Relaxed);
        server.join().expect("local mock shutdown");
    }

    #[test]
    #[ignore = "removes only local-only QA models left by an interrupted live test"]
    fn live_remove_interrupted_local_qa_models() {
        if std::env::var("AT_SWITCH_TRAE_CLEAN_QA").as_deref() != Ok("1") {
            return;
        }
        let context = DiscoveryContext::native();
        for adapter in [TraeAdapter::unified()] {
            let detection = adapter.detect(&context);
            let snapshot = adapter
                .ui
                .snapshot_interactive(adapter.kind, &detection)
                .expect("native QA model inventory");
            let names = snapshot
                .custom_models_by_id
                .iter()
                .filter(|(id, _)| id.starts_with("atswitch-qa-"))
                .flat_map(|(_, names)| names)
                .filter(|name| {
                    name.starts_with("ATSwitch QA")
                        && snapshot
                            .custom_endpoints_by_name
                            .get(*name)
                            .is_some_and(|endpoints| {
                                !endpoints.is_empty()
                                    && endpoints
                                        .iter()
                                        .all(|endpoint| endpoint.starts_with("http://127.0.0.1:"))
                            })
                })
                .cloned()
                .chain(
                    ui::cached_custom_model_catalog(detection.config_path.as_deref())
                        .expect("cached QA model inventory")
                        .names_by_id
                        .into_iter()
                        .filter(|(id, _)| id.starts_with("atswitch-qa-"))
                        .flat_map(|(_, names)| names)
                        .filter(|name| name.starts_with("ATSwitch QA")),
                )
                .collect::<std::collections::BTreeSet<_>>();
            for name in &names {
                adapter
                    .ui
                    .delete_model(adapter.kind, &detection, name)
                    .expect("delete interrupted local QA model");
            }
            eprintln!("{} removed {} local QA models", adapter.id(), names.len());
            adapter.ui.finish_operation(adapter.kind);
        }
    }

    fn live_selection_state(path: &Path) -> Vec<(String, String, i64, String)> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read Trae profile");
        let mut selections = Vec::new();
        for suffix in [
            "AI.agent.model.recent_user_selection_by_agent_label",
            "AI.agent.model.session_selected_model",
        ] {
            let raw: Option<String> = connection
                .query_row(
                    "SELECT value FROM ItemTable WHERE key LIKE ?1 LIMIT 1",
                    [format!("%{suffix}")],
                    |row| row.get(0),
                )
                .optional()
                .expect("stored Trae selection");
            let Some(raw) = raw else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&raw).expect("selection JSON");
            if suffix.contains("recent_user_selection") {
                for (label, selection) in value.as_object().expect("recent selections") {
                    selections.push(normalized_selection("recent", label, selection));
                }
            } else {
                for (session, labels) in value.as_object().expect("session selections") {
                    for (label, selection) in labels.as_object().expect("session labels") {
                        selections.push(normalized_selection(session, label, selection));
                    }
                }
            }
        }
        selections.sort();
        selections
    }

    fn normalized_selection(
        scope: &str,
        label: &str,
        selection: &serde_json::Value,
    ) -> (String, String, i64, String) {
        let mode = selection["mode"].as_i64().unwrap_or(-1);
        let key = if mode == 1 {
            String::new()
        } else {
            selection["modelId"].as_str().unwrap_or_default().to_owned()
        };
        (scope.to_owned(), label.to_owned(), mode, key)
    }
}
