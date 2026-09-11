//! Explicitly opted-in integration acceptance. This is excluded from all normal
//! tests: it accesses the logged-in ima account and changes real model settings.
//! Message prompts are sent only by the separately authorized UI acceptance
//! runner while this test waits at explicit stages.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    agents::{
        ima_adapter::{web_version, ImaAdapter},
        ima_local,
        locator::DiscoveryContext,
        AgentAdapter, DesiredAgentBinding,
    },
    domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError},
    infrastructure::{Database, NativeSecretStore, SecretStore, SecretValue},
    services::ConfigTransaction,
};

use super::ImaBindingManager;

/// Optional test-only handshakes keep a single native login session alive while
/// a human or UI acceptance runner checks the application's selected models.
struct LiveUiHandshake {
    directory: Option<PathBuf>,
}

impl LiveUiHandshake {
    const STAGES: [&'static str; 5] = [
        "native-before",
        "third-party",
        "third-party-repeat",
        "native-after",
        "third-party-after-restore",
    ];

    fn from_env() -> AppResult<Self> {
        let directory = std::env::var_os("AT_SWITCH_IMA_LIVE_UI_DIR").map(PathBuf::from);
        Self::new(directory)
    }

    fn new(directory: Option<PathBuf>) -> AppResult<Self> {
        if let Some(directory) = &directory {
            if !directory.is_absolute() {
                return Err(ui_marker_error());
            }
            std::fs::create_dir_all(directory).map_err(|_| ui_marker_error())?;
            for stage in Self::STAGES {
                for prefix in ["ready", "continue"] {
                    match std::fs::remove_file(directory.join(format!("{prefix}-{stage}"))) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => return Err(ui_marker_error()),
                    }
                }
            }
        }
        Ok(Self { directory })
    }

    async fn wait(&self, stage: &str) -> AppResult<()> {
        self.wait_with_timeout(stage, Duration::from_secs(10 * 60))
            .await
    }

    async fn wait_with_timeout(&self, stage: &str, timeout: Duration) -> AppResult<()> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        if !Self::STAGES.contains(&stage) {
            return Err(ui_marker_error());
        }
        std::fs::write(directory.join(format!("ready-{stage}")), stage.as_bytes())
            .map_err(|_| ui_marker_error())?;
        println!("IMA_LIVE_UI_STAGE={stage}");
        let continuation = directory.join(format!("continue-{stage}"));
        let started = tokio::time::Instant::now();
        loop {
            if continuation.is_file() {
                std::fs::remove_file(&continuation).map_err(|_| ui_marker_error())?;
                return Ok(());
            }
            if started.elapsed() >= timeout {
                return Err(CommandError::new(
                    "ima_live_ui_timeout",
                    "Live UI acceptance timed out; restore original settings",
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

fn ui_marker_error() -> CommandError {
    CommandError::new(
        "ima_live_ui_markers_invalid",
        "Live UI marker directory is unavailable",
    )
}

fn require_live_result<T>(stage: &'static str, result: AppResult<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("IMA_LIVE_FAILURE stage={stage} code={}", error.code),
    }
}

struct LiveProvider {
    provider_name: String,
    model_id: String,
    supports_tools: bool,
    base_url: String,
    credential: SecretValue,
}

impl LiveProvider {
    fn desired(&self) -> DesiredAgentBinding<'_> {
        DesiredAgentBinding {
            mode: AgentBindingMode::Direct,
            provider_name: &self.provider_name,
            model_id: &self.model_id,
            supports_tools: self.supports_tools,
            upstream_protocol: ApiProtocol::OpenaiChatCompletions,
            source_protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: &self.base_url,
            credential: self.credential.expose(),
        }
    }
}

fn configured_live_provider() -> AppResult<LiveProvider> {
    let database_path = PathBuf::from(
        std::env::var("AT_SWITCH_IMA_LIVE_DATABASE").map_err(|_| live_configuration_error())?,
    );
    if !database_path.is_absolute() || !database_path.is_file() {
        return Err(live_configuration_error());
    }
    let target_model =
        std::env::var("AT_SWITCH_IMA_LIVE_MODEL_ID").map_err(|_| live_configuration_error())?;
    let database = Database::open(&database_path)?;
    let mut matches = Vec::new();
    for provider in database.list_providers()? {
        for model in &provider.models {
            if model.model_id == target_model {
                matches.push((provider.clone(), model.clone()));
            }
        }
    }
    if matches.len() != 1 {
        return Err(live_configuration_error());
    }
    let (provider, model) = &matches[0];
    if provider.protocol != ApiProtocol::OpenaiChatCompletions || !provider.is_enabled {
        return Err(live_configuration_error());
    }
    let stored = database.get_provider(&provider.id)?;
    let secret_reference = stored
        .api_key_ref
        .as_deref()
        .ok_or_else(live_configuration_error)?;
    let credential = NativeSecretStore::default().get(secret_reference)?;
    Ok(LiveProvider {
        provider_name: provider.name.clone(),
        model_id: model.model_id.clone(),
        supports_tools: model.supports_tools,
        base_url: provider.base_url.clone(),
        credential,
    })
}

fn live_configuration_error() -> CommandError {
    CommandError::new(
        "ima_live_provider_invalid",
        "Live acceptance provider configuration is missing or ambiguous",
    )
}

/// Read-only native compatibility probe. Run with
/// `AT_SWITCH_IMA_LIVE_SCHEMA=1 cargo test --manifest-path src-tauri/Cargo.toml
/// ima_live_readonly_contract_shapes -- --ignored --nocapture` only after the
/// account owner authorizes native ima credential access. Output contains only
/// predefined field names, array indexes, and JSON type/presence labels.
#[tokio::test]
#[ignore = "requires explicit read-only native ima account authorization"]
async fn ima_live_readonly_contract_shapes() {
    assert_eq!(
        std::env::var("AT_SWITCH_IMA_LIVE_SCHEMA").as_deref(),
        Ok("1"),
        "Set AT_SWITCH_IMA_LIVE_SCHEMA=1 only after explicit read-only account authorization"
    );
    let detection = ImaAdapter::default().detect(&DiscoveryContext::native());
    assert!(
        detection.write_supported,
        "ima must be installed, initialized and logged in"
    );
    let preferences = detection.config_path.as_deref().expect("ima preferences");
    let version = web_version(preferences).expect("ima extension version");
    let result: AppResult<serde_json::Value> = async {
        let manager = ImaBindingManager::default();
        let client = manager.connect(&detection, &version).await?;
        client.contract_shapes().await
    }
    .await;
    match result {
        Ok(shape) => println!(
            "IMA_CONTRACT_SHAPES={}",
            serde_json::to_string(&shape).expect("serialize safe contract types")
        ),
        Err(error) => panic!(
            "Native read-only contract probe failed with stable error code: {}",
            error.code
        ),
    }
}

#[tokio::test]
#[ignore = "requires explicit read-only native ima account authorization"]
async fn ima_live_safe_checkpoint_diagnostics() {
    assert_eq!(
        std::env::var("AT_SWITCH_IMA_LIVE_DIAGNOSTICS").as_deref(),
        Ok("1"),
        "Set AT_SWITCH_IMA_LIVE_DIAGNOSTICS=1 only after explicit read-only authorization"
    );
    let backup_root = PathBuf::from(
        std::env::var("AT_SWITCH_IMA_LIVE_BACKUP_DIR")
            .expect("AT_SWITCH_IMA_LIVE_BACKUP_DIR must name the encrypted-backup root"),
    );
    let detection = ImaAdapter::default().detect(&DiscoveryContext::native());
    assert!(detection.write_supported, "ima must be ready");
    let preferences = detection.config_path.as_deref().expect("ima preferences");
    let version = web_version(preferences).expect("ima extension version");
    let manager = ImaBindingManager::default();
    let client = require_live_result(
        "diagnostics-auth",
        manager.connect(&detection, &version).await,
    );
    let transaction = ConfigTransaction::new(Arc::new(NativeSecretStore::default()), backup_root);
    let diagnostics = require_live_result(
        "diagnostics",
        manager
            .safe_checkpoint_diagnostics(&client, &detection, &transaction)
            .await,
    );
    println!("IMA_LIVE_SAFE_STATE={diagnostics}");
}

#[tokio::test]
#[ignore = "requires explicit live-account authorization and persistent encrypted backup directory"]
async fn ima_live_roundtrip_restores_original_models_and_preferences() {
    assert_eq!(
        std::env::var("AT_SWITCH_IMA_LIVE_ROUNDTRIP").as_deref(),
        Ok("1"),
        "Set AT_SWITCH_IMA_LIVE_ROUNDTRIP=1 only after explicit live-account authorization"
    );
    let backup_root =
        PathBuf::from(std::env::var("AT_SWITCH_IMA_LIVE_BACKUP_DIR").expect(
            "AT_SWITCH_IMA_LIVE_BACKUP_DIR must name a persistent encrypted-backup directory",
        ));
    assert!(
        backup_root.is_absolute(),
        "The encrypted backup directory must be absolute"
    );
    let ui = LiveUiHandshake::from_env().expect("initialize optional live UI handshake");
    let live_provider = require_live_result("provider", configured_live_provider());
    let adapter = ImaAdapter::default();
    let detection = adapter.detect(&DiscoveryContext::native());
    assert!(
        detection.write_supported,
        "ima must be installed, initialized and logged in"
    );
    let preferences = detection.config_path.as_deref().expect("ima preferences");
    let version = web_version(preferences).expect("ima extension version");
    let manager = ImaBindingManager::default();
    let client = require_live_result("initial-auth", manager.connect(&detection, &version).await);
    let transaction = ConfigTransaction::new(Arc::new(NativeSecretStore::default()), backup_root);
    // Reusing the same backup directory after an interrupted run first repairs
    // its journal. Never throw away a pending real-account recovery checkpoint.
    if manager
        .checkpoint_status(&detection, &transaction)
        .expect("checkpoint status")
        .is_some_and(|(managed, pending)| managed || pending)
    {
        if let Err(error) = manager
            .restore(&detection, &version, &transaction, &|| Ok(()))
            .await
        {
            if let Ok(diagnostics) = manager
                .safe_checkpoint_diagnostics(&client, &detection, &transaction)
                .await
            {
                eprintln!("IMA_LIVE_SAFE_STATE={diagnostics}");
            }
            require_live_result::<()>("initial-recovery", Err(error));
        }
    }
    let before = require_live_result("initial-snapshot", client.snapshot().await);
    let before_local = require_live_result("initial-local", ima_local::snapshot(preferences));
    let mut exercise_stage = "initial";
    let exercise: AppResult<()> = async {
        exercise_stage = "ui-native-before";
        ui.wait("native-before").await?;
        let desired = live_provider.desired();
        adapter.validate_binding(&desired)?;

        exercise_stage = "switch-third-party";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        let outcome = manager
            .apply(&detection, &version, &desired, &transaction, &|| Ok(()))
            .await?;
        if outcome.needs_restart {
            return Err(CommandError::new(
                "ima_live_restart_failed",
                "ima automatic relaunch needs attention",
            ));
        }
        manager.verify_cached(&detection, &desired, &transaction)?;
        let first_switch = client.snapshot().await?;
        exercise_stage = "ui-third-party";
        ui.wait("third-party").await?;

        exercise_stage = "switch-third-party-repeat";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        manager
            .apply(&detection, &version, &desired, &transaction, &|| Ok(()))
            .await?;
        manager.verify_cached(&detection, &desired, &transaction)?;
        let repeated_switch = client.snapshot().await?;
        if first_switch.homepage.models.len() != repeated_switch.homepage.models.len() {
            return Err(CommandError::new(
                "ima_live_duplicate_model",
                "Repeated switching created an extra ima model",
            ));
        }
        exercise_stage = "ui-third-party-repeat";
        ui.wait("third-party-repeat").await?;

        exercise_stage = "restore-native";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        manager
            .restore(&detection, &version, &transaction, &|| Ok(()))
            .await?;
        exercise_stage = "ui-native-after";
        ui.wait("native-after").await?;

        exercise_stage = "switch-third-party-after-restore";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        manager
            .apply(&detection, &version, &desired, &transaction, &|| Ok(()))
            .await?;
        manager.verify_cached(&detection, &desired, &transaction)?;
        exercise_stage = "ui-third-party-after-restore";
        ui.wait("third-party-after-restore").await?;
        Ok(())
    }
    .await;
    if let Err(error) = &exercise {
        eprintln!(
            "IMA_LIVE_FAILURE stage={exercise_stage} code={}",
            error.code
        );
    }
    // Cleanup is unconditional even when the roundtrip failed. On a failed
    // recovery the encrypted persistent journal is deliberately retained.
    let recovery = manager
        .restore(&detection, &version, &transaction, &|| Ok(()))
        .await;
    require_live_result("final-recovery", recovery);
    let after = require_live_result("final-snapshot", client.snapshot().await);
    assert!(
        before
            .scenes
            .iter()
            .zip(&after.scenes)
            .all(|(before, after)| before.preferred_model_id == after.preferred_model_id),
        "Original remote model preferences were not restored"
    );
    assert!(
        before.homepage.models.len() == after.homepage.models.len()
            && before
                .homepage
                .models
                .iter()
                .all(|model| after.homepage.models.contains(model)),
        "Original user-configured model rows were not preserved"
    );
    require_live_result(
        "final-local",
        ima_local::verify_snapshot(preferences, &before_local),
    );
    println!("IMA_LIVE_RESTORATION=verified");
    if let Err(error) = exercise {
        panic!(
            "IMA_LIVE_FAILURE stage={exercise_stage} code={} restoration=verified",
            error.code
        );
    }
}

#[tokio::test]
async fn ui_handshake_discards_stale_markers_and_consumes_only_the_current_stage() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("continue-native-before"), "stale").unwrap();
    std::fs::write(directory.path().join("ready-native-after"), "stale").unwrap();
    std::fs::write(directory.path().join("unrelated"), "keep").unwrap();
    let ui = LiveUiHandshake::new(Some(directory.path().to_path_buf())).unwrap();
    assert!(!directory.path().join("continue-native-before").exists());
    assert!(!directory.path().join("ready-native-after").exists());
    assert!(directory.path().join("unrelated").exists());
    std::fs::write(directory.path().join("continue-third-party"), "continue").unwrap();
    let timeout = ui
        .wait_with_timeout("native-before", Duration::ZERO)
        .await
        .unwrap_err();
    assert_eq!(timeout.code, "ima_live_ui_timeout");
    assert_eq!(
        std::fs::read_to_string(directory.path().join("ready-native-before")).unwrap(),
        "native-before"
    );
    assert!(directory.path().join("continue-third-party").exists());
    std::fs::write(directory.path().join("continue-native-before"), "continue").unwrap();
    ui.wait_with_timeout("native-before", Duration::ZERO)
        .await
        .unwrap();
    assert!(!directory.path().join("continue-native-before").exists());
    LiveUiHandshake::new(None)
        .unwrap()
        .wait("native-before")
        .await
        .unwrap();
}
