use std::path::PathBuf;

use super::{
    locator::{locate_desktop_app, DiscoveryContext},
    AgentAdapter, AgentDetection, DesiredAgentBinding,
};
use crate::{
    domain::{
        AgentBindingMode, AgentConfigHealth, AgentInstallStatus, ApiProtocol, AppResult,
        CommandError,
    },
    services::BaselineSnapshot,
};

/// Desktop Agents that AT-Switch can detect but must not rewrite. Their model
/// settings live in vendor-owned stores without a stable third-party
/// configuration contract, so detection stays read-only and every write
/// returns a stable unsupported error.
pub struct DetectionOnlyAdapter {
    id: &'static str,
    display_name: &'static str,
    macos_apps: &'static [&'static str],
    bundle_ids: &'static [&'static str],
    windows_paths: &'static [&'static str],
    config_candidates: &'static [&'static str],
    read_only_notice: &'static str,
}

impl DetectionOnlyAdapter {
    const fn new(
        id: &'static str,
        display_name: &'static str,
        macos_apps: &'static [&'static str],
        bundle_ids: &'static [&'static str],
        windows_paths: &'static [&'static str],
        config_candidates: &'static [&'static str],
        read_only_notice: &'static str,
    ) -> Self {
        Self {
            id,
            display_name,
            macos_apps,
            bundle_ids,
            windows_paths,
            config_candidates,
            read_only_notice,
        }
    }

    fn unsupported(&self) -> CommandError {
        CommandError::new(
            format!("{}_write_unsupported", self.id),
            format!(
                "AT-Switch 只检测 {}，不修改其内置模型配置",
                self.display_name
            ),
        )
        .with_recovery("请在该 Agent 的官方设置中配置模型；安装检测和状态展示仍可继续使用。")
    }

    fn resolve_config_path(&self, context: &DiscoveryContext) -> PathBuf {
        self.config_candidates
            .iter()
            .map(|relative| context.home.join(relative))
            .find(|path| path.exists())
            .unwrap_or_else(|| context.home.join(self.config_candidates[0]))
    }
}

impl AgentAdapter for DetectionOnlyAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.display_name
    }

    fn detect(&self, context: &DiscoveryContext) -> AgentDetection {
        let installation = locate_desktop_app(
            context,
            self.macos_apps,
            self.bundle_ids,
            self.windows_paths,
        );
        let config_path = self.resolve_config_path(context);
        let installed = installation.is_some();

        AgentDetection {
            id: self.id,
            display_name: self.display_name,
            installation,
            config_path: Some(config_path),
            runtime_data_dir: None,
            install_status: if installed {
                AgentInstallStatus::Installed
            } else {
                AgentInstallStatus::NotInstalled
            },
            config_health: if installed {
                AgentConfigHealth::Healthy
            } else {
                AgentConfigHealth::UnsupportedVersion
            },
            write_supported: false,
            needs_restart: false,
            custom_install_path: None,
            using_custom_install_path: false,
            message: Some(if installed {
                self.read_only_notice.to_owned()
            } else {
                format!("未在系统标准安装位置检测到 {}", self.display_name)
            }),
        }
    }

    fn source_protocol(
        &self,
        _mode: AgentBindingMode,
        _upstream_protocol: ApiProtocol,
    ) -> ApiProtocol {
        ApiProtocol::OpenaiChatCompletions
    }

    fn validate_binding(&self, _desired: &DesiredAgentBinding<'_>) -> AppResult<()> {
        Err(self.unsupported())
    }

    fn build_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<Vec<u8>> {
        Err(self.unsupported())
    }

    fn build_native_config(
        &self,
        _detection: &AgentDetection,
        _baseline: &BaselineSnapshot,
    ) -> AppResult<Vec<u8>> {
        Err(self.unsupported())
    }

    fn verify_config(
        &self,
        _detection: &AgentDetection,
        _desired: &DesiredAgentBinding<'_>,
    ) -> AppResult<()> {
        Err(self.unsupported())
    }
}

/// 千问办公确实存在 BYOK 自定义模型通道：`QwenWorkCN/data/agents.db` 是明文 SQLite，
/// 表 `byok_custom_models` 可写，当前模型由 `app_settings.modelLevel` 指定。曾据此
/// 实现过完整写入，但**真机验证表明这条路不可用**：
///
/// 1. 只写库不够。应用自身的 `hasCustomByokEnvOverride()` 会读 `~/.qwenworkcn/.env`
///    里的 `ALLOW_CUSTOM_BYOK`；账号侧 `allowBYOK` 为 0 时，写进去的模型会被
///    `ModelSelection` 判为 `configured_model_unavailable` 并 `auto_fallback` 回退。
/// 2. 即使补上那个开关，模型**能被选中、却无法使用**——调用经 Qwen 自己的模型网关
///    鉴权，服务端直接拒绝：
///    `[UnifiedExecutor] Assistant output finalized for API error {"errorCode":403}`，
///    提示 "You do not have access to this model service. Please check your account
///    permissions."。
///
/// 即 BYOK 是"客户端开关 + 服务端授权"双重门控，本地只能解开第一道。既然打开客户端
/// 开关既不能让功能可用、又等于绕过账号侧门控，本适配器不再写入任何内容。
pub const QWEN_WORK_ADAPTER: DetectionOnlyAdapter = DetectionOnlyAdapter::new(
    "qwenwork",
    "千问办公",
    &["QwenWorkCN.app"],
    &["cn.qwenwork.desktop.mac"],
    &[
        "Programs/QwenWork/QwenWork.exe",
        "QwenWork/QwenWork.exe",
        "QwenWork/QwenWorkCN.exe",
    ],
    &[
        "Library/Application Support/QwenWorkCN",
        ".qwenworkcn",
        "AppData/Local/QwenWorkCN",
        "AppData/Roaming/QwenWorkCN",
    ],
    "千问办公已检测到；其自定义模型需要账号侧授权（调用由服务端按权限放行），AT-Switch 不修改其配置，当前只显示安装状态。",
);

/// Kimi Work (Daimon)。曾实现为可写入，实测后回退为仅检测。
///
/// 原方案是往 `kimi-desktop/daimon-share/daimon/config.json` 新增自有 Provider，并把
/// `model.current` 指向它——依据是"Daimon 用 `model.current` 生成运行态 TOML 的
/// `default_model`"。前半段成立：真机 `runtime/kimi-code/config.toml` 里确实出现了
/// `[providers.at-switch]`（指向本地代理）与 `[models.at-switch]`，说明 Provider 与模型
/// 都被 Daimon 正常接收。
///
/// **但 `model.current` 写不进去**：Daimon 在每次启动时把它重置为服务端下发的默认模型
/// `k2d8-preview`，随后才生成运行态 TOML，于是 `default_model` 永远回落到官方模型。
/// 真机对照实验（各等 25 秒并重启验证）：
///
/// | 写入值 | 运行中不重启 | 重启后 |
/// |---|---|---|
/// | `at-switch` | 保持 | 被重置为 `k2d8-preview` |
/// | `k3-agent`（官方已有模型） | 保持 | 同样被重置为 `k2d8-preview` |
///
/// 连"官方已有的合法模型名"都会被重置，说明这不是"校验不认识的模型"，而是**启动时
/// 无条件覆盖**。全盘搜索确认 `k2d8-preview` 不在任何其它本地文件中，来源在服务端。
/// 因此本地没有可写入的开关，切换无法生效（表现为代理侧始终零流量、请求仍走官方网关）。
pub const KIMI_WORK_ADAPTER: DetectionOnlyAdapter = DetectionOnlyAdapter::new(
    "kimiwork",
    "Kimi Work",
    &["Kimi.app"],
    &["com.moonshot.kimichat"],
    &["Programs/Kimi/Kimi.exe", "Kimi/Kimi.exe", "Kimi.exe"],
    &[
        "Library/Application Support/kimi-desktop",
        "AppData/Roaming/kimi-desktop",
        "AppData/Local/kimi-desktop",
    ],
    "Kimi Work 已检测到；其运行时配置由 Daimon 在每次启动时按服务端下发的默认模型重建（model.current 会被无条件覆盖），本地写入无法生效，因此不修改其配置，当前只显示安装状态。",
);

/// ima（腾讯，`com.tencent.imamac`）。**注意它确实有自定义模型入口**，只是写入留不住。
///
/// 它是原生壳 + CEF（Frameworks 里是 `ImSDKForMac_Plus` / `MMKV` / `RDelivery`），
/// 没有 Electron 的 `app.asar`。模型配置是**本地明文**，在
/// `Default/Preferences` 的 `kExtraSettingInfo`（JSON 字符串）里：
///
/// ```text
/// modelConfig.modelOptions = [ {id:3, name:"DeepSeek-V4-Flash", modelId:"official_3"},
///                              …,
///                              {id:1000000, name:"NMauto", desc:"自定义模型",
///                               modelId:"9305e434-c7b3-48fa-8b31-181e202567e0"} ]
/// modelConfig.modelType / modelId / hasUserSelection   ← 选中态
/// ```
///
/// 但**外部写入会被覆盖**。真机对照实验（ima 关闭 → 写入 `modelType=3` /
/// `modelId="official_3"` → 打开 ima）：
///
/// - ima 启动 **11 秒**后即重写 `Preferences`，选中态被改回 `1000000` /
///   `9305e434-…`，并把 `hasUserSelection` 置回 `false`
/// - 用该模型 UUID 反查整个数据目录，**除 `Preferences` 外没有任何副本**
///   （`mmkv/`、leveldb、`com.tencent.imamac.plist` 均无），说明权威状态在服务端
///
/// 结论：模型清单与选中态由登录态 + 服务端下发，本地没有可写入的通道，
/// 因此只做安装检测。这与 QClaw / EasyClaw 那类"自有 Provider 可写"的发行版不同。
pub const IMA_ADAPTER: DetectionOnlyAdapter = DetectionOnlyAdapter::new(
    "ima",
    "ima",
    &["ima.copilot.app"],
    &["com.tencent.imamac"],
    &["Programs/ima/ima.exe", "ima/ima.exe"],
    &[
        "Library/Application Support/com.tencent.imamac",
        "AppData/Roaming/com.tencent.imamac",
        "AppData/Local/com.tencent.imamac",
    ],
    "ima 已检测到；其模型由登录态与服务端下发，本地没有用户级模型配置入口，AT-Switch 不修改其配置，当前只显示安装状态。",
);

pub const DOUBAO_WORK_ADAPTER: DetectionOnlyAdapter = DetectionOnlyAdapter::new(
    "doubaowork",
    "豆包工作",
    &["DoubaoWork.app"],
    &["com.work.pc.doubao"],
    &[
        "Programs/DoubaoWork/DoubaoWork.exe",
        "DoubaoWork/DoubaoWork.exe",
        "Doubao/DoubaoWork.exe",
    ],
    &[
        "Library/Application Support/DoubaoWork",
        "DoubaoWork",
        "AppData/Local/DoubaoWork",
        "AppData/Roaming/DoubaoWork",
    ],
    "豆包工作已检测到；AT-Switch 当前只显示安装状态，不修改其内置模型配置。",
);

pub const COZE_ADAPTER: DetectionOnlyAdapter = DetectionOnlyAdapter::new(
    "coze",
    "扣子",
    &["Coze.app"],
    &["cn.coze.desktop"],
    &["Programs/Coze/Coze.exe", "Coze/Coze.exe"],
    &[
        "Library/Application Support/Coze",
        ".coze",
        "AppData/Local/Coze",
        "AppData/Roaming/Coze",
    ],
    "扣子已检测到；AT-Switch 当前只显示安装状态，不修改其内置模型配置。",
);

#[cfg(test)]
#[path = "detection_only_tests.rs"]
mod tests;
