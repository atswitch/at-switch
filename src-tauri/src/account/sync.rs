use serde::Serialize;

use crate::domain::{ModelSummary, ProviderSummary};

/// 供应商元数据上报载荷：白名单字段（名称 / URL / 模型元数据）。
/// API Key 与任何请求/响应内容永不进入该结构（设计 §3.4）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPayload {
    pub providers: Vec<SyncProvider>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProvider {
    /// 供应商序列号（本地 UUID，稳定唯一，UI 不展示）；后端按它 upsert/删除。
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<SyncModel>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncModel {
    pub model_id: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
}

pub fn build_payload(providers: &[ProviderSummary]) -> SyncPayload {
    SyncPayload {
        providers: providers
            .iter()
            .map(|provider| SyncProvider {
                provider_id: provider.id.clone(),
                name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                models: provider.models.iter().map(model_payload).collect(),
            })
            .collect(),
    }
}

fn model_payload(model: &ModelSummary) -> SyncModel {
    use crate::domain::ModelOutputModality;
    let mut capabilities = vec!["对话".to_owned()];
    match model.output_modality {
        ModelOutputModality::Text => {}
        ModelOutputModality::Image => capabilities.push("视觉".to_owned()),
        ModelOutputModality::Audio => capabilities.push("语音".to_owned()),
        ModelOutputModality::Video => capabilities.push("视频".to_owned()),
    }
    if model.supports_tools {
        capabilities.push("工具".to_owned());
    }
    SyncModel {
        model_id: model.model_id.clone(),
        display_name: model.display_name.clone(),
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApiProtocol, ModelOutputModality, ProviderKind, VerificationStatus};

    fn provider() -> ProviderSummary {
        ProviderSummary {
            id: "p1".to_owned(),
            name: "蒙云智算".to_owned(),
            kind: ProviderKind::Mongyun,
            protocol: ApiProtocol::OpenaiChatCompletions,
            base_url: "https://api.example.com/v1".to_owned(),
            is_recommended: false,
            is_enabled: true,
            has_api_key: true,
            masked_api_key: None,
            verification_status: VerificationStatus::Verified,
            verified_model_id: None,
            default_model_id: None,
            models: vec![ModelSummary {
                id: "m1".to_owned(),
                provider_id: "p1".to_owned(),
                model_id: "glm-5.2".to_owned(),
                display_name: "GLM-5.2".to_owned(),
                output_modality: ModelOutputModality::Text,
                supports_streaming: true,
                supports_tools: true,
                source: "builtin".to_owned(),
                verification_status: VerificationStatus::Verified,
            }],
        }
    }

    #[test]
    fn payload_excludes_api_key() {
        let payload = build_payload(&[provider()]);
        let json = serde_json::to_string(&payload).expect("serialize");
        assert!(!json.to_lowercase().contains("apikey"));
        assert_eq!(payload.providers[0].models[0].model_id, "glm-5.2");
        assert!(payload.providers[0].models[0]
            .capabilities
            .contains(&"工具".to_owned()));
    }
}
