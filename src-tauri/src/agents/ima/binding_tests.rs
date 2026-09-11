use std::sync::{Arc, Mutex};

use axum::{extract::State, http::Uri, routing::post, Json, Router};
use serde_json::{json, Value};

use super::*;
use crate::{
    agents::ima_local,
    domain::{AgentBindingMode, ApiProtocol},
    infrastructure::MemorySecretStore,
};

#[derive(Clone)]
struct RemoteState(Arc<Mutex<RemoteData>>);

struct RemoteData {
    models: Vec<Value>,
    preferred: [String; 2],
    serial: u64,
    fail_scene_one_once: bool,
    deletion_resets_preference: bool,
    add_count: u64,
    distinct_model_ids: bool,
    nested_custom_models: bool,
    ambiguous_model_links: bool,
    reject_official_preferences: bool,
}

fn selectable_id(data: &RemoteData, customize_id: &str, scene: usize) -> String {
    if data.distinct_model_ids {
        format!("selectable-{scene}-{customize_id}")
    } else {
        customize_id.to_owned()
    }
}

async fn remote(
    State(state): State<RemoteState>,
    uri: Uri,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut data = state.0.lock().unwrap();
    let response = match uri.path() {
        "/cgi-bin/customize_models/get_homepage" => json!({"code":0,"models":data.models,
            "customize_model_config":{"default_input_tokens":8192,"default_output_tokens":4096}}),
        "/cgi-bin/model_manage/get_models" => {
            let scene = body["scene"].as_u64().unwrap() as usize;
            let mut models = vec![
                json!({"model_id":format!("official-{scene}"),"model_type":10,"is_default":true,"sub_model_infos":{}}),
            ];
            for model in &data.models {
                let customize_id = model["customize_id"].as_str().unwrap();
                let mut option = json!({"model_id":selectable_id(&data,customize_id,scene),"customize_id":customize_id,"model_name":model["model_name"],"model_type":1000000,"sub_model_infos":{}});
                if data.nested_custom_models {
                    option.as_object_mut().unwrap().remove("model_type");
                    option = json!({"model_id":format!("category-{customize_id}"),"model_type":1000000,"sub_model_infos":{"0":option,"1":null,"2":{}}});
                }
                models.push(option);
                if data.ambiguous_model_links {
                    models.push(json!({"model_id":format!("other-{scene}-{customize_id}"),"customize_id":customize_id,"model_type":1000000}));
                }
            }
            json!({"code":0,"models":models,"preferred_model_id":data.preferred[scene]})
        }
        "/cgi-bin/customize_models/add_model" => {
            data.serial += 1;
            data.add_count += 1;
            let id = format!("owned-{}", data.serial);
            let mut model = body["model_info"].clone();
            model["customize_id"] = json!(id);
            data.models.push(model);
            json!({"code":0,"customize_id":id})
        }
        "/cgi-bin/customize_models/modify_model" => {
            let model = &body["model_info"];
            let target = data
                .models
                .iter_mut()
                .find(|target| target["customize_id"] == model["customize_id"])
                .unwrap();
            *target = model.clone();
            json!({"code":0})
        }
        "/cgi-bin/customize_models/set_preferred_model" => {
            let scene = body["scene"].as_u64().unwrap() as usize;
            if scene == 1 && data.fail_scene_one_once {
                data.fail_scene_one_once = false;
                json!({"code":51})
            } else {
                let id = body["model_id"].as_str().unwrap();
                if (data.reject_official_preferences && id == format!("official-{scene}"))
                    || id.is_empty()
                    || (id != format!("official-{scene}")
                        && !data.models.iter().any(|model| {
                            selectable_id(&data, model["customize_id"].as_str().unwrap(), scene)
                                == id
                        }))
                {
                    json!({"code":51})
                } else {
                    data.preferred[scene] = id.to_owned();
                    json!({"code":0,"preferred_model_id":id})
                }
            }
        }
        "/cgi-bin/customize_models/delete_model" => {
            let id = body["customize_id"].as_str().unwrap();
            let deleted_choices = [selectable_id(&data, id, 0), selectable_id(&data, id, 1)];
            data.models
                .retain(|model| model["customize_id"].as_str() != Some(id));
            if data.deletion_resets_preference {
                for (preferred, deleted) in data.preferred.iter_mut().zip(&deleted_choices) {
                    if preferred == deleted {
                        preferred.clear();
                    }
                }
            } else {
                for (scene, preferred) in data.preferred.iter_mut().enumerate() {
                    if deleted_choices[scene] == *preferred {
                        *preferred = "user-owned".to_owned();
                    }
                }
            }
            json!({"code":0})
        }
        path => panic!("unexpected mock path {path}"),
    };
    Json(response)
}

async fn fixture() -> (
    tempfile::TempDir,
    ImaClient,
    ConfigTransaction,
    RemoteState,
    tokio::task::JoinHandle<()>,
) {
    let (directory, session) = crate::agents::ima::auth::tests::test_session().await;
    let preferences = directory.path().join("Preferences");
    let mut root: Value = serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
    root["kExtraSettingInfo"] = json!(json!({"modelConfig":{"modelId":"official-0","modelType":10,"timestamp":7,"untouched":"existing"}}).to_string());
    std::fs::write(&preferences, serde_json::to_vec(&root).unwrap()).unwrap();
    let state = RemoteState(Arc::new(Mutex::new(RemoteData {
        models: vec![
            json!({"customize_id":"user-owned","model_name":"user-model","api_uri":"https://user.example/v1/chat/completions","api_key":"fictional-user-key","max_input_tokens":8192,"max_output_tokens":4096}),
        ],
        preferred: [String::new(), String::new()],
        serial: 0,
        fail_scene_one_once: false,
        deletion_resets_preference: true,
        add_count: 0,
        distinct_model_ids: false,
        nested_custom_models: false,
        ambiguous_model_links: false,
        reject_official_preferences: false,
    })));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .fallback(post(remote))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = ImaClient::new(session)
        .unwrap()
        .with_test_origin(format!("http://{address}"));
    let transaction = ConfigTransaction::new(
        Arc::new(MemorySecretStore::default()),
        directory.path().join("backups"),
    );
    (directory, client, transaction, state, server)
}

fn desired(model: &str) -> DesiredAgentBinding<'_> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "Fictional Provider",
        model_id: model,
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://provider.example/v1",
        credential: "fictional-provider-key",
    }
}

fn desired_existing_user_model() -> DesiredAgentBinding<'static> {
    DesiredAgentBinding {
        mode: AgentBindingMode::Direct,
        provider_name: "Existing ima model",
        model_id: "user-model",
        supports_tools: true,
        upstream_protocol: ApiProtocol::OpenaiChatCompletions,
        source_protocol: ApiProtocol::OpenaiChatCompletions,
        base_url: "https://user.example/v1",
        credential: "fictional-user-key",
    }
}

#[test]
fn identifies_a_selected_thinking_mode_under_an_official_default() {
    let scene: ImaSceneModels = serde_json::from_value(json!({
        "preferred_model_id": "official-thinking-mode",
        "models": [{
            "model_id": "official-parent",
            "model_type": 10,
            "is_default": true,
            "sub_model_infos": {
                "0": {"model_id": "official-fast-mode", "model_type": 10},
                "1": {"model_id": "official-thinking-mode", "model_type": 10}
            }
        }]
    }))
    .unwrap();

    assert!(is_official_default_choice(&scene, "official-thinking-mode"));
}

#[tokio::test]
async fn reuses_an_identical_existing_model_without_deleting_it_on_restore() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(
            &client,
            &path,
            &desired_existing_user_model(),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().add_count, 0);
    manager
        .restore_paused(&client, &path, &transaction, &|| Ok(()))
        .await
        .unwrap();
    let data = remote.0.lock().unwrap();
    assert_eq!(data.models.len(), 1);
    assert_eq!(data.models[0]["customize_id"], "user-owned");
    server.abort();
}

#[tokio::test]
async fn switching_and_restore_preserve_user_rows_unknown_local_fields_and_baseline() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let baseline = ima_local::snapshot(&path).unwrap();
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-b"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    {
        let data = remote.0.lock().unwrap();
        assert_eq!(data.models.len(), 2);
        assert_eq!(data.add_count, 1);
        assert_eq!(data.models[0]["api_key"], "fictional-user-key");
        assert_eq!(data.models[1]["model_name"], "model-b");
    }
    let mut root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut extra: Value =
        serde_json::from_str(root["kExtraSettingInfo"].as_str().unwrap()).unwrap();
    extra["copilotModelConfig"]["unrelated_added_after_switch"] = json!(true);
    root["kExtraSettingInfo"] = json!(extra.to_string());
    std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
    manager
        .restore_paused(&client, &path, &transaction, &|| Ok(()))
        .await
        .unwrap();
    ima_local::verify_snapshot(&path, &baseline).unwrap();
    let root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let extra: Value = serde_json::from_str(root["kExtraSettingInfo"].as_str().unwrap()).unwrap();
    assert_eq!(
        extra["copilotModelConfig"]["unrelated_added_after_switch"],
        true
    );
    assert_eq!(remote.0.lock().unwrap().models.len(), 2);
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-c"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().models.len(), 2);
    server.abort();
}

#[tokio::test]
async fn second_scene_failure_rolls_back_new_rows_and_both_original_preferences() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let before = ima_local::snapshot(&path).unwrap();
    remote.0.lock().unwrap().fail_scene_one_once = true;
    let error = ImaBindingManager::default()
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "ima_api_rejected");
    assert_eq!(remote.0.lock().unwrap().models.len(), 1);
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    ima_local::verify_snapshot(&path, &before).unwrap();
    server.abort();
}

#[tokio::test]
async fn failed_metadata_commit_restores_prior_owned_model_input_and_local_selection() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    let before = ima_local::snapshot(&path).unwrap();
    let error = manager
        .apply_paused(&client, &path, &desired("model-b"), &transaction, &|| {
            Err(CommandError::new("fictional_db_failure", "模拟数据库错误"))
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "fictional_db_failure");
    assert_eq!(remote.0.lock().unwrap().models[1]["model_name"], "model-a");
    ima_local::verify_snapshot(&path, &before).unwrap();
    server.abort();
}

#[tokio::test]
async fn restore_does_not_report_success_when_server_cannot_restore_empty_preference() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    remote.0.lock().unwrap().reject_official_preferences = true;
    let error = manager
        .restore_paused(&client, &path, &transaction, &|| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error.code, "ima_api_rejected");
    let data = remote.0.lock().unwrap();
    assert_eq!(data.models.len(), 2);
    assert_eq!(data.models[1]["model_name"], "model-a");
    assert_eq!(data.preferred, ["owned-1", "owned-1"]);
    server.abort();
}

#[tokio::test]
async fn interrupted_add_is_reused_without_duplicate_model_creation() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let before = client.snapshot().await.unwrap();
    let input = desired_input(&desired("model-a"), &before.homepage).unwrap();
    let mut record = load_or_create(&transaction, client.account_key(), &before, &path).unwrap();
    record.pending = Some(PendingOperation {
        before: selection_state(&before, &path).unwrap(),
        previous: None,
        before_ids: before
            .homepage
            .models
            .iter()
            .map(|model| model.customize_id.clone())
            .collect(),
        add_input: Some(input.clone()),
        created: None,
    });
    save(&transaction, &record).unwrap();
    client.add_model(&input).await.unwrap();
    // Reconstruct state from the encrypted journal as a new process would.
    drop(record);
    let mut record = load_or_create(&transaction, client.account_key(), &before, &path).unwrap();
    let managed = create_owned(&client, &transaction, &mut record, &input)
        .await
        .unwrap();
    assert_eq!(managed.customize_id, "owned-1");
    assert_eq!(remote.0.lock().unwrap().add_count, 1);
    recover_pending(&client, &path, &transaction, &mut record)
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().models.len(), 1);
    server.abort();
}

#[tokio::test]
async fn interrupted_first_switch_repairs_dangling_preferences_to_native_defaults() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let before = client.snapshot().await.unwrap();
    let input = desired_input(&desired("model-a"), &before.homepage).unwrap();
    let mut record = load_or_create(&transaction, client.account_key(), &before, &path).unwrap();
    record.pending = Some(PendingOperation {
        before: selection_state(&before, &path).unwrap(),
        previous: None,
        before_ids: before
            .homepage
            .models
            .iter()
            .map(|model| model.customize_id.clone())
            .collect(),
        add_input: Some(input),
        created: None,
    });
    save(&transaction, &record).unwrap();
    remote.0.lock().unwrap().preferred = ["deleted-0".to_owned(), "deleted-1".to_owned()];

    recover_pending(&client, &path, &transaction, &mut record)
        .await
        .unwrap();

    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    assert!(record.pending.is_none());
    assert!(record.managed.is_none());
    server.abort();
}

#[tokio::test]
async fn failed_restore_commit_recreates_only_owned_row_and_can_restore_again() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap();
    let before = ima_local::snapshot(&path).unwrap();
    let error = manager
        .restore_paused(&client, &path, &transaction, &|| {
            Err(CommandError::new("fictional_db_failure", "模拟数据库错误"))
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "fictional_db_failure");
    assert_eq!(remote.0.lock().unwrap().models.len(), 2);
    assert_eq!(remote.0.lock().unwrap().preferred, ["owned-1", "owned-1"]);
    ima_local::verify_snapshot(&path, &before).unwrap();
    manager
        .restore_paused(&client, &path, &transaction, &|| Ok(()))
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().models.len(), 2);
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    server.abort();
}

#[tokio::test]
async fn distinct_nested_model_ids_switch_and_restore_with_inherited_model_types() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let baseline = ima_local::snapshot(&path).unwrap();
    {
        let mut data = remote.0.lock().unwrap();
        data.distinct_model_ids = true;
        data.nested_custom_models = true;
        data.models[0]["model_name"] = json!("same-name");
    }
    let manager = ImaBindingManager::default();
    manager
        .apply_paused(&client, &path, &desired("same-name"), &transaction, &|| {
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["selectable-0-owned-1", "selectable-1-owned-1"]
    );
    ima_local::verify_selection(
        &path,
        &[
            Selection {
                model_id: "selectable-0-owned-1".to_owned(),
                model_type: 1000000,
            },
            Selection {
                model_id: "selectable-1-owned-1".to_owned(),
                model_type: 1000000,
            },
        ],
    )
    .unwrap();
    let error = manager
        .restore_paused(&client, &path, &transaction, &|| {
            Err(CommandError::new("fictional_db_failure", "模拟数据库错误"))
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "fictional_db_failure");
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["selectable-0-owned-1", "selectable-1-owned-1"]
    );
    ima_local::verify_selection(
        &path,
        &[
            Selection {
                model_id: "selectable-0-owned-1".to_owned(),
                model_type: 1000000,
            },
            Selection {
                model_id: "selectable-1-owned-1".to_owned(),
                model_type: 1000000,
            },
        ],
    )
    .unwrap();
    manager
        .restore_paused(&client, &path, &transaction, &|| Ok(()))
        .await
        .unwrap();
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    assert_eq!(remote.0.lock().unwrap().models.len(), 2);
    ima_local::verify_snapshot(&path, &baseline).unwrap();
    server.abort();
}

#[tokio::test]
async fn ambiguous_custom_model_links_roll_back_without_selecting_a_similar_name() {
    let (directory, client, transaction, remote, server) = fixture().await;
    let path = directory.path().join("Preferences");
    let baseline = ima_local::snapshot(&path).unwrap();
    {
        let mut data = remote.0.lock().unwrap();
        data.distinct_model_ids = true;
        data.ambiguous_model_links = true;
    }
    let error = ImaBindingManager::default()
        .apply_paused(
            &client,
            &path,
            &desired("model-a"),
            &transaction,
            &|| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "ima_model_mapping_ambiguous");
    assert_eq!(remote.0.lock().unwrap().models.len(), 1);
    assert_eq!(
        remote.0.lock().unwrap().preferred,
        ["official-0", "official-1"]
    );
    ima_local::verify_snapshot(&path, &baseline).unwrap();
    server.abort();
}
