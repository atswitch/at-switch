# ZCode 完整接入设计方案

> 更新日期：2026-09-21
> 对应规范：`doc/Agent接入规范.md` §1、§6.1、§6.2
> 前置结论：推翻既有 detection-only 定性，升级为**完整接入（可切换）**

---

## 1. 接入定性变更

原 `detection_only.rs` 的 `ZCODE_ADAPTER` 注释写的是「`provider_config.json` schema 为空，实际
Provider 由应用内置 `zcode-builtin.json` 决定」。真机复核（ZCode 已安装，`~/.zcode/v2/` 存在）
发现该结论只对了一半：文件里的数组确实是空的，但**空数组正是官方预留给用户手动声明第三方
Provider 的写入通道**，应用侧已实现完整的 zod 校验与迁移器。

定性为**完整接入**：ZCode 暴露稳定用户级配置入口，允许第三方声明 `base_url` + `api_key` +
`model_id`，满足接入规范 §6.1 的三要素。

---

## 2. 调研事实（落实规范 §6.1）

### 2.1 安装与路径

| 项 | macOS | Windows |
| --- | --- | --- |
| 应用 | `ZCode.app` | `ZCode.exe`（待真机确认） |
| Bundle ID | `dev.zcode.app` | — |
| 个人配置 | `~/.zcode/v2/provider_config.json` | `%APPDATA%/ZCode/v2/provider_config.json`（按现有候选推导，待确认） |

应用内部将该文件标识为 `ZCODE_PERSONAL_PROVIDER_CONFIG_FILE`，与内置配置
（`ZCODE_BUILTIN_PROVIDER_CONFIG_FILE`、`.../Resources/config/provider/zcode-builtin.json`）
分离。内置配置由服务端下发 `builtin_provider_config_json` 覆盖，**不是写入真相**。

### 2.2 配置 Schema（来自应用内 zod 定义）

顶层（严格模式，未知字段会被拒绝）：

```
schemaVersion: 1
config: {
  providerOrder?: string[]
  providerConfigRules: { providerRules: [...] }
  modelConfigRules:    { providerModelRules: [...], manualProviderModelRules: [...] }
  defaultModelSelection?: { providerId, modelId, options?: { reasoningLevel? } }
}
```

`providerRules[]` 元素：

```
{ providerId, templateId?, providerName?, enabled?,
  config: { access, api, ... } }
```

- `access`：`{ type: "api-key" | "zhipu-coding-plan-api-key", apiKey?: string, apiKeyManagementUrl?: string }`
  —— **`apiKey` 是明文字段**，这是凭据写入通道成立的关键依据。
- `api`：`{ type, baseUrl, headers? }`，`type` ∈ `anthropic-messages` /
  `openai-chat-completions` / `openai-responses`，`baseUrl` 必须是合法 URL。

**模型不由 `manualProviderModelRules` 声明**（2026-09-21 真机验证修正）。该数组是给内置
Provider 补模型能力用的，元素 schema 为 `hz`：`{ enabled, properties, optionSpecs }` 三者必填，
`properties` 要 8 个字段、`optionSpecs` 的 `map` 还要是合法表达式。实测漏写时应用日志报

```
[provider-config] Personal Provider Config 加载失败，已保留磁盘状态并以内存空配置降级
path: ["manualProviderModelRules", 0, "config", "properties"]  → expected object, received undefined
path: ["manualProviderModelRules", 0, "config", "optionSpecs"] → expected object, received undefined
```

且**整个个人文件加载失败**并降级为空目录——不是只忽略这一条。

第三方 Provider 的模型应声明在 Provider 自身上：`config.personalModelIds` 与
`config.modelOrder`，配合 `group: "standard-personal"`。这是 ZCode 自己在 UI 中创建「新供应商」
时写出的真实结构，也是 `_z` 为 personal provider 保留 `personalModelIds` / `modelOrder` 的原因。
不再触碰 `manualProviderModelRules`。

### 2.3 必须遵守的校验约束

1. **`.strict()`**：顶层与 `providerRules` 元素均为严格模式，写入**不得引入未知字段**。
2. `providerId` 以 `account:` 开头且携带 `access` 时会校验失败——受管 Provider 固定用 `at-switch`，不带前缀。
3. `manualProviderModelRules` 与 `providerModelRules` **不能声明同一 `(providerId, modelId)`**。
   受管模型走 Provider 自身的 `personalModelIds`，两者都不写。
4. `config.group` 取 `standard-personal`（`WEe` 枚举），否则不会按个人 Provider 处理。
4. `baseUrl` 必须是 URL；`api.type` 只能取上述三值。

### 2.4 Endpoint 语义

内置模板的 `baseUrl` 均为**根地址**（如 `https://api.openai.com/v1`、
`https://api.z.ai/api/paas/v4`），由 ZCode 自行拼接 `/chat/completions` 等路径。AT-Switch 直接
写入 Provider 保存的 Base URL，不额外拼接，符合规范 §7.1「Base URL 已含协议路径时不得重复拼接」。

### 2.5 生效方式

配置改动后需重启 ZCode 才能加载（配置在服务启动时读取）。`needs_restart` 取 `true`，写入前
按统一确认流程安全退出，写入后按原状态恢复。

---

## 3. 受管写入内容

写入示例（Direct，OpenAI Chat）：

```jsonc
{
  "schemaVersion": 1,
  "config": {
    "providerConfigRules": {
      "providerRules": [
        {
          "providerId": "at-switch",
          "providerName": "AT-Switch",
          "enabled": true,
          "config": {
            "group": "standard-personal",
            "access": { "type": "api-key", "apiKey": "<凭据>" },
            "api": { "type": "openai-chat-completions", "baseUrl": "<provider base url>" },
            "personalModelIds": ["<model id>"],
            "modelOrder": ["<model id>"],
            "visibility": "visible"
          }
        }
      ]
    },
    "defaultModelSelection": { "providerId": "at-switch", "modelId": "<model id>" }
  }
}
```

### 3.1 受管字段（只改这些）

- `config.providerConfigRules.providerRules[]` 中 `providerId == "at-switch"` 的条目
  （含其 `config.personalModelIds` / `config.modelOrder`）
- `config.providerOrder`：把 `at-switch` 置到首位，其余顺序原样保留
- `config.defaultModelSelection`（仅当其 `providerId == "at-switch"`）

### 3.2 保留字段

其他 Provider 条目、`providerOrder`、用户已有的未知字段一律原样保留。严格模式下**只保留不新增**，
适配器不得自行发明字段。

### 3.3 恢复官方配置

移除上述 3 类受管内容；若 `defaultModelSelection` 指向 `at-switch` 则整体清除。接管后用户新增的
其他 Provider 不受影响。

---

## 4. 协议与模式

| AT-Switch 协议 | ZCode `api.type` |
| --- | --- |
| `OpenaiChatCompletions` | `openai-chat-completions` |
| `OpenaiResponses` | `openai-responses` |
| `AnthropicMessages` | `anthropic-messages` |

- **Direct**：写入 Provider 的 `base_url`、真实模型 ID 与凭据。三种协议均原生支持，无需转换。
- **Proxy**：写入本地 Endpoint 与本地令牌，`api.type` 固定 `openai-chat-completions`，由本地代理
  完成上游鉴权与协议转换。

模型名一律使用真实模型 ID，不拼接内部标识。

---

## 5. 失败与回滚

写入全程走 `ConfigTransaction`：读取 → 保存原始基线并加密备份 → 校验备份可恢复 → 同目录临时文件 →
原子替换 → 重新读取并校验 → 失败回滚 → 成功后才更新绑定状态。

稳定错误码：

| 错误码 | 场景 |
| --- | --- |
| `zcode_config_path_missing` | 未定位到配置文件 |
| `zcode_config_parse_failed` | 文件不是合法 JSON 或结构不符 |
| `zcode_protocol_unsupported` | 上游协议无法映射到 `api.type` |
| `zcode_write_verification_failed` | 写后重新读取校验不通过 |

---

## 6. 真机验证结论

2026-09-21 真机验收通过。以下几条是被真机实测**推翻或修正**的早期判断，记录在此以免重走：

1. **`access.apiKey` 明文写入被接受。** 应用内那处 `omit({apiKey:true})` 只在字段未填时省略，
   并非剥离凭据。
2. **模型声明在 Provider 自身**：`config.personalModelIds` + `config.modelOrder` +
   `group: "standard-personal"`。误写进 `modelConfigRules.manualProviderModelRules` 会因缺少
   `properties`/`optionSpecs` 能力块而让**整个文件加载失败**并降级为空目录。
3. **"当前选中模型"有三层状态，只改配置文件不会改变选中**：
   - `tasks-index.sqlite` 的 `tasks.model`（形如 `at-switch/<模型>`）是**权威来源**；
   - Chromium Local Storage 的 `modelSelection` 是启动时从任务表刷出的**镜像**；
   - `provider_config.json` 的 `defaultModelSelection` 不参与"当前选中"，只影响新建任务的取值。
   因此适配器在 ZCode 暂停期间**同时**更新任务表与 Local Storage。
4. **必须重启才生效**，且 ZCode 退出时会把内存状态回写配置文件——所以写入必须发生在它退出之后。
   这由 `pause_for_config_update` 保证。
5. **进程检测**：ZCode 主进程 `argv[0]` 只有 `ZCode`（不含完整路径），原先只按绝对路径匹配会误判
   其"未运行"，从而跳过暂停与重启。`macos_main_process_ids` 已改为同时接受纯可执行文件名。

仍未验证：**Windows 配置路径**未在 Windows 真机确认（按 `%APPDATA%` 推导，无样本）。
