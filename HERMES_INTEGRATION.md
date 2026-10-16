# Hermes Agent 接入说明（HERMES_INTEGRATION）

日期：2026-10-16
分支：`release/hermes`（基点 `fd1e68a`）
实现来源：移植自 `release-hermes` commit `200e313` 的 `hermes.rs`（523 行）/
`hermes_tests.rs`（208 行）/ `hermes.png`，适配当前 trait（删除 `discovery_hints()`
覆盖）并新增 `serde_yaml` 依赖。

## 机制摘要

- 应用：Hermes Agent。macOS `Hermes.app` / Bundle `com.hermes.agent`；Windows
  `Programs/Hermes/Hermes.exe`、`Hermes/Hermes.exe`、`Hermes.exe`；同时支持命令行
  安装发现（`~/.local/bin`、`~/.cargo/bin`、npm 全局目录、PATH、注册表 App Paths）。
- 配置权威源：`$HERMES_HOME/config.yaml` 或 `~/.hermes/config.yaml`（YAML，
  serde_yaml 解析）。
- 写入：受管 Provider `at-switch` + 默认模型；API Key 经环境变量
  `HERMES_AT_SWITCH_API_KEY` 引用，不落明文。
- 协议：Direct 仅 OpenAI Chat Completions（`hermes_direct_protocol_unsupported`）；
  其余协议走本地代理。
- 事务：`ConfigTransaction` 基线 + 加密备份 + 原子替换 + 回读校验 + 失败回滚。
- 生效时机：无需重启（`needs_restart = false`，config.yaml 热加载）。

## 验收状态

- 自动化门禁（2026-10-16，macOS）：`cargo fmt --check`、`cargo clippy -D warnings`、
  `cargo test`（230 通过，含 hermes 移植用例）、`npm run build`、
  `npm test -- --run`（115 通过）、`npm run licenses:check` 全部通过。
- macOS 真机验收：待执行（发现三态 / 首次切换 / A→B→A / Direct+Proxy / 真实请求 /
  恢复原配置；本机是否安装 Hermes 待确认）。
- Windows 真机验收：未验证，待补；当前仅有单测与构建覆盖，不得宣称已通过。

## 已知边界

- `manual_recovery_steps`（人工恢复提示）依赖 release-hermes 的 domain 扩展字段，
  本次移植未包含；恢复原配置走统一的基线还原路径。
- 测试与文档不得包含真实 apiKey；真机验证证据需脱敏。
