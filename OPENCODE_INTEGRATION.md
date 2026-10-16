# OpenCode 接入说明（OPENCODE_INTEGRATION）

日期：2026-10-14
分支：`release/opencode`（基点 `fd1e68a`）
实现来源：移植自 `release-hermes` commit `200e313` 的 `opencode.rs` / `opencode_tests.rs` /
`opencode.png`，并按当前分支 trait 适配（删除 `discovery_hints()` 覆盖；domain 层
hermes 专属字段未移植）。机制调研与设计：`doc/opencode-integration/设计方案.md`。

## 机制摘要

- 应用：`OpenCode.app`，Bundle ID `ai.opencode.desktop`；Windows 相对路径
  `Programs/OpenCode/OpenCode.exe`、`OpenCode/OpenCode.exe`、`OpenCode.exe`。
- 配置目录：`OPENCODE_HOME` > `~/.config/opencode`（**Windows 同样是
  `%USERPROFILE%\.config\opencode`**，不是 `%APPDATA%`；2026-10-15 修复：原移植
  实现在 Windows 误写 `%APPDATA%\opencode`，导致切换不被 OpenCode 加载）；
  `opencode.json` 优先于 `opencode.jsonc`，JSONC（注释/尾逗号）完整支持。
- 写入（受控点仅两处，其余全部保留）：
  - `provider.at-switch`：`@ai-sdk/openai-compatible` + `options.baseURL` +
    `options.apiKey` + `models`（仅目标模型）；
  - 顶层 `model`：`at-switch/<model-id>`。
- 协议：Direct 仅 OpenAI Chat Completions（`opencode_direct_protocol_unsupported` /
  `opencode_protocol_unsupported`）；其余走本地代理。
- 事务：`ConfigTransaction` 基线 + 加密备份 + 原子替换 + 回读校验 + 失败回滚；
  恢复按首次接管基线，无基线时移除受管项（若 provider 下仅剩受管项则一并移除）。
- 生效时机：需重启 OpenCode（`needs_restart = true`）。

## 与 release-hermes 的差异

| 项 | 说明 |
| --- | --- |
| `discovery_hints()` | 当前 trait 无此方法；`detect()` 已显式传入相同常量，行为等价 |
| domain 扩展字段 | `proxy_pref_enabled`、人工恢复步骤等为 hermes/zcode 功能，未随移植引入 |
| 注册位置 | `AgentRegistry` 中位于 `dumate` 之后；ID 稳定性 / Windows 标准路径 / 自定义目录（macOS+Windows）测试已同步 |

## 验收状态

- 自动化门禁（2026-10-14，macOS）：`cargo fmt --check`、`cargo clippy -D warnings`、
  `cargo test`（228 通过，含 opencode 7 例）、`npm run build`、`npm test -- --run`
  （115 通过）、`npm run licenses:check` 全部通过。
- macOS 真机验收（2026-10-14，OpenCode 桌面端 1.18.31，部分通过）：
  - 已安装发现、切换写入、自动重启、配置加载：通过（配置写入与进程重启同秒完成，
    OpenCode 日志确认加载 `opencode.json`）；
  - A→B 连续切换（NMauto → deepseek-v4-flash）与真实请求：通过（OpenCode 日志
    `llm.provider=at-switch llm.model=deepseek-v4-flash`，Direct 模式，写入后 29 秒内路由成功）；
  - 切换后重启、新建会话默认选中切换的模型：通过（用户实测确认；桌面端新建会话
    跟随配置 `model` 键）；
  - 待补：重复应用幂等复查、Proxy 模式真实请求、恢复原配置。
- Windows 真机验收：未验证，待补；当前仅有单测与构建覆盖，不得宣称已通过。

## 已知边界

- 模型选择器的初始显示：**新建会话**跟随配置 `model` 键（已实测确认）；**已有会话**
  保持各自会话记录中的模型（OpenCode 的 `session.model` 字段），不回退也不跟随。
- OpenCode 同时加载 `config.json`、`opencode.json`、`opencode.jsonc` 三个文件并合并；
  `opencode.json` 优先于 `opencode.jsonc` 的实现假设与该加载顺序兼容。
- 本机 `~/.config/opencode/opencode.json` 存在历史实验残留的 `at-switch` 受管块
  （含真实 apiKey）。首次接管时该文件整体进入基线；如需恢复"官方状态"，将按无基线
  处理移除受管项，用户自有 Provider（`opencode.jsonc` 中的 `g2`）不受影响。
- 测试与文档中不得出现真实 apiKey；真机验证证据需脱敏。
