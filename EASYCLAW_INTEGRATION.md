# EasyClaw 接入说明（EASYCLAW_INTEGRATION）

日期：2026-10-16
分支：`release/easyclaw`（基点 `fd1e68a`）
实现来源：移植自 `release-hermes` commit `200e313` 的 `EasyClawAdapter`（位于
`openclaw.rs`）。EasyClaw 与 QClaw/AionClaw 同属 OpenClaw 发行版，共享读写内核
（`build_openclaw_config` / `build_native_openclaw_config` / `verify_openclaw_config`），
当前分支内核函数签名一致，直接复用；仅删除 `discovery_hints()` 覆盖以适配当前 trait。

## 机制摘要

- 应用：EasyClaw（猎豹移动），macOS `easyclaw.app` / Bundle `ai.easyclawcn.desktop`；
  Windows `Programs/EasyClaw/EasyClaw.exe`、`EasyClaw/EasyClaw.exe`。
- 配置权威源：`~/.easyclaw/easyclaw.json`（`models.providers` +
  `agents.defaults.model.primary`，与 QClaw/AionClaw 结构逐字一致）。
  `~/.easyclaw/easyclawcli` 明确设置 `EASYCLAW_CONFIG_DIR=/…/.easyclaw`，
  因此 `easyclaw.json` 本身就是权威配置，无需写第二处。
- 写入时机：EasyClaw 仅在安装/升级时按 `.config-merge-signal.json` 合并一次默认值
  （留 `.easyclaw-snapshots/*.bak`），**不会在每次启动时重建**——与 AutoClaw 的
  "启动重建 openclaw.json"不同，配置写入可持久生效。
- 事务：`ConfigTransaction` 基线 + 加密备份 + 原子替换 + 回读校验 + 失败回滚。
- 协议：`openclaw_source_protocol`——Direct 直通上游协议（OpenAI Chat / Responses /
  Anthropic Messages）；Proxy 固定 OpenAI Chat 由本地代理转换。
- 生效时机：需重启 EasyClaw（`needs_restart = true`）。

## 模型锚点（三层，2026-10-19 补齐）

EasyClaw 的"当前模型"锚定在 `easyclaw.json` 之外还有三处，切换时必须一并更新，
否则已存在的 Agent / 会话会继续请求切换前的模型：

1. `agents.list[*].model.primary`——每个 Agent 定义自带的模型，**优先级高于
   `agents.defaults.model`**。只改指向自定义 Provider（非官方 `easyclaw`）的
   primary，官方模型的 Agent 配置不动。
2. `agents/main/sessions/sessions.json`——每个会话对象的 `model` 字段；没有该
   字段的会话本来就走默认。
3. 会话线程转录 `agents/main/sessions/*.jsonl`——每条消息的 `"model"` 元数据，
   网关恢复线程时按最后一条续聊。只改写**裸 id**（不含 `/`，用户自建/受管端点
   的模型）；带 provider 前缀的官方 id 保持原样。

第 2、3 层通过 `AgentAdapter::build_sidecar_updates` 伴生更新钩子实现：与主配置
写入共用同一事务窗口，任一失败或 verify 失败全部一起回滚。该钩子为 trait 默认
空实现，非 OpenClaw 系适配器不受影响；QClaw / AutoClaw 共享同一内核，同样受益。

## 验收状态

- 自动化门禁（2026-10-16，macOS）：`cargo fmt --check`、`cargo clippy -D warnings`、
  `cargo test`（221 通过）、`npm run build`、`npm test -- --run`（115 通过）、
  `npm run licenses:check` 全部通过。
- macOS 真机验收：待执行（三态发现 / 首次切换 / A→B→A / Direct+Proxy / 真实请求 /
  重启持久 / 恢复原配置）。
- Windows 真机验收：未验证，待补；当前仅有单测与构建覆盖，不得宣称已通过。

## 已知边界

- EasyClaw 官方配置（`easyclaw/deepseek.deepseek-v4-flash` 等）在首次接管时进入
  基线；"恢复原配置"按基线还原，官方 Provider 与用户无关配置保留。
- 测试与文档不得包含真实 apiKey；真机验证证据需脱敏。
