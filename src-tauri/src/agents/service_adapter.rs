use futures_util::future::BoxFuture;

use crate::{domain::AppResult, services::ConfigTransaction};

use super::{AgentDetection, DesiredAgentBinding};

pub(super) type CommitBinding<'a> = dyn Fn() -> AppResult<()> + Send + Sync + 'a;

pub(super) struct ServiceConfigOutcome {
    pub needs_restart: bool,
    pub message: String,
}

/// Optional capability for Agents whose authoritative settings live behind an
/// authenticated service. File adapters keep their existing transaction path.
pub(super) trait ServiceConfigAdapter: Send + Sync {
    fn account_scope(&self, detection: &AgentDetection) -> AppResult<String>;

    fn checkpoint_status(
        &self,
        detection: &AgentDetection,
        transaction: &ConfigTransaction,
    ) -> AppResult<Option<(bool, bool)>>;

    fn apply<'a>(
        &'a self,
        detection: &'a AgentDetection,
        desired: &'a DesiredAgentBinding<'a>,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>>;

    fn restore<'a>(
        &'a self,
        detection: &'a AgentDetection,
        transaction: &'a ConfigTransaction,
        commit: &'a CommitBinding<'a>,
    ) -> BoxFuture<'a, AppResult<ServiceConfigOutcome>>;

    /// Installation scans verify the local selection against the durable
    /// journal without connecting to the Agent's account or prompting for it.
    fn verify_cached(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
        transaction: &ConfigTransaction,
    ) -> AppResult<()>;
}
