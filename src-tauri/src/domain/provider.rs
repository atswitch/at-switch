use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    OpenaiChatCompletions,
    OpenaiResponses,
    AnthropicMessages,
}

impl ApiProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiChatCompletions => "openai_chat_completions",
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai_chat_completions" => Some(Self::OpenaiChatCompletions),
            "openai_responses" => Some(Self::OpenaiResponses),
            "anthropic_messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Mongyun,
    Deepseek,
    Minimax,
    Kimi,
    Zhipu,
    Qwen,
    Doubao,
    Custom,
}

/// Protocols that one Provider endpoint can accept.
///
/// `fallback` remains the user-configured compatibility protocol. When the
/// Agent's native protocol is also advertised, routing selects that exact
/// protocol and avoids conversion. Adding Anthropic (or another protocol)
/// therefore extends this profile instead of adding Agent-specific branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProtocolCapabilities {
    fallback: ApiProtocol,
    supported: Vec<ApiProtocol>,
}

impl ProviderProtocolCapabilities {
    pub fn for_provider(kind: ProviderKind, configured: ApiProtocol) -> Self {
        let mut supported = match kind {
            // Mongyun currently exposes both OpenAI-compatible endpoints under
            // the same Base URL and API Key.
            ProviderKind::Mongyun => vec![
                ApiProtocol::OpenaiChatCompletions,
                ApiProtocol::OpenaiResponses,
            ],
            _ => vec![configured],
        };
        if !supported.contains(&configured) {
            supported.push(configured);
        }
        Self {
            fallback: configured,
            supported,
        }
    }

    pub fn supported(&self) -> &[ApiProtocol] {
        &self.supported
    }

    pub fn upstream_for(&self, agent_protocol: ApiProtocol) -> ApiProtocol {
        if self.supported.contains(&agent_protocol) {
            agent_protocol
        } else {
            self.fallback
        }
    }
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mongyun => "mongyun",
            Self::Deepseek => "deepseek",
            Self::Minimax => "minimax",
            Self::Kimi => "kimi",
            Self::Zhipu => "zhipu",
            Self::Qwen => "qwen",
            Self::Doubao => "doubao",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mongyun" => Some(Self::Mongyun),
            "deepseek" => Some(Self::Deepseek),
            "minimax" => Some(Self::Minimax),
            "kimi" => Some(Self::Kimi),
            "zhipu" => Some(Self::Zhipu),
            "qwen" => Some(Self::Qwen),
            "doubao" => Some(Self::Doubao),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    DraftUnverified,
    Verifying,
    Verified,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOutputModality {
    #[default]
    Text,
    Image,
    Audio,
    Video,
}

impl ModelOutputModality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => Self::Text,
        }
    }

    pub fn requires_verification(self) -> bool {
        matches!(self, Self::Text)
    }
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DraftUnverified => "draft_unverified",
            Self::Verifying => "verifying",
            Self::Verified => "verified",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "verifying" => Self::Verifying,
            "verified" => Self::Verified,
            "stale" => Self::Stale,
            "failed" => Self::Failed,
            _ => Self::DraftUnverified,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDraft {
    pub model_id: String,
    pub display_name: String,
    #[serde(default)]
    pub output_modality: ModelOutputModality,
    pub supports_streaming: bool,
    pub supports_tools: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub kind: ProviderKind,
    pub protocol: ApiProtocol,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub default_model_id: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelDraft>,
    #[serde(default)]
    pub allow_insecure_http: bool,
}

impl std::fmt::Debug for ProviderDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderDraft")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("protocol", &self.protocol)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("default_model_id", &self.default_model_id)
            .field("models", &self.models)
            .field("allow_insecure_http", &self.allow_insecure_http)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub output_modality: ModelOutputModality,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub source: String,
    pub verification_status: VerificationStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub protocol: ApiProtocol,
    pub base_url: String,
    pub is_recommended: bool,
    pub is_enabled: bool,
    pub has_api_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_api_key: Option<String>,
    pub verification_status: VerificationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model_id: Option<String>,
    pub models: Vec<ModelSummary>,
}

impl ProviderSummary {
    pub fn protocol_capabilities(&self) -> ProviderProtocolCapabilities {
        ProviderProtocolCapabilities::for_provider(self.kind, self.protocol)
    }

    pub fn upstream_protocol_for(&self, agent_protocol: ApiProtocol) -> ApiProtocol {
        self.protocol_capabilities().upstream_for(agent_protocol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mongyun_matches_chat_and_responses_without_conversion() {
        let capabilities = ProviderProtocolCapabilities::for_provider(
            ProviderKind::Mongyun,
            ApiProtocol::OpenaiChatCompletions,
        );

        assert_eq!(
            capabilities.upstream_for(ApiProtocol::OpenaiChatCompletions),
            ApiProtocol::OpenaiChatCompletions
        );
        assert_eq!(
            capabilities.upstream_for(ApiProtocol::OpenaiResponses),
            ApiProtocol::OpenaiResponses
        );
    }

    #[test]
    fn configured_anthropic_protocol_extends_the_same_matcher() {
        let capabilities = ProviderProtocolCapabilities::for_provider(
            ProviderKind::Mongyun,
            ApiProtocol::AnthropicMessages,
        );

        assert!(capabilities
            .supported()
            .contains(&ApiProtocol::AnthropicMessages));
        assert_eq!(
            capabilities.upstream_for(ApiProtocol::AnthropicMessages),
            ApiProtocol::AnthropicMessages
        );
    }

    #[test]
    fn single_protocol_providers_fall_back_to_conversion() {
        let capabilities = ProviderProtocolCapabilities::for_provider(
            ProviderKind::Custom,
            ApiProtocol::OpenaiChatCompletions,
        );

        assert_eq!(
            capabilities.upstream_for(ApiProtocol::OpenaiResponses),
            ApiProtocol::OpenaiChatCompletions
        );
    }

    #[test]
    fn only_text_models_require_connectivity_verification() {
        assert!(ModelOutputModality::Text.requires_verification());
        assert!(!ModelOutputModality::Image.requires_verification());
        assert!(!ModelOutputModality::Audio.requires_verification());
        assert!(!ModelOutputModality::Video.requires_verification());
    }
}
