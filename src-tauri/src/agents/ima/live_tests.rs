//! Explicitly opted-in integration acceptance. This is excluded from all normal
//! tests: it accesses the logged-in ima account and changes real model settings.
//! No prompts or provider inference requests are issued by this test.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    agents::{
        ima_adapter::{web_version, ImaAdapter},
        ima_local,
        locator::DiscoveryContext,
        AgentAdapter, DesiredAgentBinding,
    },
    domain::{AgentBindingMode, ApiProtocol, AppResult, CommandError},
    infrastructure::NativeSecretStore,
    services::ConfigTransaction,
};

use super::ImaBindingManager;

/// Optional test-only handshakes keep a single native login session alive while
/// a human or UI acceptance runner checks the application's selected models.
struct LiveUiHandshake {
    directory: Option<PathBuf>,
}

impl LiveUiHandshake {
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
            for stage in ["a", "b", "native"] {
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
        if !matches!(stage, "a" | "b" | "native") {
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
        require_live_result(
            "initial-recovery",
            manager
                .restore(&detection, &version, &transaction, &|| Ok(()))
                .await,
        );
    }
    let before = require_live_result("initial-snapshot", client.snapshot().await);
    assert!(
        before.homepage.models.len() >= 2,
        "The live test requires two existing user-configured models"
    );
    let before_local = require_live_result("initial-local", ima_local::snapshot(preferences));
    let models = before
        .homepage
        .models
        .iter()
        .take(2)
        .map(|model| model.input())
        .collect::<Vec<_>>();
    let mut exercise_stage = "initial";
    let exercise: AppResult<()> = async {
        for (index, input) in models.iter().enumerate() {
            let desired = DesiredAgentBinding {
                mode: AgentBindingMode::Direct,
                provider_name: "ima existing configured model",
                model_id: &input.model_name,
                supports_tools: true,
                upstream_protocol: ApiProtocol::OpenaiChatCompletions,
                source_protocol: ApiProtocol::OpenaiChatCompletions,
                base_url: &input.api_uri,
                credential: input.api_key.expose(),
            };
            exercise_stage = if index == 0 {
                "validate-a"
            } else {
                "validate-b"
            };
            adapter.validate_binding(&desired)?;
            exercise_stage = if index == 0 { "switch-a" } else { "switch-b" };
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
            exercise_stage = if index == 0 { "verify-a" } else { "verify-b" };
            manager.verify_cached(&detection, &desired, &transaction)?;
            exercise_stage = if index == 0 { "ui-a" } else { "ui-b" };
            ui.wait(if index == 0 { "a" } else { "b" }).await?;
        }
        exercise_stage = "restore-native";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        manager
            .restore(&detection, &version, &transaction, &|| Ok(()))
            .await?;
        exercise_stage = "ui-native";
        ui.wait("native").await?;
        let input = &models[0];
        let desired = DesiredAgentBinding {
            mode: AgentBindingMode::Direct,
            provider_name: "ima existing configured model",
            model_id: &input.model_name,
            supports_tools: true,
            upstream_protocol: ApiProtocol::OpenaiChatCompletions,
            source_protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: &input.api_uri,
            credential: input.api_key.expose(),
        };
        exercise_stage = "switch-a-again";
        println!("IMA_LIVE_STAGE={exercise_stage}");
        manager
            .apply(&detection, &version, &desired, &transaction, &|| Ok(()))
            .await?;
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
    std::fs::write(directory.path().join("continue-a"), "stale").unwrap();
    std::fs::write(directory.path().join("ready-native"), "stale").unwrap();
    std::fs::write(directory.path().join("unrelated"), "keep").unwrap();
    let ui = LiveUiHandshake::new(Some(directory.path().to_path_buf())).unwrap();
    assert!(!directory.path().join("continue-a").exists());
    assert!(!directory.path().join("ready-native").exists());
    assert!(directory.path().join("unrelated").exists());
    std::fs::write(directory.path().join("continue-b"), "continue").unwrap();
    let timeout = ui.wait_with_timeout("a", Duration::ZERO).await.unwrap_err();
    assert_eq!(timeout.code, "ima_live_ui_timeout");
    assert_eq!(
        std::fs::read_to_string(directory.path().join("ready-a")).unwrap(),
        "a"
    );
    assert!(directory.path().join("continue-b").exists());
    std::fs::write(directory.path().join("continue-a"), "continue").unwrap();
    ui.wait_with_timeout("a", Duration::ZERO).await.unwrap();
    assert!(!directory.path().join("continue-a").exists());
    LiveUiHandshake::new(None).unwrap().wait("a").await.unwrap();
}
