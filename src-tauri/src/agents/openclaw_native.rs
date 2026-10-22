use std::path::PathBuf;

use super::{
    locator::{
        locate_command, locate_desktop_app, DiscoveryContext, Installation, InstallationKind,
    },
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::domain::{AgentBindingMode, ApiProtocol, AppResult};
use crate::services::{BaselineSnapshot, FileChange};

use super::openclaw::{
    build_native_openclaw_config, build_openclaw_config, build_session_model_updates,
    openclaw_source_protocol, probe_openclaw, verify_openclaw_config,
};

/// OpenClaw 本体（openclaw.ai，开源框架的原版发行）。桌面应用为
/// `OpenClaw.app`（Bundle ID `ai.openclaw.mac`），同版 CLI 安装在
/// `~/.openclaw/bin/openclaw` 并注册 PATH。权威配置就是 `~/.openclaw/openclaw.json`
/// ——没有 QClaw 的 `configPath` 重定向、没有 AutoClaw 的启动重建、也没有
/// AionClaw 的 sqlite 权威态，因此直接复用 OpenClaw 系共享读写内核
/// （`models.providers` + `agents.defaults.model.primary`）。
pub struct OpenClawAdapter;

impl OpenClawAdapter {
    fn detect_command(context: &DiscoveryContext) -> Option<Installation> {
        // 官方安装脚本固定落在 ~/.openclaw/bin，先于 PATH 检查；
        // npm 全局安装的 Windows 形态是 npm 目录下的 openclaw.cmd。
        let mut candidates = Vec::new();
        for name in ["openclaw", "openclaw.exe", "openclaw.cmd"] {
            candidates.push(context.home.join(".openclaw/bin").join(name));
            #[cfg(target_os = "windows")]
            candidates.push(context.home.join("AppData/Roaming/npm").join(name));
            candidates.push(context.home.join(".local/bin").join(name));
        }
        candidates
            .into_iter()
            .find(|path| path.is_file() || path.is_symlink())
            .map(|path| Installation {
                version: None,
                path,
                kind: InstallationKind::Command,
            })
            .or_else(|| locate_command(context, &["openclaw", "openclaw.exe", "openclaw.cmd"]))
    }
}

impl AgentAdapter for OpenClawAdapter {
    fn id(&self) -> &'static str {
        "openclaw"
    }

    fn display_name(&self) -> &'static str {
        "OpenClaw"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        // 桌面版优先：暂停/重启瞄准桌面应用进程，而不是 PATH 上的 CLI；
        // 仅 CLI 安装时按"退出失败不阻塞写入"的既有约定处理。
        let installation = locate_desktop_app(
            context,
            &["OpenClaw.app", "OpenClaw.exe", "OpenClaw"],
            &["ai.openclaw.mac"],
            &["Programs/OpenClaw/OpenClaw.exe", "OpenClaw/OpenClaw.exe"],
        )
        .or_else(|| Self::detect_command(context));

        AgentDetection::from_file_probe(
            self.id(),
            self.display_name(),
            installation,
            openclaw_config_path(context),
            probe_openclaw,
            true,
        )
    }

    fn source_protocol(
        &self,
        mode: AgentBindingMode,
        upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        openclaw_source_protocol(mode, upstream_protocol)
    }

    fn build_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        build_openclaw_config(detection, desired)
    }

    fn build_native_config(
        &self,
        detection: &AgentDetection,
        baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        build_native_openclaw_config(detection, baseline)
    }

    fn verify_config(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        verify_openclaw_config(detection, desired)
    }

    fn build_sidecar_updates(
        &self,
        detection: &AgentDetection,
        desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<FileChange>> {
        build_session_model_updates(detection, desired.model_id)
    }
}

fn openclaw_config_path(context: &DiscoveryContext) -> PathBuf {
    context.home.join(".openclaw").join("openclaw.json")
}

#[cfg(test)]
#[path = "openclaw_native_tests.rs"]
mod tests;
