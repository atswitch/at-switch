use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Only serialize into the encrypted configuration transaction or ima's HTTPS
/// request. Never return this type through an IPC command or write it to SQLite.
#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ImaSecret(String);

impl ImaSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ImaSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ImaSecret([REDACTED])")
    }
}

impl Drop for ImaSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaModelInput {
    pub api_uri: String,
    pub api_key: ImaSecret,
    pub model_name: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaModel {
    pub customize_id: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub supplier_id: Option<i64>,
    #[serde(default)]
    pub api_uri: String,
    #[serde(default)]
    pub api_key: ImaSecret,
    pub model_name: String,
    #[serde(default)]
    pub max_input_tokens: u64,
    #[serde(default)]
    pub max_output_tokens: u64,
}

impl ImaModel {
    pub fn input(&self) -> ImaModelInput {
        ImaModelInput {
            api_uri: self.api_uri.clone(),
            api_key: self.api_key.clone(),
            model_name: self.model_name.clone(),
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens,
        }
    }

    pub fn matches(&self, input: &ImaModelInput) -> bool {
        self.api_uri == input.api_uri
            && self.api_key == input.api_key
            && self.model_name == input.model_name
            && self.max_input_tokens == input.max_input_tokens
            && self.max_output_tokens == input.max_output_tokens
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaSceneModel {
    pub model_id: String,
    #[serde(
        default = "unknown_model_type",
        deserialize_with = "optional_model_type"
    )]
    pub model_type: i64,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub customize_id: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default, deserialize_with = "sub_models")]
    pub sub_model_infos: Vec<ImaSceneModel>,
}

fn optional_model_type<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Option::<i64>::deserialize(deserializer).map(|value| value.unwrap_or_else(unknown_model_type))
}

fn sub_models<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ImaSceneModel>, D::Error> {
    use serde::de::Error;
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    // ima stores sub-models by thinking-mode number and looks them up through
    // Object.values(...). AT-Switch preserves each model's concrete ID/type;
    // those mode keys are not model identities. Accept the earlier array shape
    // too, and keep nested choices even if their container has no selectable ID.
    let values: Vec<Value> = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(values) => values,
        Value::Object(values) => values.into_iter().map(|(_, value)| value).collect(),
        _ => return Err(D::Error::custom("unsupported ima sub-model collection")),
    };
    let mut models = Vec::with_capacity(values.len());
    for value in values {
        let mut fields = match value {
            Value::Null => continue,
            Value::Object(fields) if fields.is_empty() => continue,
            Value::Object(fields) => fields,
            _ => return Err(D::Error::custom("unsupported ima sub-model entry")),
        };
        if fields.get("model_id").is_none_or(Value::is_null) {
            fields.insert("model_id".to_owned(), Value::String(String::new()));
        }
        // Missing/null IDs are non-selectable containers, just as in ima's
        // optional modelId check. Wrong non-null types remain a hard error.
        models.push(
            serde_json::from_value(Value::Object(fields))
                .map_err(|_| D::Error::custom("invalid ima sub-model fields"))?,
        );
    }
    Ok(models)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaSceneModels {
    pub models: Vec<ImaSceneModel>,
    #[serde(default)]
    pub preferred_model_id: Option<String>,
}

impl ImaSceneModels {
    pub(super) fn inherit_model_types(&mut self) {
        fn inherit(models: &mut [ImaSceneModel], parent: i64) {
            for model in models {
                if model.model_type < 0 {
                    model.model_type = parent;
                }
                inherit(&mut model.sub_model_infos, model.model_type);
            }
        }
        inherit(&mut self.models, -1);
    }

    pub fn find(&self, id: &str) -> Option<&ImaSceneModel> {
        fn find<'a>(models: &'a [ImaSceneModel], id: &str) -> Option<&'a ImaSceneModel> {
            models.iter().find_map(|model| {
                if model.model_id == id {
                    Some(model)
                } else {
                    find(&model.sub_model_infos, id)
                }
            })
        }
        find(&self.models, id)
    }
}

fn unknown_model_type() -> i64 {
    -1
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaModelLimits {
    #[serde(default)]
    pub default_input_tokens: u64,
    #[serde(default)]
    pub default_output_tokens: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaHomePage {
    pub models: Vec<ImaModel>,
    #[serde(default)]
    pub customize_model_config: ImaModelLimits,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImaSnapshot {
    pub account_key: String,
    pub homepage: ImaHomePage,
    pub scenes: [ImaSceneModels; 2],
}

/// The add response is deliberately kept separate from selectable model IDs.
/// The subsequent model-list read establishes the actual selectable ID.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ImaAddResult {
    #[serde(default)]
    pub customize_id: Option<String>,
    #[serde(default)]
    pub model_info: Option<ImaModel>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn thinking_mode_objects_preserve_all_selectable_ids_and_inherit_nullable_types() {
        let mut response: ImaSceneModels = serde_json::from_value(json!({
            "preferred_model_id":"thoughtful", "models":[{
                "model_id":"official-parent", "model_type":10, "is_default":true,
                "sub_model_infos":{
                    "0":{"model_id":"quick","model_type":11},
                    "1":{"model_id":"thoughtful","model_type":null},
                    "2":null, "3":{},
                    "4":{"model_type":12,"sub_model_infos":{"0":{"model_id":"nested"}}}
                }
            },{"model_id":"custom-entry","model_type":1000000,"sub_model_infos":{}}]
        }))
        .unwrap();
        response.inherit_model_types();
        assert_eq!(response.find("quick").unwrap().model_type, 11);
        assert_eq!(response.find("thoughtful").unwrap().model_type, 10);
        assert_eq!(response.find("nested").unwrap().model_type, 12);
        assert!(response
            .find("custom-entry")
            .unwrap()
            .sub_model_infos
            .is_empty());
        assert_eq!(response.models[0].sub_model_infos.len(), 3);
    }

    #[test]
    fn arrays_remain_supported_and_wrong_nonempty_child_types_are_rejected() {
        let response: ImaSceneModels = serde_json::from_value(json!({"models":[{
            "model_id":"parent", "model_type":10,"sub_model_infos":[null,{"model_id":"child"}]
        }]}))
        .unwrap();
        assert!(response.find("child").is_some());
        for invalid in [
            json!({"0":true}),
            json!({"0":{"model_id":123}}),
            json!({"0":{"model_id":"child","model_type":"invalid"}}),
        ] {
            assert!(serde_json::from_value::<ImaSceneModels>(json!({"models":[{
                "model_id":"parent","model_type":10,"sub_model_infos":invalid,
            }]}))
            .is_err());
        }
        assert!(serde_json::from_value::<ImaSceneModels>(
            json!({"models":[{"model_type":10,"sub_model_infos":{}}]})
        )
        .is_err());
    }
}
