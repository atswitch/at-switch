use std::sync::{atomic::AtomicUsize, Mutex};

use futures_util::future::BoxFuture;

use super::service_adapter::{CommitBinding, ServiceConfigOutcome};
use super::*;
use crate::{
    domain::{ModelDraft, ModelOutputModality, ProviderDraft, ProviderKind},
    infrastructure::MemorySecretStore,
};

const PROVIDER: &str = "fictional-provider";
const SECRET: &str = "fictional-provider-secret";

#[derive(Default)]
struct CountingSecrets {
    memory: MemorySecretStore,
    reads: AtomicUsize,
}

impl SecretStore for CountingSecrets {
    fn put(&self, reference: &str, secret: &SecretValue) -> AppResult<()> {
        self.memory.put(reference, secret)
    }
    fn get(&self, reference: &str) -> AppResult<SecretValue> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.memory.get(reference)
    }
    fn delete(&self, reference: &str) -> AppResult<()> {
        self.memory.delete(reference)
    }
    fn exists(&self, reference: &str) -> bool {
        self.memory.exists(reference)
    }
}

struct FakeState {
    scope: String,
    managed: bool,
    pending: bool,
    current_model: String,
    fail_restore_after_commit: bool,
    paused_operation: Option<PausedOperation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FakeOperation {
    Apply,
    Restore,
}

#[derive(Clone)]
struct PausedOperation {
    kind: FakeOperation,
    release: Arc<tokio::sync::Notify>,
    fail: bool,
}

struct FakeServiceAdapter {
    state: Arc<Mutex<FakeState>>,
    contacts: Arc<AtomicUsize>,
}

impl FakeServiceAdapter {
    async fn pause_if_requested(&self, kind: FakeOperation) -> AppResult<()> {
        let pause = {
            let mut state = self.state.lock().unwrap();
            if state
                .paused_operation
                .as_ref()
                .is_some_and(|pause| pause.kind == kind)
            {
                state.paused_operation.take()
            } else {
                None
            }
        };
        if let Some(pause) = pause {
            pause.release.notified().await;
            if pause.fail {
                return Err(CommandError::new(
                    "fictional_interleaved_failure",
                    "fictional delayed failure",
                ));
            }
        }
        Ok(())
    }
}

impl AgentAdapter for FakeServiceAdapter {
    fn id(&self) -> &'static str {
        "ima"
    }
    fn display_name(&self) -> &'static str {
        "ima"
    }
    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        AgentDetection {
            id: "ima",
            display_name: "ima",
            installation: None,
            config_path: Some(context.home.join("fictional-preferences")),
            runtime_data_dir: None,
            install_status: AgentInstallStatus::Installed,
            config_health: AgentConfigHealth::Healthy,
            write_supported: true,
            needs_restart: false,
            message: None,
            custom_install_path: None,
            using_custom_install_path: false,
        }
    }
    fn source_protocol(&self, _: AgentBindingMode, _: ApiProtocol) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }
    fn build_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<Vec<u8>> {
        panic!("service adapters must not use a file-write path")
    }
    fn build_native_config(&self, _: &AgentDetection, _: &BaselineSnapshot) -> AppResult<Vec<u8>> {
        panic!("service adapters must not use a file-restore path")
    }
    fn verify_config(&self, _: &AgentDetection, _: &DesiredAgentBinding<'_>) -> AppResult<()> {
        panic!("service adapters must use passive checkpoint verification")
    }
    fn service_config(&self) -> Option<&dyn ServiceConfigAdapter> {
        Some(self)
    }
}

impl ServiceConfigAdapter for FakeServiceAdapter {
    fn account_scope(&self, _: &AgentDetection) -> AppResult<String> {
        Ok(self.state.lock().unwrap().scope.clone())
    }
    fn checkpoint_status(
        &self,
        _: &AgentDetection,
        _: &ConfigTransaction,
    ) -> AppResult<Option<(bool, bool)>> {
        let state = self.state.lock().unwrap();
        Ok(Some((state.managed, state.pending)))
    }
    fn apply<'a>(
        &'a self,
        _: &'a AgentDetection,
        desired: &'a DesiredAgentBinding<'a>,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            self.contacts.fetch_add(1, Ordering::SeqCst);
            self.pause_if_requested(FakeOperation::Apply).await?;
            let scope = self.state.lock().unwrap().scope.clone();
            transaction.save_service_checkpoint("ima", &scope, b"fictional-checkpoint")?;
            commit()?;
            let mut state = self.state.lock().unwrap();
            state.managed = true;
            state.current_model = desired.model_id.to_owned();
            Ok(ServiceConfigOutcome {
                needs_restart: false,
                message: "fictional switched".into(),
            })
        })
    }
    fn restore<'a>(
        &'a self,
        _: &'a AgentDetection,
        _: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>> {
        Box::pin(async move {
            self.contacts.fetch_add(1, Ordering::SeqCst);
            self.pause_if_requested(FakeOperation::Restore).await?;
            commit()?;
            if self.state.lock().unwrap().fail_restore_after_commit {
                return Err(CommandError::new(
                    "fictional_restore_failed",
                    "fictional recovery failure",
                ));
            }
            self.state.lock().unwrap().managed = false;
            Ok(ServiceConfigOutcome {
                needs_restart: false,
                message: "fictional restored".into(),
            })
        })
    }
    fn verify_cached(
        &self,
        _: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
        _: &ConfigTransaction,
    ) -> AppResult<()> {
        if self.state.lock().unwrap().current_model == desired.model_id {
            Ok(())
        } else {
            Err(CommandError::new(
                "ima_binding_changed",
                "fictional account has another model",
            ))
        }
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    service: AgentService,
    secrets: Arc<CountingSecrets>,
    state: Arc<Mutex<FakeState>>,
    contacts: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::in_memory().unwrap());
        let secrets = Arc::new(CountingSecrets::default());
        secrets
            .put(SECRET, &SecretValue::new("fictional-api-key".into()))
            .unwrap();
        database
            .save_provider(
                PROVIDER,
                &ProviderDraft {
                    id: Some(PROVIDER.into()),
                    name: "Fictional provider".into(),
                    kind: ProviderKind::Custom,
                    protocol: ApiProtocol::OpenaiChatCompletions,
                    base_url: "https://provider.example.test/v1".into(),
                    api_key: None,
                    default_model_id: Some("fictional-model".into()),
                    allow_insecure_http: false,
                    models: ["fictional-model", "fictional-next-model"]
                        .into_iter()
                        .map(|model_id| ModelDraft {
                            model_id: model_id.into(),
                            display_name: model_id.into(),
                            output_modality: ModelOutputModality::Text,
                            supports_streaming: false,
                            supports_tools: false,
                        })
                        .collect(),
                },
                Some(SECRET),
                1,
                None,
            )
            .unwrap();
        let state = Arc::new(Mutex::new(FakeState {
            scope: "fictional-account".into(),
            managed: false,
            pending: false,
            current_model: "fictional-model".into(),
            fail_restore_after_commit: false,
            paused_operation: None,
        }));
        let contacts = Arc::new(AtomicUsize::new(0));
        let context = DiscoveryContext {
            home: temp.path().to_path_buf(),
            application_data_dir: temp.path().join("data"),
            application_dirs: vec![],
            path_entries: vec![],
            system_application_search: false,
            custom_installation_path: None,
            #[cfg(target_os = "windows")]
            local_app_data: None,
            #[cfg(target_os = "windows")]
            program_files: vec![],
        };
        let secret_store: Arc<dyn SecretStore> = secrets.clone();
        let proxy = ProxySupervisor::new(54187, secret_store.clone()).unwrap();
        let service = AgentService {
            registry: AgentRegistry {
                adapters: vec![Box::new(FakeServiceAdapter {
                    state: state.clone(),
                    contacts: contacts.clone(),
                })],
                context,
            },
            database,
            secret_store: secret_store.clone(),
            transaction: ConfigTransaction::new(secret_store, temp.path().join("backups")),
            proxy,
            proxy_routes_restored: AtomicBool::new(false),
            service_operations: tokio::sync::Mutex::new(()),
        };
        Self {
            _temp: temp,
            service,
            secrets,
            state,
            contacts,
        }
    }

    fn checkpoint(&self, managed: bool, pending: bool) {
        let mut state = self.state.lock().unwrap();
        state.managed = managed;
        state.pending = pending;
        self.service
            .transaction
            .save_service_checkpoint("ima", &state.scope, b"fictional-checkpoint")
            .unwrap();
    }

    fn binding(&self) {
        let detection = self.service.registry.adapters[0].detect(&self.service.registry.context);
        self.service
            .database
            .upsert_agent_state(&detection.summary())
            .unwrap();
        self.service
            .database
            .save_agent_binding(&StoredAgentBinding {
                agent_id: "ima".into(),
                provider_id: PROVIDER.into(),
                default_model_id: "fictional-model".into(),
                mode: "direct".into(),
                request_protocol: ApiProtocol::OpenaiChatCompletions,
                local_token_ref: None,
                local_token_revision: 0,
            })
            .unwrap();
    }
}

#[tokio::test]
async fn first_service_switch_requires_confirmation_before_credentials_or_mutations() {
    let fixture = Fixture::new();
    let draft = AgentBindingDraft {
        agent_id: "ima".into(),
        provider_id: PROVIDER.into(),
        model_id: "fictional-model".into(),
        mode: AgentBindingMode::Direct,
    };
    let error = fixture
        .service
        .apply_authorized(draft.clone(), false)
        .await
        .unwrap_err();
    assert_eq!(error.code, "agent_account_connection_required");
    assert_eq!(fixture.secrets.reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.contacts.load(Ordering::SeqCst), 0);
    assert!(fixture
        .service
        .database
        .get_agent_binding("ima")
        .unwrap()
        .is_none());
    fixture.service.apply_authorized(draft, true).await.unwrap();
    assert_eq!(fixture.contacts.load(Ordering::SeqCst), 1);
    assert!(!fixture.service.scan().unwrap()[0].requires_account_connection);
}

#[tokio::test]
async fn failed_service_restore_blocks_provider_deletion_and_keeps_binding_and_key() {
    let fixture = Fixture::new();
    fixture.checkpoint(true, false);
    fixture.binding();
    fixture.state.lock().unwrap().fail_restore_after_commit = true;
    let error = fixture
        .service
        .delete_provider_with_service_restore(PROVIDER, &|| {
            panic!("Provider deletion must not run after failed restoration")
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "fictional_restore_failed");
    assert!(fixture
        .service
        .database
        .get_agent_binding("ima")
        .unwrap()
        .is_some());
    assert!(fixture.service.database.get_provider(PROVIDER).is_ok());
    assert!(fixture.secrets.exists(SECRET));
}

#[test]
fn passive_scan_exposes_interrupted_first_switch_without_a_database_binding() {
    let fixture = Fixture::new();
    fixture.checkpoint(false, true);
    let summary = fixture.service.scan().unwrap().remove(0);
    assert!(matches!(
        summary.config_health,
        AgentConfigHealth::TakeoverInterrupted
    ));
    assert!(summary.activation_required);
    assert!(summary.adapter_verified);
    assert!(summary.provider_id.is_none());
    assert_eq!(fixture.contacts.load(Ordering::SeqCst), 0);
}

#[test]
fn passive_scan_does_not_show_another_accounts_database_binding_as_current() {
    let fixture = Fixture::new();
    fixture.checkpoint(true, false);
    fixture.binding();
    fixture.state.lock().unwrap().current_model = "fictional-other-account-model".into();
    let summary = fixture.service.scan().unwrap().remove(0);
    assert!(matches!(
        summary.config_health,
        AgentConfigHealth::ExternalChanged
    ));
    assert!(summary.activation_required);
    assert!(summary.provider_id.is_none());
    assert!(summary.model_id.is_none());
    assert_eq!(fixture.contacts.load(Ordering::SeqCst), 0);
}

async fn failing_operation_cannot_overwrite_a_concurrent_success(kind: FakeOperation) {
    let fixture = Fixture::new();
    fixture.checkpoint(true, false);
    fixture.binding();
    let release = Arc::new(tokio::sync::Notify::new());
    fixture.state.lock().unwrap().paused_operation = Some(PausedOperation {
        kind,
        release: release.clone(),
        fail: true,
    });
    let draft = |model: &str| AgentBindingDraft {
        agent_id: "ima".into(),
        provider_id: PROVIDER.into(),
        model_id: model.into(),
        mode: AgentBindingMode::Direct,
    };
    let first = async {
        match kind {
            FakeOperation::Apply => {
                fixture
                    .service
                    .apply_authorized(draft("fictional-model"), false)
                    .await
            }
            FakeOperation::Restore => {
                fixture
                    .service
                    .restore_native_authorized("ima", false)
                    .await
            }
        }
    };
    tokio::pin!(first);
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    let next = fixture
        .service
        .apply_authorized(draft("fictional-next-model"), false);
    tokio::pin!(next);
    // With the old adapter-only lock, this second IPC can complete while the
    // first still owns its stale database snapshot. Releasing the first then
    // makes its failure compensation overwrite this successful binding.
    let completed_next = match futures_util::poll!(next.as_mut()) {
        std::task::Poll::Ready(result) => Some(result),
        std::task::Poll::Pending => None,
    };
    release.notify_one();
    assert_eq!(
        first.await.unwrap_err().code,
        "fictional_interleaved_failure"
    );
    match completed_next {
        Some(result) => result,
        None => next.await,
    }
    .unwrap();
    assert_eq!(
        fixture
            .service
            .database
            .get_agent_binding("ima")
            .unwrap()
            .unwrap()
            .default_model_id,
        "fictional-next-model"
    );
    assert_eq!(
        fixture.state.lock().unwrap().current_model,
        "fictional-next-model"
    );
}

#[tokio::test]
async fn failed_switch_cannot_roll_back_a_concurrent_successful_switch() {
    failing_operation_cannot_overwrite_a_concurrent_success(FakeOperation::Apply).await;
}

#[tokio::test]
async fn failed_restore_cannot_roll_back_a_concurrent_successful_switch() {
    failing_operation_cannot_overwrite_a_concurrent_success(FakeOperation::Restore).await;
}

#[tokio::test]
async fn provider_deletion_serializes_restore_and_delete_before_a_waiting_switch() {
    let fixture = Fixture::new();
    fixture.checkpoint(true, false);
    fixture.binding();
    let release = Arc::new(tokio::sync::Notify::new());
    fixture.state.lock().unwrap().paused_operation = Some(PausedOperation {
        kind: FakeOperation::Restore,
        release: release.clone(),
        fail: false,
    });
    let delete = || {
        let (reference, _) = fixture.service.database.delete_provider(PROVIDER)?;
        if let Some(reference) = reference {
            fixture.secrets.delete(&reference)?;
        }
        Ok(())
    };
    let deletion = fixture
        .service
        .delete_provider_with_service_restore(PROVIDER, &delete);
    tokio::pin!(deletion);
    assert!(futures_util::poll!(deletion.as_mut()).is_pending());
    let next = fixture.service.apply_authorized(
        AgentBindingDraft {
            agent_id: "ima".into(),
            provider_id: PROVIDER.into(),
            model_id: "fictional-next-model".into(),
            mode: AgentBindingMode::Direct,
        },
        false,
    );
    tokio::pin!(next);
    let completed_next = match futures_util::poll!(next.as_mut()) {
        std::task::Poll::Ready(result) => Some(result),
        std::task::Poll::Pending => None,
    };
    release.notify_one();
    assert_eq!(deletion.await.unwrap(), vec!["ima".to_owned()]);
    assert!(match completed_next {
        Some(result) => result,
        None => next.await,
    }
    .is_err());
    assert!(fixture
        .service
        .database
        .get_agent_binding("ima")
        .unwrap()
        .is_none());
    assert!(!fixture.secrets.exists(SECRET));
    assert_eq!(fixture.contacts.load(Ordering::SeqCst), 1);
}
