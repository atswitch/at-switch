# OpenClaw 本体接入与验收

- 接入日期：2026-10-22　分支：`release/openclaw`　基线：`main` @ `243b631`
- 设计文档：`doc/openclaw-integration/设计方案.md`（工作区 `doc/`）

## 技术依据

| 项 | 结论 |
| --- | --- |
| 安装形态 | 桌面 `OpenClaw.app`（Bundle ID `ai.openclaw.mac`，本机样本 `2026.10.5`）+ CLI `~/.openclaw/bin/openclaw`（官方安装脚本固定目录，PATH 可发现） |
| 权威配置 | `~/.openclaw/openclaw.json`（Windows `%USERPROFILE%\.openclaw\openclaw.json`），无任何间接层 |
| 实证 | 本机配置 524 字节纯 JSON（`gateway.auth` / `meta` / `plugins.entries`，无 `models` 段）；`state/openclaw.sqlite` 的 `user_preferences` / `config_machine_state` 为空，不持有模型选择 |
| 复用内核 | `openclaw.rs` 共享写内核（`build_openclaw_config` 等，QClaw/AutoClaw/EasyClaw/AionClaw 四个已发布适配器验证），本适配器为薄封装，仅开放 `pub(super)` 可见性 |
| 协议 | Direct 镜像上游协议（OpenAI Chat / Responses / Anthropic Messages 全支持），Proxy 固定 OpenAI Chat |
| 受管边界 | `models.providers.<provider_name>`（替换旧 `at-switch*` 条目）+ `agents.defaults.model.primary`；用户 provider、未知字段、`gateway`/`plugins` 全保留 |
| sidecar | `agents/main/sessions/sessions.json` 会话模型 + `*.jsonl` 转录裸模型 id（复用 `build_session_model_updates`） |
| 恢复 | baseline 存在 → 恢复原 `primary`；不存在 → 移除受管 `primary`、清 `at-switch*` 条目，用户条目保留 |

## 已知边界

- **严格 JSON**：官方文档称配置为 JSON5，但 OpenClaw 自写为纯 JSON；带注释文件返回 `agent_config_unparseable` 拒绝接管（恢复建议：用 OpenClaw 设置页保存一次）
- **apiKey 明文**：沿用 QClaw/EasyClaw/AionClaw 发布先例，值由 AgentService 从系统凭据库注入
- Windows 安装路径按标准布局声明（`Programs/OpenClaw/OpenClaw.exe` 等），**真机样本待补**

## 自动化验证（2026-10-22）

- `cargo fmt` / `cargo clippy --all-targets -- -D warnings` / `cargo test`：全绿，271 通过（含新增 10 个 `openclaw_native_tests`：元数据稳定、三协议 source_protocol、fresh 配置写入与用户字段保留、A→B 幂等替换、restore 有/无 baseline、损坏配置拒绝、sessions sidecar、桌面优先/CLI 回退/未安装检测）
- 内核既有 openclaw 测试无回归

## 真机验收（待执行）

按《AT-Switch新Agent接入流程与规范》§6.2 矩阵执行：

- [ ] macOS：首次接管 / A→B→A / 真实请求 / 重启生效 / 恢复 / 与其它 Claw 共存
- [ ] Windows：安装发现路径样本 + 写入冒烟
- [ ] 额外 2–3 名开发人员独立验证
