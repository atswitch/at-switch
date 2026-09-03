use std::{collections::HashSet, sync::Arc};

use reqwest::{header, Client, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    domain::{
        ApiProtocol, AppResult, CommandError, ModelDraft, ModelSummary, ProviderDraft,
        ProviderSummary, VerificationStatus,
    },
    infrastructure::{Database, SecretStore, SecretValue},
};

pub struct ProviderService {
    database: Arc<Database>,
    secret_store: Arc<dyn SecretStore>,
    http: Client,
}

impl ProviderService {
    pub fn new(database: Arc<Database>, secret_store: Arc<dyn SecretStore>) -> AppResult<Self> {
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(45))
            .user_agent(format!("AT-Switch/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                log::error!("HTTP client initialization failed: {error}");
                CommandError::internal("无法初始化 Provider 网络客户端")
            })?;
        Ok(Self {
            database,
            secret_store,
            http,
        })
    }

    pub fn list(&self) -> AppResult<Vec<ProviderSummary>> {
        let mut providers = self.database.list_providers()?;
        for provider in &mut providers {
            let metadata = self.database.provider_secret_metadata(&provider.id)?;
            // Listing Providers must never unlock Keychain/Credential Manager.
            // The credential itself is checked when the user tests or applies
            // the Provider, where an authorization prompt is expected.
            provider.has_api_key = metadata.reference.is_some();
        }
        Ok(providers)
    }

    pub fn masked_api_key(&self, provider_id: &str) -> AppResult<String> {
        let secret = self.provider_api_key(provider_id)?;
        Ok(mask_secret(secret.expose()))
    }

    pub fn reveal_api_key(&self, provider_id: &str) -> AppResult<String> {
        let secret = self.provider_api_key(provider_id)?;
        Ok(secret.expose().to_owned())
    }

    pub fn delete(&self, provider_id: &str) -> AppResult<()> {
        let (secret_ref, token_refs) = self.database.delete_provider(provider_id)?;
        if let Some(reference) = secret_ref {
            if let Err(error) = self.secret_store.delete(&reference) {
                log::warn!("Provider credential cleanup was deferred: {error}");
            }
        }
        for token_ref in token_refs {
            if let Err(error) = self.secret_store.delete(&token_ref) {
                log::warn!("Local token cleanup was deferred: {error}");
            }
        }
        Ok(())
    }

    pub fn save(&self, mut draft: ProviderDraft) -> AppResult<ProviderSummary> {
        validate_provider_draft(&draft)?;
        let mut duplicate_provider_ids = Vec::new();
        let mut duplicate_secret_refs = Vec::new();
        let provider_id = if let Some(id) = draft.id.clone() {
            id
        } else {
            let matches = self
                .database
                .list_providers()?
                .into_iter()
                .filter(|provider| provider_identity_matches(provider, &draft))
                .collect::<Vec<_>>();
            if let Some(canonical) = matches.first() {
                let mut merged_models = Vec::new();
                for provider in &matches {
                    merge_model_catalog(&mut merged_models, &provider.models);
                }
                merge_model_drafts(&mut merged_models, std::mem::take(&mut draft.models));
                draft.models = merged_models;
                draft.default_model_id = canonical
                    .default_model_id
                    .clone()
                    .or(draft.default_model_id);
                draft.id = Some(canonical.id.clone());
                for duplicate in matches.iter().skip(1) {
                    duplicate_provider_ids.push(duplicate.id.clone());
                    if let Some(reference) = self
                        .database
                        .provider_secret_metadata(&duplicate.id)?
                        .reference
                    {
                        duplicate_secret_refs.push(reference);
                    }
                }
                canonical.id.clone()
            } else {
                Uuid::new_v4().to_string()
            }
        };
        validate_provider_draft(&draft)?;
        validate_identifier(&provider_id)?;

        let previous = self.database.provider_secret_metadata(&provider_id)?;
        let mut next_reference = previous.reference.clone();
        let mut next_revision = previous.revision;
        let mut next_masked = previous.masked.clone();
        let mut newly_created_reference = None;
        let submitted_api_key = draft.api_key.as_deref().filter(|value| !value.is_empty());
        let submitted_key_is_unchanged = submitted_api_key.is_some_and(|api_key| {
            previous
                .reference
                .as_deref()
                .and_then(|reference| self.secret_store.get(reference).ok())
                .is_some_and(|secret| secret.expose() == api_key)
        });

        if let Some(api_key) = submitted_api_key.filter(|_| !submitted_key_is_unchanged) {
            next_revision += 1;
            let reference = format!("provider/{provider_id}/api-key/v{next_revision}");
            self.secret_store
                .put(&reference, &SecretValue::new(api_key.to_owned()))?;
            next_masked = Some(mask_secret(api_key));
            next_reference = Some(reference.clone());
            newly_created_reference = Some(reference);
        } else if let Some(api_key) = submitted_api_key {
            next_masked = Some(mask_secret(api_key));
        }

        if submitted_api_key.is_none()
            && !previous
                .reference
                .as_deref()
                .is_some_and(|reference| self.secret_store.exists(reference))
        {
            return Err(CommandError::new("credential_missing", "请填写 API Key")
                .with_recovery("系统凭据库中没有可继续使用的密钥。"));
        }

        if let Err(error) = self.database.save_provider_merging(
            &provider_id,
            &draft,
            next_reference.as_deref(),
            next_revision,
            next_masked.as_deref(),
            &duplicate_provider_ids,
        ) {
            if let Some(reference) = newly_created_reference {
                let _ = self.secret_store.delete(&reference);
            }
            return Err(error);
        }

        if previous.reference != next_reference {
            if let Some(reference) = previous.reference {
                if let Err(error) = self.secret_store.delete(&reference) {
                    log::warn!("old credential cleanup was deferred: {error}");
                }
            }
        }
        for reference in duplicate_secret_refs {
            if next_reference.as_deref() == Some(reference.as_str()) {
                continue;
            }
            if let Err(error) = self.secret_store.delete(&reference) {
                log::warn!("merged Provider credential cleanup was deferred: {error}");
            }
        }

        Ok(self.database.get_provider(&provider_id)?.summary)
    }

    fn provider_api_key(&self, provider_id: &str) -> AppResult<SecretValue> {
        let provider = self.database.get_provider(provider_id)?;
        let reference = provider.api_key_ref.as_deref().ok_or_else(|| {
            CommandError::new("secret_missing", "该模型供应商尚未保存 API Key")
                .with_recovery("请填写并保存 API Key。")
        })?;
        self.secret_store.get(reference)
    }

    pub async fn test(
        &self,
        provider_id: &str,
        requested_model_id: Option<&str>,
    ) -> AppResult<ProviderSummary> {
        let provider = self.database.get_provider(provider_id)?;
        let requested_model = requested_model_id
            .map(|model_id| {
                provider
                    .summary
                    .models
                    .iter()
                    .find(|model| model.model_id == model_id)
                    .ok_or_else(|| {
                        CommandError::new("model_not_found", "所选模型不属于该 Provider")
                    })
            })
            .transpose()?;
        if requested_model
            .as_ref()
            .is_some_and(|model| !model.output_modality.requires_verification())
        {
            return Err(CommandError::new(
                "model_verification_not_required",
                "生图、语音和视频模型无需连接测试",
            ));
        }
        let model = requested_model
            .or_else(|| {
                provider
                    .summary
                    .default_model_id
                    .as_deref()
                    .and_then(|model_id| {
                        provider.summary.models.iter().find(|model| {
                            model.model_id == model_id
                                && model.output_modality.requires_verification()
                        })
                    })
            })
            .or_else(|| {
                provider
                    .summary
                    .models
                    .iter()
                    .find(|model| model.output_modality.requires_verification())
            })
            .cloned()
            .ok_or_else(|| {
                CommandError::new(
                    "model_verification_not_required",
                    "当前 Provider 没有需要连接测试的文本模型",
                )
            })?;
        let secret_ref = provider.api_key_ref.as_deref().ok_or_else(|| {
            CommandError::new("secret_missing", "请先保存 API Key")
                .with_recovery("编辑 Provider 并填写 API Key。")
        })?;
        let secret = self.secret_store.get(secret_ref)?;

        self.database.mark_model_verification(
            provider_id,
            &model.model_id,
            VerificationStatus::Verifying,
            None,
        )?;

        let capabilities = provider.summary.protocol_capabilities();
        let result = async {
            for protocol in capabilities.supported() {
                if let Err(mut error) = self
                    .perform_test_request(
                        *protocol,
                        &provider.summary.base_url,
                        &model.model_id,
                        model.supports_streaming,
                        model.supports_tools,
                        secret.expose(),
                    )
                    .await
                {
                    error.message =
                        format!("{} 协议验证失败：{}", protocol.as_str(), error.message);
                    return Err(error);
                }
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                let fingerprint = model_verification_fingerprint(
                    &provider.summary.base_url,
                    capabilities.supported(),
                    &model.model_id,
                    provider.api_key_revision,
                );
                self.database.mark_model_verification(
                    provider_id,
                    &model.model_id,
                    VerificationStatus::Verified,
                    Some(&fingerprint),
                )?;
                Ok(self.database.get_provider(provider_id)?.summary)
            }
            Err(error) => {
                self.database.mark_model_verification(
                    provider_id,
                    &model.model_id,
                    VerificationStatus::Failed,
                    None,
                )?;
                Err(error)
            }
        }
    }

    async fn perform_test_request(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        model: &str,
        supports_streaming: bool,
        supports_tools: bool,
        api_key: &str,
    ) -> AppResult<()> {
        self.perform_text_request(protocol, base_url, model, api_key)
            .await?;
        if supports_streaming {
            self.perform_streaming_request(protocol, base_url, model, api_key)
                .await?;
        }
        if supports_tools {
            self.perform_tool_request(protocol, base_url, model, api_key)
                .await?;
        }
        Ok(())
    }

    async fn perform_text_request(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        model: &str,
        api_key: &str,
    ) -> AppResult<()> {
        let (endpoint, body) = match protocol {
            ApiProtocol::OpenaiChatCompletions => (
                "chat/completions",
                json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Reply with exactly: AT-Switch ready"}],
                    "max_tokens": 512,
                    "stream": false
                }),
            ),
            ApiProtocol::OpenaiResponses => (
                "responses",
                json!({
                    "model": model,
                    "input": "Reply with exactly: AT-Switch ready",
                    "max_output_tokens": 512,
                    "stream": false
                }),
            ),
            ApiProtocol::AnthropicMessages => (
                "messages",
                json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Reply with exactly: AT-Switch ready"}],
                    "max_tokens": 512,
                    "stream": false
                }),
            ),
        };
        let value = self
            .send_json(protocol, base_url, endpoint, body, api_key)
            .await?;
        if response_has_text(protocol, &value) {
            Ok(())
        } else {
            Err(CommandError::new(
                "provider_text_response_invalid",
                "Provider 已响应，但没有返回可识别的文本内容",
            )
            .with_recovery("检查 API 协议和模型 ID 是否与上游实际接口一致。"))
        }
    }

    async fn perform_streaming_request(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        model: &str,
        api_key: &str,
    ) -> AppResult<()> {
        let (endpoint, body) = match protocol {
            ApiProtocol::OpenaiChatCompletions => (
                "chat/completions",
                json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Say stream-ready."}],
                    "max_tokens": 512,
                    "stream": true
                }),
            ),
            ApiProtocol::OpenaiResponses => (
                "responses",
                json!({
                    "model": model,
                    "input": "Say stream-ready.",
                    "max_output_tokens": 512,
                    "stream": true
                }),
            ),
            ApiProtocol::AnthropicMessages => (
                "messages",
                json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Say stream-ready."}],
                    "max_tokens": 512,
                    "stream": true
                }),
            ),
        };
        let response = self
            .send(protocol, base_url, endpoint, body, api_key)
            .await?;
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = response.text().await.map_err(classify_network_error)?;
        if content_type.contains("text/event-stream") && sse_has_model_event(protocol, &body) {
            Ok(())
        } else {
            Err(CommandError::new(
                "provider_streaming_unsupported",
                "模型声明支持 Streaming，但连接测试没有收到有效 SSE 数据",
            )
            .with_recovery("若该模型实际不支持流式输出，请在 Provider 模型配置中取消 Streaming。"))
        }
    }

    async fn perform_tool_request(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        model: &str,
        api_key: &str,
    ) -> AppResult<()> {
        let parameters = json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
            "additionalProperties": false
        });
        let (endpoint, body) = match protocol {
            ApiProtocol::OpenaiChatCompletions => (
                "chat/completions",
                json!({
                    "model": model,
                    "messages": [{
                        "role": "user",
                        "content": "Call the at_switch_echo tool with text \"tool-ready\". Do not answer directly."
                    }],
                    "tools": [{
                        "type": "function",
                        "function": {
                            "name": "at_switch_echo",
                            "description": "Deterministic AT-Switch capability check",
                            "parameters": parameters
                        }
                    }],
                    "tool_choice": "auto",
                    "max_tokens": 512,
                    "stream": false
                }),
            ),
            ApiProtocol::OpenaiResponses => (
                "responses",
                json!({
                    "model": model,
                    "input": "Call the at_switch_echo tool with text \"tool-ready\".",
                    "tools": [{
                        "type": "function",
                        "name": "at_switch_echo",
                        "description": "Deterministic AT-Switch capability check",
                        "parameters": parameters
                    }],
                    "tool_choice": "auto",
                    "max_output_tokens": 512,
                    "stream": false
                }),
            ),
            ApiProtocol::AnthropicMessages => (
                "messages",
                json!({
                    "model": model,
                    "messages": [{
                        "role": "user",
                        "content": "Call the at_switch_echo tool with text \"tool-ready\"."
                    }],
                    "tools": [{
                        "name": "at_switch_echo",
                        "description": "Deterministic AT-Switch capability check",
                        "input_schema": parameters
                    }],
                    "tool_choice": {"type": "auto"},
                    "max_tokens": 512,
                    "stream": false
                }),
            ),
        };
        let value = self
            .send_json(protocol, base_url, endpoint, body, api_key)
            .await?;
        if response_has_tool_call(protocol, &value, "at_switch_echo") {
            Ok(())
        } else {
            Err(CommandError::new(
                "provider_tools_unsupported",
                "模型声明支持 Tool，但没有返回要求的函数调用",
            )
            .with_recovery("确认上游支持工具调用；否则请在 Provider 模型配置中取消 Tool。"))
        }
    }

    async fn send_json(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        endpoint: &str,
        body: Value,
        api_key: &str,
    ) -> AppResult<Value> {
        let response = self
            .send(protocol, base_url, endpoint, body, api_key)
            .await?;
        response.json::<Value>().await.map_err(|error| {
            log::warn!("provider returned invalid JSON: {error}");
            CommandError::new(
                "provider_response_invalid",
                "Provider 返回内容不是当前协议的有效 JSON",
            )
        })
    }

    async fn send(
        &self,
        protocol: ApiProtocol,
        base_url: &str,
        endpoint: &str,
        body: Value,
        api_key: &str,
    ) -> AppResult<reqwest::Response> {
        let url = endpoint_url(base_url, endpoint)?;
        let request = self
            .http
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body);
        let request = match protocol {
            ApiProtocol::AnthropicMessages => request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01"),
            _ => request.bearer_auth(api_key),
        };
        let response = request.send().await.map_err(classify_network_error)?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(safe_http_error(response.status()))
        }
    }
}

fn response_has_text(protocol: ApiProtocol, value: &Value) -> bool {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => {
            value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
                || value
                    .pointer("/choices/0/message/reasoning_content")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
        }
        ApiProtocol::OpenaiResponses => {
            value
                .get("output_text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
                || value
                    .get("output")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("content")
                                .and_then(Value::as_array)
                                .is_some_and(|content| {
                                    content.iter().any(|part| {
                                        part.get("text")
                                            .and_then(Value::as_str)
                                            .is_some_and(|text| !text.trim().is_empty())
                                    })
                                })
                        })
                    })
        }
        ApiProtocol::AnthropicMessages => value
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("text")
                        && part
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty())
                })
            }),
    }
}

fn response_has_tool_call(protocol: ApiProtocol, value: &Value, tool_name: &str) -> bool {
    match protocol {
        ApiProtocol::OpenaiChatCompletions => value
            .pointer("/choices/0/message/tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| {
                calls.iter().any(|call| {
                    call.pointer("/function/name").and_then(Value::as_str) == Some(tool_name)
                })
            }),
        ApiProtocol::OpenaiResponses => {
            value
                .get("output")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                            && item.get("name").and_then(Value::as_str) == Some(tool_name)
                    })
                })
        }
        ApiProtocol::AnthropicMessages => value
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("tool_use")
                        && part.get("name").and_then(Value::as_str) == Some(tool_name)
                })
            }),
    }
}

fn sse_has_model_event(protocol: ApiProtocol, body: &str) -> bool {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .filter(|data| !data.is_empty() && *data != "[DONE]")
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .any(|value| match protocol {
            ApiProtocol::OpenaiChatCompletions => value
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|choices| !choices.is_empty()),
            ApiProtocol::OpenaiResponses => value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("response.") && kind != "response.created"),
            ApiProtocol::AnthropicMessages => value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "content_block_start"
                            | "content_block_delta"
                            | "message_delta"
                            | "message_stop"
                    )
                }),
        })
}

fn provider_identity_matches(provider: &ProviderSummary, draft: &ProviderDraft) -> bool {
    provider.name.trim().to_lowercase() == draft.name.trim().to_lowercase()
        && normalized_provider_url(&provider.base_url) == normalized_provider_url(&draft.base_url)
}

fn normalized_provider_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value.trim()).ok()?;
    url.set_fragment(None);
    let normalized_path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if normalized_path.is_empty() {
        "/"
    } else {
        &normalized_path
    });
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn merge_model_catalog(target: &mut Vec<ModelDraft>, models: &[ModelSummary]) {
    merge_model_drafts(
        target,
        models
            .iter()
            .map(|model| ModelDraft {
                model_id: model.model_id.clone(),
                display_name: model.display_name.clone(),
                output_modality: model.output_modality,
                supports_streaming: model.supports_streaming,
                supports_tools: model.supports_tools,
            })
            .collect(),
    );
}

fn merge_model_drafts(target: &mut Vec<ModelDraft>, models: Vec<ModelDraft>) {
    for model in models {
        if let Some(index) = target
            .iter()
            .position(|existing| existing.model_id.trim() == model.model_id.trim())
        {
            target[index] = model;
        } else {
            target.push(model);
        }
    }
}

fn validate_provider_draft(draft: &ProviderDraft) -> AppResult<()> {
    if draft.name.trim().is_empty() || draft.name.chars().count() > 80 {
        return Err(CommandError::new(
            "provider_name_invalid",
            "Provider 名称不能为空且不能超过 80 个字符",
        ));
    }
    if draft.models.len() > 200 {
        return Err(CommandError::new(
            "too_many_models",
            "单个 Provider 最多保存 200 个模型",
        ));
    }
    let mut model_ids = HashSet::with_capacity(draft.models.len());
    for model in &draft.models {
        let model_id = model.model_id.trim();
        if model_id.is_empty() || model_id.chars().count() > 160 {
            return Err(CommandError::new(
                "model_id_invalid",
                "模型 ID 不能为空且不能超过 160 个字符",
            ));
        }
        if !model_ids.insert(model_id) {
            return Err(CommandError::new(
                "model_id_duplicate",
                format!("模型 ID `{model_id}` 重复"),
            ));
        }
    }
    if let Some(default_model_id) = draft.default_model_id.as_deref() {
        if !model_ids.contains(default_model_id.trim()) {
            return Err(CommandError::new(
                "default_model_missing",
                "默认模型必须存在于当前 Provider 的模型列表中",
            ));
        }
    }
    let url = Url::parse(draft.base_url.trim())
        .map_err(|_| CommandError::new("base_url_invalid", "Base URL 格式无效"))?;
    if url.host_str().is_none() {
        return Err(CommandError::new(
            "base_url_host_missing",
            "Base URL 必须包含主机名",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CommandError::new(
            "base_url_credentials_forbidden",
            "Base URL 不能包含用户名或密码",
        ));
    }
    match url.scheme() {
        "https" | "http" => {}
        _ => {
            return Err(CommandError::new(
                "base_url_scheme_invalid",
                "Base URL 只支持 HTTP 或 HTTPS",
            ))
        }
    }
    Ok(())
}

fn validate_identifier(identifier: &str) -> AppResult<()> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CommandError::new("provider_id_invalid", "Provider ID 无效"));
    }
    Ok(())
}

fn mask_secret(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    if characters.len() <= 4 {
        return "•".repeat(characters.len());
    }
    let hidden = "•".repeat(characters.len() - 4);
    let suffix = characters[characters.len() - 4..]
        .iter()
        .collect::<String>();
    format!("{hidden}{suffix}")
}

pub(crate) fn endpoint_url(base_url: &str, endpoint: &str) -> AppResult<Url> {
    let mut normalized = base_url.trim().trim_end_matches('/').to_owned();
    normalized.push('/');
    let base = Url::parse(&normalized)
        .map_err(|_| CommandError::new("base_url_invalid", "Base URL 格式无效"))?;
    base.join(endpoint)
        .map_err(|_| CommandError::new("endpoint_invalid", "无法构造 Provider 请求地址"))
}

fn model_verification_fingerprint(
    base_url: &str,
    protocols: &[ApiProtocol],
    model: &str,
    secret_revision: i64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base_url.trim().as_bytes());
    for protocol in protocols {
        hasher.update([0]);
        hasher.update(protocol.as_str().as_bytes());
    }
    hasher.update([0]);
    hasher.update(model.as_bytes());
    hasher.update([0]);
    hasher.update(secret_revision.to_le_bytes());
    hex::encode(hasher.finalize())
}

fn classify_network_error(error: reqwest::Error) -> CommandError {
    if error.is_timeout() || error.is_connect() {
        CommandError::new("network_unreachable", "无法连接到 Provider")
            .with_recovery("检查网络、Base URL、防火墙或代理设置后重试。")
    } else {
        log::warn!("provider test transport error: {error}");
        CommandError::new("provider_transport_failed", "Provider 请求未完成")
            .with_recovery("请检查 Provider Endpoint 与网络设置。")
    }
}

fn safe_http_error(status: StatusCode) -> CommandError {
    let (code, message, recovery) = match status {
        StatusCode::UNAUTHORIZED => (
            "provider_unauthorized",
            "Provider 拒绝了 API Key",
            "检查 API Key 是否正确、有效并属于当前 Endpoint。",
        ),
        StatusCode::FORBIDDEN => (
            "provider_forbidden",
            "当前凭据没有访问目标模型的权限",
            "检查模型权限、套餐或 Provider 控制台设置。",
        ),
        StatusCode::NOT_FOUND => (
            "provider_not_found",
            "Provider Endpoint 或模型不存在",
            "检查 Base URL 是否包含正确版本路径，以及模型 ID 是否准确。",
        ),
        StatusCode::TOO_MANY_REQUESTS => (
            "provider_rate_limited",
            "Provider 当前触发限流",
            "稍后重试；限流状态不能标记为连接已验证。",
        ),
        status if status.is_server_error() => (
            "provider_unavailable",
            "Provider 服务暂时不可用",
            "稍后重试；首版不会自动切换到其他 Provider。",
        ),
        _ => (
            "provider_request_rejected",
            "Provider 拒绝了连接测试请求",
            "检查协议、模型和 Endpoint 配置。",
        ),
    };
    CommandError::new(code, message).with_recovery(recovery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ModelDraft, ModelOutputModality, ProviderKind};
    use crate::infrastructure::{Database, MemorySecretStore};
    use std::sync::Arc;

    fn draft(url: &str) -> ProviderDraft {
        ProviderDraft {
            id: None,
            name: "Test".to_owned(),
            kind: ProviderKind::Custom,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: url.to_owned(),
            api_key: Some("secret".to_owned()),
            default_model_id: Some("model".to_owned()),
            models: vec![ModelDraft {
                model_id: "model".to_owned(),
                display_name: "Model".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
            }],
            allow_insecure_http: false,
        }
    }

    #[test]
    fn accepts_plain_http_urls() {
        assert!(validate_provider_draft(&draft("http://192.168.1.20/v1")).is_ok());
    }

    #[test]
    fn joins_protocol_endpoint_without_dropping_v1() {
        let url = endpoint_url("https://api.example.test/v1", "responses").expect("url");
        assert_eq!(url.as_str(), "https://api.example.test/v1/responses");
    }

    #[test]
    fn mask_keeps_only_the_last_four_characters() {
        assert_eq!(mask_secret("sk-123456789"), "••••••••6789");
        assert_eq!(mask_secret("abc"), "•••");
    }

    #[test]
    fn explicit_reveal_reads_the_full_key_without_adding_it_to_provider_dto() {
        let database = Arc::new(Database::in_memory().expect("database"));
        let secret_store = Arc::new(MemorySecretStore::default());
        let service = ProviderService::new(database, secret_store).expect("service");
        let mut stored = draft("https://api.example.test/v1");
        stored.id = Some("provider-reveal".to_owned());
        stored.api_key = Some("sk-test-secret-1234".to_owned());
        let summary = service.save(stored).expect("save provider");

        let masked = service
            .masked_api_key("provider-reveal")
            .expect("masked key");
        assert_eq!(
            masked.chars().count(),
            "sk-test-secret-1234".chars().count()
        );
        assert!(masked.ends_with("1234"));
        assert_eq!(
            service
                .reveal_api_key("provider-reveal")
                .expect("revealed key"),
            "sk-test-secret-1234"
        );
        assert_ne!(
            summary.masked_api_key.as_deref(),
            Some("sk-test-secret-1234")
        );
    }

    #[test]
    fn rejects_duplicate_model_ids_before_database_write() {
        let mut invalid = draft("https://api.example.test/v1");
        invalid.models.push(invalid.models[0].clone());
        let error = validate_provider_draft(&invalid).expect_err("must fail");
        assert_eq!(error.code, "model_id_duplicate");
    }

    #[test]
    fn rejects_a_default_model_outside_the_model_list() {
        let mut invalid = draft("https://api.example.test/v1");
        invalid.default_model_id = Some("not-configured".to_owned());
        let error = validate_provider_draft(&invalid).expect_err("must fail");
        assert_eq!(error.code, "default_model_missing");
    }

    #[test]
    fn list_uses_credential_metadata_without_unlocking_the_secret_store() {
        let database = Arc::new(Database::in_memory().expect("database"));
        let mut stored = draft("https://api.example.test/v1");
        stored.id = Some("provider-1".to_owned());
        database
            .save_provider(
                "provider-1",
                &stored,
                Some("provider/provider-1/api-key/v1"),
                1,
                Some("••••cret"),
            )
            .expect("save metadata");

        let service = ProviderService::new(database, Arc::new(MemorySecretStore::default()))
            .expect("service");
        let providers = service.list().expect("providers");
        let provider = providers
            .iter()
            .find(|provider| provider.id == "provider-1")
            .expect("provider");

        assert!(provider.has_api_key);
        assert_eq!(provider.masked_api_key.as_deref(), Some("••••cret"));
        assert_eq!(
            provider.verification_status,
            VerificationStatus::DraftUnverified
        );
    }

    #[test]
    fn selected_default_model_survives_save_and_reload() {
        let database = Arc::new(Database::in_memory().expect("database"));
        let service = ProviderService::new(
            Arc::clone(&database),
            Arc::new(MemorySecretStore::default()),
        )
        .expect("service");
        let mut stored = draft("https://api.example.test/v1");
        stored.id = Some("provider-2".to_owned());
        stored.models.push(ModelDraft {
            model_id: "model-b".to_owned(),
            display_name: "Model B".to_owned(),
            output_modality: ModelOutputModality::Text,
            supports_streaming: true,
            supports_tools: false,
        });
        stored.default_model_id = Some("model-b".to_owned());

        service.save(stored).expect("save");
        let provider = service
            .list()
            .expect("providers")
            .into_iter()
            .find(|provider| provider.id == "provider-2")
            .expect("provider");

        assert_eq!(provider.default_model_id.as_deref(), Some("model-b"));
        assert_eq!(provider.models.len(), 2);
        assert!(provider.has_api_key);
    }

    #[test]
    fn creating_the_same_provider_identity_merges_model_catalogs() {
        let database = Arc::new(Database::in_memory().expect("database"));
        let service = ProviderService::new(
            Arc::clone(&database),
            Arc::new(MemorySecretStore::default()),
        )
        .expect("service");
        let first = service
            .save(draft("https://api.example.test/v1"))
            .expect("first save");
        database
            .mark_model_verification(
                &first.id,
                "model",
                VerificationStatus::Verified,
                Some("fingerprint"),
            )
            .expect("verify existing model");

        let mut second = draft("https://api.example.test/v1/");
        second.name = " test ".to_owned();
        second.models = vec![ModelDraft {
            model_id: "model-b".to_owned(),
            display_name: "Model B".to_owned(),
            output_modality: ModelOutputModality::Image,
            supports_streaming: false,
            supports_tools: false,
        }];
        second.default_model_id = Some("model-b".to_owned());
        let merged = service.save(second).expect("merged save");

        assert_eq!(merged.id, first.id);
        assert_eq!(
            merged.models[0].verification_status,
            VerificationStatus::Verified
        );
        assert_eq!(
            merged
                .models
                .iter()
                .map(|model| model.model_id.as_str())
                .collect::<Vec<_>>(),
            vec!["model", "model-b"]
        );
        assert_eq!(
            service
                .list()
                .expect("providers")
                .into_iter()
                .filter(|provider| {
                    provider.name.trim().eq_ignore_ascii_case("test")
                        && provider.base_url.trim_end_matches('/') == "https://api.example.test/v1"
                })
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn non_text_models_do_not_run_connectivity_tests() {
        let database = Arc::new(Database::in_memory().expect("database"));
        let service = ProviderService::new(database, Arc::new(MemorySecretStore::default()))
            .expect("service");
        let mut image_provider = draft("https://media.example.test/v1");
        image_provider.models[0].output_modality = ModelOutputModality::Image;
        image_provider.models[0].supports_streaming = false;
        image_provider.models[0].supports_tools = false;
        let saved = service.save(image_provider).expect("save image provider");

        let error = service
            .test(&saved.id, Some("model"))
            .await
            .expect_err("image model must not be tested");

        assert_eq!(error.code, "model_verification_not_required");
        assert_eq!(
            service
                .list()
                .expect("providers")
                .into_iter()
                .find(|provider| provider.id == saved.id)
                .expect("image provider")
                .models[0]
                .verification_status,
            VerificationStatus::DraftUnverified
        );
    }

    #[test]
    fn recognizes_tool_calls_for_all_supported_protocols() {
        let chat = json!({
            "choices": [{"message": {"tool_calls": [{
                "function": {"name": "at_switch_echo", "arguments": "{\"text\":\"tool-ready\"}"}
            }]}}]
        });
        let responses = json!({
            "output": [{"type": "function_call", "name": "at_switch_echo"}]
        });
        let anthropic = json!({
            "content": [{"type": "tool_use", "name": "at_switch_echo"}]
        });
        assert!(response_has_tool_call(
            ApiProtocol::OpenaiChatCompletions,
            &chat,
            "at_switch_echo"
        ));
        assert!(response_has_tool_call(
            ApiProtocol::OpenaiResponses,
            &responses,
            "at_switch_echo"
        ));
        assert!(response_has_tool_call(
            ApiProtocol::AnthropicMessages,
            &anthropic,
            "at_switch_echo"
        ));
    }

    #[test]
    fn streaming_check_requires_a_real_protocol_event() {
        let valid = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"ready\"}\n\n";
        let invalid = "data: [DONE]\n\n";
        assert!(sse_has_model_event(ApiProtocol::OpenaiResponses, valid));
        assert!(!sse_has_model_event(ApiProtocol::OpenaiResponses, invalid));
    }
}
