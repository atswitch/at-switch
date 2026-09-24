//! GitHub Releases 版本检查。

use serde::Deserialize;

use super::{AppResult, CommandError};

/// 从 GitHub Releases API 获取的最新版本信息。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// 完整 tag 名称，如 "v3.15.0"。
    pub tag_name: String,
    /// 规范化版本号（不含 v 前缀），如 "3.15.0"。
    pub version: String,
    /// 发行说明页面 URL。
    pub html_url: String,
    /// 发布时间（ISO 8601）。
    pub published_at: String,
    /// 发行说明前 500 字符（Markdown）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    published_at: String,
    #[serde(default)]
    body: String,
}

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/at-switch/at-switch/releases/latest";

/// 检查 GitHub 上是否有比当前版本更新的正式发布。
/// 如果无法连接或已是最新版本，返回 None 而非错误。
pub async fn check_update(current_version: &str) -> AppResult<Option<ReleaseInfo>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("AT-Switch/{}", current_version))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            log::warn!("HTTP client build failed: {e}");
            CommandError::internal("无法建立网络连接")
        })?;

    let response = match client.get(GITHUB_RELEASES_URL).send().await {
        Ok(resp) => resp,
        Err(e) => {
            log::warn!("update check request failed: {e}");
            return Ok(None);
        }
    };

    if !response.status().is_success() {
        log::warn!("GitHub releases API returned status {}", response.status());
        return Ok(None);
    }

    let release: GithubRelease = match response.json().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("failed to parse GitHub release JSON: {e}");
            return Ok(None);
        }
    };

    let latest = release.tag_name.trim_start_matches('v');
    if !is_newer_version(latest, current_version) {
        return Ok(None);
    }

    let body_preview = if release.body.is_empty() {
        None
    } else {
        Some(
            release
                .body
                .chars()
                .take(500)
                .collect::<String>()
                .trim()
                .to_string(),
        )
    };

    let version = latest.to_string();
    let tag_name = release.tag_name;

    Ok(Some(ReleaseInfo {
        tag_name,
        version,
        html_url: release.html_url,
        published_at: release.published_at,
        body_preview,
    }))
}

/// 比较两个 semver 风格的版本字符串（不含 v 前缀）。
/// 返回 `latest` 是否严格大于 `current`。
fn is_newer_version(latest: &str, current: &str) -> bool {
    // 预处理：去除 prerelease 后缀（如 alpha、beta、rc）以便比较
    let latest_stripped = latest.split('-').next().unwrap_or(latest);
    let current_stripped = current.split('-').next().unwrap_or(current);

    let parse_parts =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|p| p.parse::<u64>().ok()).collect() };

    let latest_parts = parse_parts(latest_stripped);
    let current_parts = parse_parts(current_stripped);

    let max_len = latest_parts.len().max(current_parts.len());

    for i in 0..max_len {
        let l = latest_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("3.15.0", "3.14.2"));
        assert!(is_newer_version("4.0.0", "3.14.2"));
        assert!(is_newer_version("3.14.3", "3.14.2"));
        assert!(!is_newer_version("3.14.2", "3.14.2"));
        assert!(!is_newer_version("3.14.1", "3.14.2"));
        assert!(is_newer_version("3.15.0-alpha", "3.14.2"));
        assert!(!is_newer_version("3.14.2-rc1", "3.14.2"));
        assert!(is_newer_version("3.15.0", "3.14.2-alpha"));
    }
}
