# ZCode 接入与验收

> 接入 Agent：**ZCode**（Agent ID `zcode`），本次范围仅此一个 Agent
> 基线：`main` @ `fd1e68a`（v3.16.1）；接入分支：`feat/zcode`
> 设计方案与任务拆分：`doc/zcode-integration/`（工作区 `doc/` 下）
> 规范依据：`doc/AT-Switch新Agent接入流程与规范.md`、`doc/AGENTS.md`

## 1. 范围与能力

| 项 | 内容 |
| --- | --- |
| Agent ID / 显示名 | `zcode` / `ZCode` |
| 应用身份 | macOS `ZCode.app`（Bundle ID `dev.zcode.app`）；Windows `ZCode.exe` |
| 配置权威来源 | 个人 Provider 配置 `~/.zcode/v2/provider_config.json`（Windows 按应用数据目录推导） |
| 协议 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages |
| 模式 | Direct 与 Proxy 均支持；Proxy 固定 `openai-chat-completions` |
| 生效方式 | 需重启 ZCode（`needs_restart`），运行中切换会安全退出并在写入后按原状态恢复 |
| 限制 | 只改写 AT-Switch 受管内容；内置 `zcode-builtin.json` 由服务端下发，不是写入目标 |

## 2. 写入机制

受管内容只有三类，其余一律保留：

1. `config.providerConfigRules.providerRules[]` 中 `providerId == "at-switch"` 的条目
   （`group: "standard-personal"`、`access.apiKey` 明文、`api.type` 与 `baseUrl`、
   `personalModelIds` / `modelOrder`）
2. `config.providerOrder`：把 `at-switch` 置到首位，其余顺序不变
3. `config.defaultModelSelection`（仅当其指向 `at-switch`）

模型声明在 Provider 自身，不写 `modelConfigRules.manualProviderModelRules`——该数组要求完整
能力块，缺失时 ZCode 会判定**整个个人文件无效**并降级为空配置。同一 Provider 内切换模型时
累积保留历史模型，仅当上游 Endpoint 变化时重置，避免已固定模型的任务失效。

ZCode 的「当前选中模型」不在配置文件里，而是分布在两层：任务表
`~/.zcode/v2/tasks-index.sqlite` 的 `tasks.model`（权威）与 Chromium Local Storage 的
`modelSelection`（启动时刷出的镜像）。AT-Switch 在 ZCode 暂停期间同步这两层，返回原值用于
失败回滚。

## 3. 与既有实现的关系

`release-hermes` 分支已实现过 ZCode 并完成 macOS 真机验收，但其基线落后当前 `main`
约 96 个文件、15000/9200 行增删，无法直接复用。本次移植：保留配置读写、受管字段、
幂等 upsert、协议映射、任务表与 Local Storage 读写、单元测试与图标；重写
`discovery_hints`（main 无该能力）、进程检测修正、注册表与流水线接入点、前端声明与文档。

流水线接入沿用 `main` 现有的按 `agent_id` 分支模式（与 workbuddy / codebuddy 一致），
不改动公共 `AgentAdapter` trait。

## 4. 验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过，无警告 |
| `cargo test` | 238 passed / 0 failed（其中 ZCode 16 个） |
| `npm test -- --run` | 116 passed / 0 failed |
| `npm run build` | 通过 |
| `npm run licenses:check` | 通过（295 npm 包 / 541 Rust crate） |

ZCode 单测覆盖：写入结构、个人 Provider 分组与模型声明、三种协议映射、Proxy 回退、
空 Base URL 拒绝、未知字段与其他 Provider 保留、重复切换幂等、切换 Provider 时重置模型、
写后校验通过与被篡改时失败、任务表与 Local Storage 往返及回滚。

## 5. 未完成项与限制

1. **Windows 未真机验证。** 配置路径 `%APPDATA%/ZCode/v2/provider_config.json` 为按
   应用数据目录推导，无 Windows 真机样本。发布前必须补齐，不能记为「不适用」。
2. **恢复原始配置不回退任务表与 Local Storage。** 与 `release-hermes` 既有行为一致：
   「恢复原始模型」只移除配置文件中的受管内容，已固定到 `at-switch/<模型>` 的历史任务
   不会自动改回。待评估是否需要在后续迭代补上。
3. **真实 Provider 推理、Streaming、Tool Calling 尚未验证**，单测与 Mock 不替代真机验收。
