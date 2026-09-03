use crate::domain::{ApiProtocol, AppResult, CommandError};
use crate::services::BaselineSnapshot;

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};

pub struct ImaAdapter;

impl AgentAdapter for ImaAdapter {
    fn id(&self) -> &'static str {
        "ima"
    }

    fn display_name(&self) -> &'static str {
        "ima"
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            &["ima.copilot.app", "ima.app"],
            &["com.tencent.imamac"],
            &[
                "Programs/ima.copilot/ima.copilot.exe",
                "Tencent/ima.copilot/ima.copilot.exe",
                "ima.copilot/ima.copilot.exe",
            ],
        );
        AgentDetection::manual(
            self.id(),
            self.display_name(),
            installation,
            "ima 自定义模型由登录态后端管理；当前仅支持检测，需在 ima 设置中手动配置",
        )
    }

    fn source_protocol(
        &self,
        _desired_mode: crate::domain::AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn build_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        Err(CommandError::new(
            "ima_remote_configuration_required",
            "ima 自定义模型需要通过其登录态服务保存",
        )
        .with_recovery("请在 ima 设置 → 模型设置 → 自定义模型中手动添加。"))
    }

    fn build_native_config(
        &self,
        _detection: &AgentDetection,
        _baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        Err(CommandError::new(
            "ima_remote_configuration_required",
            "ima 自定义模型需要通过其登录态服务保存",
        ))
    }

    fn verify_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        Err(CommandError::new(
            "ima_remote_configuration_required",
            "ima 自定义模型需要通过其登录态服务保存",
        ))
    }
}
