use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_language")]
    pub language: String,
    pub theme: String,
    pub start_at_login: bool,
    pub keep_running_in_background: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: "system".to_owned(),
            start_at_login: false,
            keep_running_in_background: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub language: Option<String>,
    pub theme: Option<String>,
    pub start_at_login: Option<bool>,
    pub keep_running_in_background: Option<bool>,
}

fn default_language() -> String {
    "zh-CN".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_default_to_simplified_chinese() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"theme":"system","startAtLogin":false,"keepRunningInBackground":true}"#,
        )
        .expect("legacy settings should remain readable");

        assert_eq!(settings.language, "zh-CN");
    }
}
