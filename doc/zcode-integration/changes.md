# ZCode 完整接入 — 修改清单

> 日期：2026-09-21
> 需求：`zcode` 从「仅检测接入」升级为「完整接入（可切换）」
> 设计：`design.md`
> 任务状态：`checkpoint.yaml`（t1–t5 done，t6 真机验收 pending）

---

## 1. 实现范围

新增 `ZCodeAdapter`，写入 ZCode 个人配置文件 `~/.zcode/v2/provider_config.json` 的三处受管内容：

- `config.providerConfigRules.providerRules[]` 中 `providerId == "at-switch"` 的条目
  （`access = { type: "api-key", apiKey }`、`api = { type, baseUrl }`）
- `config.modelConfigRules.manualProviderModelRules[]` 中同 `providerId` 的模型声明
- `config.defaultModelSelection`（仅当其指向 `at-switch`）

支持 Direct 与 Proxy 两种模式：Direct 写入 Provider 真实 Endpoint、模型 ID 与凭据；
Proxy 写入本地 Endpoint 与本地令牌，`api.type` 固定 `openai-chat-completions`。

协议映射：`OpenaiChatCompletions → openai-chat-completions`、
`OpenaiResponses → openai-responses`、`AnthropicMessages → anthropic-messages`。

## 2. 受影响文件

### Rust

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/agents/zcode.rs` | 新增。适配器实现 |
| `src-tauri/src/agents/zcode_tests.rs` | 新增。9 个单元测试 |
| `src-tauri/src/agents/mod.rs` | 声明 `mod zcode`；注册表中 `ZCODE_ADAPTER` → `ZCodeAdapter` |
| `src-tauri/src/agents/detection_only.rs` | 删除 `ZCODE_ADAPTER` 常量（已废弃） |
| `src-tauri/src/agents/detection_only_tests.rs` | 移除 `ZCODE_ADAPTER` 相关断言 |

### 前端

| 文件 | 改动 |
| --- | --- |
| `src/lib/agentCapabilities.ts` | `zcode` 加入 `SWITCHABLE_AGENT_IDS`；`isDetectionOnlyAgent` 移除 `zcode` 并补回 `aionclaw` |
| `src/lib/agentCapabilities.test.ts` | 新增 2 个用例（zcode 可切换 / aionclaw 只读） |
| `src/lib/api.ts` | Mock 补齐 `qwenwork` / `doubaowork` / `coze`，浏览器预览与 Rust 注册表对齐 |
| `src/App.test.tsx` | 详情按钮数量断言改为动态读取 Mock 快照 |

### 文档

| 文件 | 改动 |
| --- | --- |
| `doc/zcode-integration/design.md` | 新增设计文档 |
| `doc/zcode-integration/checkpoint.yaml` | 新增任务拆分 |
| `doc/zcode-integration/changes.md` | 本文件 |
| `README.md` / `README_ZH.md` / `README_EN.md` / `README_JA.md` / `README_AR.md` | 支持矩阵：ZCode 改为支持；Kimi Work 修正为仅检测；补齐千问办公、豆包工作、扣子 |
| `doc/Agent适配资料清单.md` | ZCode 条目更新为完整接入，附待确认项 |

## 3. 验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过（已应用格式化） |
| `cargo clippy --all-targets -- -D warnings` | 通过，无警告 |
| `cargo test` | 144 passed / 0 failed |
| `npm test -- --run` | 86 passed / 0 failed |
| `npm run build` | 通过 |

ZCode 单测覆盖：写入结构、重复切换幂等、未知字段与其他 Provider 保留、
恢复官方配置只删受管项、写后校验通过与被篡改时失败、三种协议映射、Proxy 回退、空 Base URL 拒绝。

## 4. 未完成项与风险

1. **真机验收（t6）未执行。** 最关键的不确定性：应用内存在一处
   `access` 的 `omit({ apiKey: true })` 变体，用途未确认。若个人配置的实际写入路径走该变体，
   则明文 API Key 无法注入，整个方案失效，t2–t5 需回退为 detection-only。
2. **Windows 配置路径未验证。** `%APPDATA%/ZCode/v2/provider_config.json` 为按现有候选推导，
   无 Windows 真机样本。
3. **生效方式未验证。** 按 `needs_restart = true` 实现，重启后是否加载待确认。
4. **其余 5 个智能体未变更。** 千问办公（模型目录应用自管加密）、豆包工作（无配置入口）、
   扣子（本地数据为运行态/登录态）、Kimi Work（服务端下发 + 会话锁定）、
   AionClaw（沙盒 + npm 包管理）经真机探测确认无合法写入通道，维持仅检测。
5. **图标缺口未处理。** `qwenwork` / `doubaowork` / `coze` 仍无图标资源，列表页降级为通用图标。

## 5. 真机验证修正记录

第一次真机测试（安装 3.14.2、切换 `deepseek-v4-flash`）未见效果。ZCode 日志给出根因：

```
[provider-config] Personal Provider Config 加载失败，已保留磁盘状态并以内存空配置降级
manualProviderModelRules[0].config.properties  → expected object, received undefined
manualProviderModelRules[0].config.optionSpecs → expected object, received undefined
```

初版把模型写在 `modelConfigRules.manualProviderModelRules`，该数组要求完整能力块，缺失时
**整个个人文件被判为无效**并降级为空目录。

第二次修正：改为 ZCode 自身的真实结构——Provider 上声明 `group: "standard-personal"` +
`personalModelIds` / `modelOrder`，并把 `at-switch` 置入 `providerOrder` 首位，不再触碰
`manualProviderModelRules`。同时确认 `access.apiKey` **可以**明文写入（`ApiKeyAccessConfig`
只在未填时省略该字段，并非被剥离）。

该结构来自 ZCode 在 UI 中创建「新供应商」时写出的真实配置，非推测。

## 6. 真机验证期间的修复（t6）

2026-09-21 验收通过。按发现顺序：

| 问题 | 根因 | 修复 |
| --- | --- | --- |
| 切换后 ZCode 无变化 | 误把模型写进 `manualProviderModelRules`，缺能力块导致整个配置文件加载失败 | 改用 `config.personalModelIds` + `modelOrder` + `group: "standard-personal"` |
| 切换后模型消失 | 每次切换替换模型列表，历史模型被移除 | 同一 Provider 内累积保留，仅换 Provider 时重置 |
| 切换后不自动重启 | ZCode 主进程 `argv[0]` 只有 `ZCode`，按绝对路径匹配不到 | `macos_main_process_ids` 同时接受纯可执行文件名 |
| 重启后仍选中旧模型 | 选中状态有三层：任务表（权威）+ Local Storage（镜像）+ 配置默认值 | 暂停期间同时更新任务表与 Local Storage |

新增能力（`zcode.rs`）：`apply_task_model` / `restore_task_model`（SQLite）、
`apply_model_selection` / `restore_model_selection`（LevelDB），均支持回滚。

ZCode 测试 16 个、全量 Rust 153 个、前端 86 个全部通过，clippy 无警告。
仍未验证：Windows 配置路径无真机样本。
