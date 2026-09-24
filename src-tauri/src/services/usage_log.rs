//! AT-Switch 的完整使用日志。
//!
//! 代理请求只覆盖"流量确实经过 AT-Switch"的那部分；直连模式下请求由智能体直接发往
//! 上游，AT-Switch 不在链路上，无法观察也无法计数——那种情况下唯一可记录的就是
//! **切换操作**本身。两类记录合在一起，才是"AT-Switch 的全部使用情况"。
//!
//! 只在内存中保留，进程退出即清空（与代理计数器的行为一致）。

use std::collections::VecDeque;

use tokio::sync::RwLock;

use crate::domain::UsageLogEntry;

/// 保留上限。日志只用于排障与用量观察，必须设上限，否则长期运行会无限增长。
pub const USAGE_LOG_LIMIT: usize = 200;

#[derive(Default)]
pub struct UsageLog {
    entries: RwLock<VecDeque<UsageLogEntry>>,
}

impl UsageLog {
    #[allow(dead_code)]
    pub async fn record(&self, entry: UsageLogEntry) {
        let mut entries = self.entries.write().await;
        if entries.len() >= USAGE_LOG_LIMIT {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// 最近的记录，新的在前。
    pub async fn recent(&self, limit: usize) -> Vec<UsageLogEntry> {
        self.entries
            .read()
            .await
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{UsageKind, UsageOutcome};

    #[tokio::test]
    async fn keeps_only_the_newest_entries() {
        let log = UsageLog::default();
        for index in 0..USAGE_LOG_LIMIT + 20 {
            log.record(UsageLogEntry::switch(
                "codex",
                "p",
                "Provider P",
                &format!("model-{index}"),
                None,
            ))
            .await;
        }

        let recent = log.recent(USAGE_LOG_LIMIT + 50).await;
        assert_eq!(recent.len(), USAGE_LOG_LIMIT);
        assert_eq!(
            recent[0].model,
            format!("model-{}", USAGE_LOG_LIMIT + 19),
            "the newest entry comes first"
        );
    }

    #[tokio::test]
    async fn a_failed_switch_is_recorded_as_failed() {
        let log = UsageLog::default();
        log.record(UsageLogEntry::switch(
            "kimiwork",
            "p",
            "Provider P",
            "m",
            Some("kimiwork_write_verification_failed".to_owned()),
        ))
        .await;

        let recent = log.recent(10).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].kind, UsageKind::Switch);
        assert_eq!(recent[0].outcome, UsageOutcome::Failed);
        assert_eq!(recent[0].status, None);
    }
}
