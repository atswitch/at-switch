# AionClaw 接入说明（AIONCLAW_INTEGRATION）

日期：2026-10-16
分支：`release/aionclaw`（基点 `fd1e68a`）
实现来源：移植自 `release-hermes` commit `200e313` 的 `AionClawAdapter`（位于
`openclaw.rs`）与 `aionclaw.rs` 状态改写助手（286 行），适配当前 trait（删除
`discovery_hints()` 覆盖）。

## 机制摘要

- 应用：AionClaw，macOS `AionClaw.app` / Bundle `com.quyuanai.aionclaw`；Windows
  `Programs/AionClaw/AionClaw.exe`、`AionClaw/AionClaw.exe`。
- 配置分两层，**必须同时改写**：
  1. `openclaw/state/openclaw.json`（macOS 沙盒容器
     `Library/Containers/com.quyuanai.aionclaw/.../AionClaw/openclaw/state/`）——
     `models.providers` + `agents.defaults.model.primary`，与 QClaw 结构一致；
  2. `~/.aionclaw/cli/db/db.sqlite` 的 `kv.app_config` 与 `agents.model`——应用
     启动时由前者重建 `openclaw.json` 并给自建 Provider 重新编号，因此数据库才是
     权威状态；`agents.model` 是界面真正使用的当前模型，只改 Provider 目录会导致
     "模型可见但未选中"。
- 切换流程：`ConfigTransaction` 写 `openclaw.json`（基线+备份+原子替换+回读校验）
  → `aionclaw::apply_config` 写数据库 → `aionclaw::verify_config` 回读校验 →
  失败时 `restore_config` + 配置回滚。
- 协议：Direct 直通上游协议（OpenAI Chat / Responses / Anthropic Messages）；
  Proxy 固定 OpenAI Chat 转换。
- 生效时机：需重启 AionClaw（`needs_restart = true`）。

## 验收状态

- 自动化门禁（2026-10-16，macOS）：`cargo fmt --check`、`cargo clippy -D warnings`、
  `cargo test`（227 通过，含 aionclaw 移植用例）、`npm run build`、
  `npm test -- --run`（115 通过）、`npm run licenses:check` 全部通过。
- macOS 真机验收：待执行（发现三态 / 首次切换 / A→B→A / Direct+Proxy / 真实请求 /
  重启持久 / 恢复原配置）。
- Windows 真机验收：未验证，待补；当前仅有单测与构建覆盖，不得宣称已通过。

## 已知边界

- 沙盒容器路径对当前用户可读写（release-hermes 实测）；若系统权限策略变化导致
  不可写，检测会报 `Unwritable` 并提示。
- 测试与文档不得包含真实 apiKey；真机验证证据需脱敏。
