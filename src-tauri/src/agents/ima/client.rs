use std::{sync::Arc, time::Duration};

use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::domain::{AppResult, CommandError};

use super::{ImaAddResult, ImaHomePage, ImaModelInput, ImaSceneModels, ImaSession, ImaSnapshot};

const API_ORIGIN: &str = "https://ima.qq.com";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Authenticated ima API access. Production requests have a fixed HTTPS origin
/// and redirects are disabled so credentials cannot follow an upstream redirect.
pub struct ImaClient {
    http: Client,
    origin: String,
    session: Arc<ImaSession>,
}

#[derive(Deserialize)]
struct Envelope {
    code: i64,
}

#[derive(Deserialize)]
struct EmptyResponse {}

#[derive(Deserialize)]
struct PreferredResponse {
    #[serde(default)]
    preferred_model_id: Option<String>,
}

#[cfg(all(test, target_os = "macos"))]
#[derive(Deserialize)]
#[serde(transparent)]
struct SensitiveJson(serde_json::Value);

#[cfg(all(test, target_os = "macos"))]
impl Drop for SensitiveJson {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        fn wipe(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(value) => value.zeroize(),
                serde_json::Value::Array(values) => values.iter_mut().for_each(wipe),
                serde_json::Value::Object(values) => values.values_mut().for_each(wipe),
                _ => {}
            }
        }
        wipe(&mut self.0);
    }
}

impl ImaClient {
    /// Test-only compatibility diagnostics. Only field-presence/type labels
    /// leave this method; all actual strings in the temporary payloads are wiped.
    #[cfg(all(test, target_os = "macos"))]
    pub(super) async fn contract_shapes(&self) -> AppResult<serde_json::Value> {
        let homepage_request = serde_json::json!({});
        let qa_request = serde_json::json!({"scene":0});
        let copilot_request = serde_json::json!({"scene":1});
        let (homepage, qa, copilot): (SensitiveJson, SensitiveJson, SensitiveJson) = tokio::try_join!(
            self.post("customize_models/get_homepage", &homepage_request),
            self.post("model_manage/get_models", &qa_request),
            self.post("model_manage/get_models", &copilot_request),
        )?;
        self.session.ensure_current_account()?;
        Ok(serde_json::json!({
            "homepage": contract_response_shape(&homepage.0, false),
            "qa_scene": contract_response_shape(&qa.0, true),
            "copilot_scene": contract_response_shape(&copilot.0, true),
        }))
    }

    #[cfg(test)]
    pub(super) fn with_test_origin(mut self, origin: String) -> Self {
        self.origin = origin;
        self
    }

    pub fn new(session: Arc<ImaSession>) -> AppResult<Self> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| network_error())?;
        Ok(Self {
            http,
            origin: API_ORIGIN.to_owned(),
            session,
        })
    }

    pub fn account_key(&self) -> &str {
        self.session.account_key()
    }

    pub async fn homepage(&self) -> AppResult<ImaHomePage> {
        self.post("customize_models/get_homepage", &serde_json::json!({}))
            .await
    }

    pub async fn scene_models(&self, scene: u8) -> AppResult<ImaSceneModels> {
        validate_scene(scene)?;
        let mut models: ImaSceneModels = self
            .post(
                "model_manage/get_models",
                &serde_json::json!({ "scene": scene }),
            )
            .await?;
        // ima selects a sub-model's explicit type, falling back to its parent's
        // type when the wire response omits it.
        models.inherit_model_types();
        Ok(models)
    }

    pub async fn snapshot(&self) -> AppResult<ImaSnapshot> {
        let (homepage, qa, copilot) =
            tokio::try_join!(self.homepage(), self.scene_models(0), self.scene_models(1),)?;
        self.session.ensure_current_account()?;
        Ok(ImaSnapshot {
            account_key: self.account_key().to_owned(),
            homepage,
            scenes: [qa, copilot],
        })
    }

    pub async fn add_model(&self, input: &ImaModelInput) -> AppResult<ImaAddResult> {
        #[derive(Serialize)]
        struct AddRequest<'a> {
            model_info: &'a ImaModelInput,
        }
        self.post(
            "customize_models/add_model",
            &AddRequest { model_info: input },
        )
        .await
    }

    pub async fn modify_model(&self, customize_id: &str, input: &ImaModelInput) -> AppResult<()> {
        validate_id(customize_id)?;
        #[derive(Serialize)]
        struct ModifyInfo<'a> {
            customize_id: &'a str,
            #[serde(flatten)]
            input: &'a ImaModelInput,
        }
        #[derive(Serialize)]
        struct ModifyRequest<'a> {
            model_info: ModifyInfo<'a>,
        }
        let _: EmptyResponse = self
            .post(
                "customize_models/modify_model",
                &ModifyRequest {
                    model_info: ModifyInfo {
                        customize_id,
                        input,
                    },
                },
            )
            .await?;
        Ok(())
    }

    pub async fn set_preferred_model(&self, scene: u8, model_id: &str) -> AppResult<()> {
        validate_scene(scene)?;
        validate_id(model_id)?;
        let response: PreferredResponse = self
            .post(
                "customize_models/set_preferred_model",
                &serde_json::json!({
                    "scene": scene, "model_id": model_id, "set_if_absent": false,
                }),
            )
            .await?;
        if response
            .preferred_model_id
            .as_deref()
            .is_some_and(|id| id != model_id)
        {
            return Err(
                CommandError::new("ima_preference_rejected", "ima 未接受所选首选模型")
                    .with_recovery("请确认该模型在 ima 中可用，再重试。"),
            );
        }
        Ok(())
    }

    pub async fn delete_model(&self, customize_id: &str) -> AppResult<()> {
        validate_id(customize_id)?;
        if !self
            .homepage()
            .await?
            .models
            .iter()
            .any(|model| model.customize_id == customize_id)
        {
            return Ok(());
        }
        let _: EmptyResponse = self
            .post(
                "customize_models/delete_model",
                &serde_json::json!({
                    "customize_id": customize_id,
                }),
            )
            .await?;
        if self
            .homepage()
            .await?
            .models
            .iter()
            .any(|model| model.customize_id == customize_id)
        {
            return Err(
                CommandError::new("ima_delete_not_applied", "ima 尚未移除已管理的模型")
                    .with_recovery("请稍后再次恢复原配置；AT-Switch 会保留恢复记录。"),
            );
        }
        Ok(())
    }

    async fn post<Q: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &Q,
    ) -> AppResult<R> {
        self.session.ensure_current_account()?;
        let mut response = self
            .http
            .post(format!("{}/cgi-bin/{endpoint}", self.origin))
            .headers(self.session.headers()?)
            .json(body)
            .send()
            .await
            .map_err(|_| network_error())?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.session.invalidate();
            return Err(
                CommandError::new("ima_session_expired", "ima 登录连接已失效")
                    .with_recovery("请在 ima 中确认登录状态，再返回 AT-Switch 重试。"),
            );
        }
        if !response.status().is_success() {
            #[cfg(test)]
            eprintln!(
                "IMA_HTTP_FAILURE endpoint={} status={}",
                diagnostic_endpoint(endpoint),
                response.status().as_u16()
            );
            return Err(
                CommandError::new("ima_http_failed", "ima 模型服务暂时无法完成请求")
                    .with_recovery("请检查网络连接，并稍后重试。"),
            );
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(response_invalid());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| network_error())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(response_invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| response_invalid())?;
        #[cfg(test)]
        if envelope.code != 0 {
            eprintln!(
                "IMA_API_FAILURE endpoint={} code={}",
                diagnostic_endpoint(endpoint),
                envelope.code
            );
        }
        // ima's client classifies these service codes as login/token expiry.
        if matches!(
            envelope.code,
            41 | 1100 | 1101 | 40030 | 600001 | 110030 | 110031
        ) {
            self.session.invalidate();
            return Err(
                CommandError::new("ima_session_expired", "ima 登录连接已失效")
                    .with_recovery("请在 ima 中确认登录状态，再返回 AT-Switch 重试。"),
            );
        }
        // The current ima settings extension classifies 100003 as a request
        // frequency limit. Keep it distinct from an invalid model setup so the
        // user does not edit credentials or retry a destructive operation.
        if envelope.code == 100003 {
            return Err(
                CommandError::new("ima_rate_limited", "ima 暂时限制了模型配置请求")
                    .with_recovery("请稍后重试；AT-Switch 已保留原始模型和恢复记录。"),
            );
        }
        if envelope.code != 0 {
            // Neither ima's message nor its response body is safe to surface:
            // upstream validation messages may contain credentials or model URLs.
            return Err(
                CommandError::new("ima_api_rejected", "ima 未接受本次模型配置操作")
                    .with_recovery("请检查 ima 登录状态及模型配置，确认模型数量未达上限，再重试。"),
            );
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            #[cfg(test)]
            eprintln!(
                "IMA_DTO_FAILURE endpoint={} code=ima_response_unsupported",
                diagnostic_endpoint(endpoint)
            );
            #[cfg(all(test, target_os = "macos"))]
            if let Ok(raw) = serde_json::from_slice::<SensitiveJson>(&bytes) {
                if matches!(
                    endpoint,
                    "customize_models/get_homepage" | "model_manage/get_models"
                ) {
                    let shape =
                        contract_response_shape(&raw.0, endpoint == "model_manage/get_models");
                    eprintln!(
                        "IMA_DTO_SHAPE endpoint={} shape={shape}",
                        diagnostic_endpoint(endpoint)
                    );
                }
            }
            response_invalid()
        })
    }
}

#[cfg(test)]
fn diagnostic_endpoint(endpoint: &str) -> &'static str {
    match endpoint {
        "customize_models/get_homepage" => "get_homepage",
        "model_manage/get_models" => "get_models",
        "customize_models/add_model" => "add_model",
        "customize_models/modify_model" => "modify_model",
        "customize_models/set_preferred_model" => "set_preferred_model",
        "customize_models/delete_model" => "delete_model",
        _ => "unknown",
    }
}

#[cfg(test)]
fn contract_response_shape(value: &serde_json::Value, scene: bool) -> serde_json::Value {
    use serde_json::{json, Map, Value};
    fn kind(value: Option<&Value>) -> &'static str {
        match value {
            None => "missing",
            Some(Value::Null) => "null",
            Some(Value::Bool(_)) => "boolean",
            Some(Value::Number(_)) => "number",
            Some(Value::String(_)) => "string",
            Some(Value::Array(_)) => "array",
            Some(Value::Object(_)) => "object",
        }
    }
    fn fields(value: Option<&Value>, names: &[&str]) -> Map<String, Value> {
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    json!(kind(value.and_then(|value| value.get(*name)))),
                )
            })
            .collect()
    }
    fn models(value: Option<&Value>, scene: bool) -> Value {
        let names: &[&str] = if scene {
            &[
                "model_id",
                "model_type",
                "model_name",
                "customize_id",
                "is_default",
                "sub_model_infos",
            ]
        } else {
            &[
                "customize_id",
                "model_id",
                "supplier_id",
                "api_uri",
                "api_key",
                "model_name",
                "max_input_tokens",
                "max_output_tokens",
            ]
        };
        let items: Vec<&Value> = match value {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(Value::Object(items)) if scene => items.values().collect(),
            _ => return json!({"type":kind(value)}),
        };
        json!({"type":kind(value),"items":items.into_iter().enumerate().map(|(index,item)| {
            let mut shape = json!({"index":index,"type":kind(Some(item)),"fields":fields(Some(item),names)});
            if scene { shape["sub_models"] = models(item.get("sub_model_infos"),true); }
            shape
        }).collect::<Vec<_>>()})
    }
    let names: &[&str] = if scene {
        &["code", "models", "preferred_model_id"]
    } else {
        &["code", "models", "customize_model_config"]
    };
    let mut shape = json!({"type":kind(Some(value)),"fields":fields(Some(value),names),"models":models(value.get("models"),scene)});
    if !scene {
        shape["limits"] = json!({"type":kind(value.get("customize_model_config")),"fields":fields(value.get("customize_model_config"), &["default_input_tokens","default_output_tokens"])});
    }
    shape
}

fn validate_scene(scene: u8) -> AppResult<()> {
    if scene > 1 {
        Err(CommandError::new(
            "ima_scene_invalid",
            "不支持的 ima 模型入口",
        ))
    } else {
        Ok(())
    }
}

fn validate_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control) {
        Err(
            CommandError::new("ima_model_id_invalid", "无法识别 ima 模型标识")
                .with_recovery("请刷新 ima 模型列表后重试。"),
        )
    } else {
        Ok(())
    }
}

fn network_error() -> CommandError {
    CommandError::new("ima_network_failed", "无法连接 ima 模型服务")
        .with_recovery("请确认网络可访问 ima，稍后重试；中断的切换可通过恢复原配置重试。")
}

fn response_invalid() -> CommandError {
    CommandError::new("ima_response_unsupported", "无法识别 ima 模型服务的响应")
        .with_recovery("请更新 AT-Switch 后重试；已有备份会保留用于恢复。")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::Mutex,
    };

    use axum::{
        extract::{Request, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
        Json, Router,
    };

    use super::*;
    use crate::agents::ima::{auth::tests::test_session, ImaSecret};

    type RecordedRequest = (String, HeaderMap, serde_json::Value);
    type ResponseMap = HashMap<(String, Option<u64>), serde_json::Value>;

    #[derive(Clone, Default)]
    struct MockState {
        responses: Arc<Mutex<VecDeque<(StatusCode, serde_json::Value)>>>,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        route_responses: Arc<Mutex<ResponseMap>>,
    }

    async fn handle(State(state): State<MockState>, request: Request) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let body = axum::body::to_bytes(body, MAX_RESPONSE_BYTES)
            .await
            .unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let response_key = (
            parts.uri.path().to_owned(),
            decoded.get("scene").and_then(serde_json::Value::as_u64),
        );
        state
            .requests
            .lock()
            .unwrap()
            .push((parts.uri.path().to_owned(), parts.headers, decoded));
        if let Some(body) = state.route_responses.lock().unwrap().remove(&response_key) {
            return (StatusCode::OK, Json(body));
        }
        let (status, body) = state.responses.lock().unwrap().pop_front().unwrap();
        (status, Json(body))
    }

    async fn mock_client(
        responses: Vec<(StatusCode, serde_json::Value)>,
    ) -> (
        tempfile::TempDir,
        ImaClient,
        MockState,
        tokio::task::JoinHandle<()>,
    ) {
        let (directory, session) = test_session().await;
        let state = MockState::default();
        state.responses.lock().unwrap().extend(responses);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .fallback(post(handle))
            .with_state(state.clone());
        let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut client = ImaClient::new(session).unwrap();
        client.origin = format!("http://{address}");
        (directory, client, state, handle)
    }

    #[tokio::test]
    async fn model_mutations_use_ima_payloads_and_keep_ids_separate() {
        let (_directory, client, state, handle) = mock_client(vec![
            (StatusCode::OK, serde_json::json!({"code":0,"customize_id":"custom-owned"})),
            (StatusCode::OK, serde_json::json!({"code":0})),
            (StatusCode::OK, serde_json::json!({"code":0,"preferred_model_id":"selectable-owned"})),
            (StatusCode::OK, serde_json::json!({"code":0,"models":[{"customize_id":"custom-owned","model_name":"example"}]})),
            (StatusCode::OK, serde_json::json!({"code":0})),
            (StatusCode::OK, serde_json::json!({"code":0,"models":[]})),
        ]).await;
        let input = ImaModelInput {
            api_uri: "https://provider.example/v1/chat/completions".to_owned(),
            api_key: ImaSecret::new("fictional-provider-key"),
            model_name: "fictional-model".to_owned(),
            max_input_tokens: 8192,
            max_output_tokens: 4096,
        };
        assert_eq!(
            client
                .add_model(&input)
                .await
                .unwrap()
                .customize_id
                .as_deref(),
            Some("custom-owned")
        );
        client.modify_model("custom-owned", &input).await.unwrap();
        client
            .set_preferred_model(1, "selectable-owned")
            .await
            .unwrap();
        client.delete_model("custom-owned").await.unwrap();
        let requests = state.requests.lock().unwrap();
        assert_eq!(requests[0].0, "/cgi-bin/customize_models/add_model");
        assert_eq!(
            requests[0].2["model_info"]["api_key"],
            "fictional-provider-key"
        );
        assert_eq!(requests[1].2["model_info"]["customize_id"], "custom-owned");
        assert!(requests[1].2.get("customize_id").is_none());
        assert_eq!(
            requests[2].2,
            serde_json::json!({"scene":1,"model_id":"selectable-owned","set_if_absent":false})
        );
        assert_eq!(
            requests[4].2,
            serde_json::json!({"customize_id":"custom-owned"})
        );
        assert_eq!(requests[0].1["from_browser_ima"], "1");
        handle.abort();
    }

    #[tokio::test]
    async fn snapshot_preserves_empty_preferences_and_nested_model_choices() {
        let (_directory, client, state, handle) = mock_client(vec![]).await;
        state.route_responses.lock().unwrap().extend([
            (("/cgi-bin/customize_models/get_homepage".to_owned(),None), serde_json::json!({"code":0,"models":[{
                "customize_id":"custom-owned", "api_key":"fictional-key", "model_name":"example",
            }],"customize_model_config":{"default_input_tokens":8192}})),
            (("/cgi-bin/model_manage/get_models".to_owned(),Some(0)), serde_json::json!({"code":0,"preferred_model_id":"thoughtful-child","models":[{
                "model_id":"parent","model_type":10,"is_default":true,"sub_model_infos":[{"model_id":"thoughtful-child"}],
            }]})),
            (("/cgi-bin/model_manage/get_models".to_owned(),Some(1)), serde_json::json!({"code":0,"preferred_model_id":"","models":[{"model_id":"automatic"}]})),
        ]);
        let snapshot = client.snapshot().await.unwrap();
        assert_eq!(
            snapshot.scenes[0]
                .find("thoughtful-child")
                .unwrap()
                .model_id,
            "thoughtful-child"
        );
        assert_eq!(snapshot.scenes[1].preferred_model_id.as_deref(), Some(""));
        assert_eq!(
            snapshot.scenes[0]
                .find("thoughtful-child")
                .unwrap()
                .model_type,
            10
        );
        assert!(snapshot.homepage.models[0].model_id.is_none());
        assert!(!format!("{snapshot:?}").contains("fictional-key"));
        handle.abort();
    }

    #[tokio::test]
    async fn untrusted_error_bodies_never_escape_to_user_errors() {
        let (_directory, client, _state, handle) = mock_client(vec![
            (
                StatusCode::OK,
                serde_json::json!({"code":999,"msg":"fictional-secret-from-upstream"}),
            ),
            (
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"token":"fictional-secret-from-upstream"}),
            ),
        ])
        .await;
        let rejected = client.homepage().await.unwrap_err();
        assert_eq!(rejected.code, "ima_api_rejected");
        assert!(!format!("{rejected:?}").contains("fictional-secret"));
        let expired = client.homepage().await.unwrap_err();
        assert_eq!(expired.code, "ima_session_expired");
        assert!(!client.session.is_valid());
        assert_eq!(
            client.homepage().await.unwrap_err().code,
            "ima_session_expired"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn service_auth_refresh_errors_invalidate_cached_login_without_exposing_message() {
        let (_directory, client, _state, handle) = mock_client(vec![(
            StatusCode::OK,
            serde_json::json!({"code":1101,"msg":"fictional-sensitive-auth-message"}),
        )])
        .await;
        let error = client.homepage().await.unwrap_err();
        assert_eq!(error.code, "ima_session_expired");
        assert!(!client.session.is_valid());
        assert!(!format!("{error:?}").contains("fictional"));
        handle.abort();
    }

    #[tokio::test]
    async fn service_rate_limit_has_a_safe_retry_error() {
        let (_directory, client, _state, handle) = mock_client(vec![(
            StatusCode::OK,
            serde_json::json!({"code":100003,"msg":"fictional-sensitive-rate-message"}),
        )])
        .await;
        let error = client.homepage().await.unwrap_err();
        assert_eq!(error.code, "ima_rate_limited");
        assert!(client.session.is_valid());
        assert!(!format!("{error:?}").contains("fictional"));
        handle.abort();
    }

    #[tokio::test]
    async fn rejects_mismatched_remote_preference_and_invalid_scene_before_http() {
        let (_directory, client, state, handle) = mock_client(vec![(
            StatusCode::OK,
            serde_json::json!({"code":0,"preferred_model_id":"wrong-model"}),
        )])
        .await;
        assert_eq!(
            client.scene_models(9).await.unwrap_err().code,
            "ima_scene_invalid"
        );
        assert_eq!(
            client
                .set_preferred_model(0, "desired")
                .await
                .unwrap_err()
                .code,
            "ima_preference_rejected"
        );
        assert_eq!(state.requests.lock().unwrap().len(), 1);
        handle.abort();
    }

    #[tokio::test]
    async fn deleting_an_already_absent_owned_model_is_idempotent() {
        let (_directory, client, state, handle) = mock_client(vec![(
            StatusCode::OK,
            serde_json::json!({"code":0,"models":[]}),
        )])
        .await;
        client.delete_model("already-removed").await.unwrap();
        assert_eq!(state.requests.lock().unwrap().len(), 1);
        handle.abort();
    }

    #[tokio::test]
    async fn deletion_is_not_successful_until_the_model_disappears_on_reread() {
        let response = serde_json::json!({"code":0,"models":[{"customize_id":"owned","model_name":"example"}]});
        let (_directory, client, _state, handle) = mock_client(vec![
            (StatusCode::OK, response.clone()),
            (StatusCode::OK, serde_json::json!({"code":0})),
            (StatusCode::OK, response),
        ])
        .await;
        assert_eq!(
            client.delete_model("owned").await.unwrap_err().code,
            "ima_delete_not_applied"
        );
        handle.abort();
    }

    #[test]
    fn contract_diagnostics_only_expose_predefined_type_and_presence_labels() {
        let raw = serde_json::json!({"code":0,"unknown_private_field":"fictional-private-note",
            "models":[{"customize_id":null,"supplier_id":"fictional-supplier-id","api_key":"fictional-secret-key","model_name":"fictional-model-name","max_input_tokens":987654321}],
            "customize_model_config":{"default_input_tokens":987654321}});
        let shape = contract_response_shape(&raw, false);
        assert_eq!(
            shape["models"]["items"][0]["fields"]["customize_id"],
            "null"
        );
        assert_eq!(shape["models"]["items"][0]["fields"]["model_id"], "missing");
        assert_eq!(
            shape["models"]["items"][0]["fields"]["supplier_id"],
            "string"
        );
        assert_eq!(shape["models"]["items"][0]["fields"]["api_key"], "string");
        assert_eq!(
            shape["models"]["items"][0]["fields"]["max_input_tokens"],
            "number"
        );
        let serialized = shape.to_string();
        for forbidden in ["fictional", "987654321", "unknown_private_field"] {
            assert!(!serialized.contains(forbidden));
        }
        let scene = serde_json::json!({"models":[{"model_id":"fictional-parent","sub_model_infos":{
            "fictional-dynamic-key":{"model_id":"fictional-child","model_type":null},
            "fictional-null-key":null,"fictional-empty-key":{},
        }}]});
        let shape = contract_response_shape(&scene, true);
        assert_eq!(shape["models"]["items"][0]["sub_models"]["type"], "object");
        let values = shape["models"]["items"][0]["sub_models"]["items"]
            .as_array()
            .unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.iter().any(|item| item["type"] == "null"));
        assert!(values
            .iter()
            .any(|item| item["fields"]["model_id"] == "string"));
        assert!(!shape.to_string().contains("fictional"));
    }
}
