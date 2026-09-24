# TraeCode Agent 适配设计方案

> 更新日期：2026-09-20
> **修订：2026-09-22（部分失效，请先读下一节）**
> 对应规范：`Agent接入规范.md` §1-§5

---

## 0. 修订说明（2026-09-22，真机复核结论）

本方案基于 Trae CN 3.3.104 编写，其中**关于凭据与写入通道的判断已被真机推翻**，现记录实际结论：

| 原方案假设 | 真机结论 |
|---|---|
| 可向 `model_list_map` 写入 base_url / model_id / api_mode | ❌ 未采用。`model_list_map` 只是**模型清单**，改它不会改变当前生效的模型 |
| 凭据走 Keychain 分发、`ak` 为受管字段 | ❌ `ak` 由 Trae 自身加密管理，`Agent接入规范.md` §6.1 禁止逆向，AT-Switch **不读写任何凭据** |
| Trae CN 与 TRAE SOLO CN 均可切换 | ✅ 成立。**Trae CN 3.3.104** 与 **TRAE SOLO CN 0.1.69+** 均可切换；仅 SOLO 0.1.66 例外（选中态在加密库，只读） |

**实际落地的能力（以代码为准）**：

- **`traework`（Trae CN 3.3.104）— 完整接入（可切换）**。选中态明文存于
  `User/globalStorage/state.vscdb` 的 `AI.agent.model.recent_user_selection_by_agent_label`
  与 `AI.agent.model.session_selected_model`，AT-Switch 只改写其中的 `modelId`（格式
  `{label}_{config_source}_{provider}_{name}_{custom_model_id}`），**仅在用户已在 Trae 内添加过的自定义模型之间切换**，凭据不落盘。已真机验证写入生效。
- **`traecode`（TRAE SOLO CN 0.1.69+）— 完整接入（可切换）**。0.1.66 曾把选中态放进
  `ModularData/ai-agent/database.db`（文件头不是 `SQLite format 3`，加密库），实测在应用内
  选择模型后明文 `state.vscdb` **不会**出现任何选中态键，那时只能只读。**0.1.69 起选中态
  回到明文 `state.vscdb`**，键名（带 `3129680676791939:` 前缀的 `recent_user_selection_by_agent_label`
  与按会话 id 分层的 `session_selected_model`）及 id 形状均与 Trae CN 一致，故复用同一实现。

⚠️ 下文 §1 之后的写入设计（凭据注入、`model_list_map` 改写等）**均已废弃**，保留仅作设计过程记录。实现请以 `src-tauri/src/agents/trae_family.rs` 为准（`traework` 与 `traecode` 共用该实现）。

---

## 1. 接入定性（已废弃，见 §0）

**完整接入（可切换）** — Trae CN 与 TRAE SOLO CN 均可通过 vscdb SQLite 检测模型配置；当用户启用了 NMauto 自定义模型后，`model_list_map` 中会出现 `api_key` 字段。直连时我们直接向该数据库写入目标 Provider 的 base_url / model_id / api_mode；**凭据通过系统 Keychain 分发，不落盘**（数据库 `ak` / `sk` 字段为受管字段，归 `at-switch` 命名空间所有）。

---

## 2. 配置真相

### 2.1 配置文件路径

| 应用 | macOS 配置目录 | 凭据数据库 |
| --- | --- | --- |
| **Trae CN** (cn.trae.app) | `~/Library/Application Support/Trae CN/User/globalStorage/` | `state.vscdb` |
| **TRAE SOLO CN** (cn.trae.solo.app) | `~/Library/Application Support/TRAE SOLO CN/User/globalStorage/` | `state.vscdb` |

**数据库**：`state.vscdb` 是 SQLite，`ItemTable(key TEXT UNIQUE, value BLOB)`。

### 2.2 模型配置 key

每个应用内**存在两份**同一份模型配置，key 为：

| key | 说明 |
| --- | --- |
| `{userId}:AI.agent.model.model_list_map` | 带工作区维度（冒号分隔），可能过期 |
| `{userId}_AI.agent.model.model_list_map` | 通用 key，写时以此为准 |

当前实测 userId = `3129680676791939`。

### 2.3 JSON 结构

vscdb value 为 UTF-8 JSON，顶层结构：

```jsonc
{
  "builder": [
    {  // ← 这是一个自定义模型条目
      "name": "NMauto",
      "base_url": "https://openrouter.ai/api/v1",
      "ak": "sk-xxx",           // ← 写入时变为 at-switch 受管凭据引用
      "sk": null,
      "api_mode": "openai_chat_completions",  // 或 openai_responses
      "custom_model_id": "NMauto",
      "is_custom_base_url": true,
      "provider": "openrouter",
      "is_default": true,
      ...
    }
  ],
  ...
}
```

关键字段映射：

| JSON 字段 | 作用 | AT-Switch 写入行为 |
| --- | --- | --- |
| `name` / `custom_model_id` | 模型名称 | 设为 desired.model_id |
| `base_url` | Provider URL | 设为 desired.base_url |
| `ak` | API Key 明文 | **覆盖为 `${ENV_VAR}` 引用**（key 存在 Keychain） |
| `api_mode` | 协议 | openai_chat / openai_responses |
| `is_default` | 默认模型 | 设为 true |
| `provider` | 提供商标识 | 设为 `"at-switch"` |
| `is_custom_base_url` | 是否自定义 URL | 设为 true |
| `sk` | 备用 key（多为 null） | 保持 null |

### 2.4 凭据写入通道

Trae **不接受明文 Key 之外的其他引用方式**（无 `key_env`、无 `key_cmd`），所以 ak 字段**直接存放 Keychain 取出的真实 Key**。这不是 Hermes 的 `key_env` 模式，而是 Codex/OpenCode 风格的"明文 key 落盘到配置"——但配置在 vscdb（SQLite）里，由 Trae 自身加密管理。

> 安全保证：AT-Switch 在写入后**不读取也不缓存**该 key，凭据仅从 `SecretStore` → `build_config` 一次性注入。

### 2.5 非写入字段（运行态/缓存）

- `models-cache.json`（Electron 缓存）
- bridge 端口
- Cookie、Local Storage、Crashpad
- `iCubeAuthInfo`、`machineid`、`telemetry.*`、`DIPS`
- `User/settings.json` 中的 UI 状态

以上一律**不动**。

---

## 3. 协议边界

| 场景 | 协议 | 说明 |
| --- | --- | --- |
| 直连（Direct） | `ApiProtocol::OpenaiChatCompletions` 或 `OpenaiResponses` | 取决于 `api_mode` 当前值；直连写入保持相同协议 |
| 代理（Proxy） | `ApiProtocol::OpenaiChatCompletions` | 本地 Proxy 做转换 |

`validate_binding`：直连时 `api_mode` 必须在 Trae 支持的集合内（`openai_chat_completions` / `openai_responses` / `anthropic_messages`）。

---

## 4. 安装发现

### 4.1 形态：桌面应用（Electron .app）

### 4.2 macOS

| 应用 | Bundle ID | .app 路径 |
| --- | --- | --- |
| Trae CN | `cn.trae.app` | `/Applications/Trae CN.app` |
| TRAE SOLO CN | `cn.trae.solo.app` | `/Applications/TRAE SOLO CN.app` |

### 4.3 配置目录动态解析

```rust
// 1. HERMES_HOME 式环境变量 (Trae 无此变量，预留)
// 2. 标准 Application Support 目录（见 2.1 表）
```

### 4.4 自定义安装目录

支持前端 `custom_install_path` 指向 `Application Support/<AppName>/User/globalStorage/` 目录。

---

## 5. 生命周期

### 5.1 修改配置后是否需重启

**是，`needs_restart: true`**。

Trae 在启动时将 vscdb 中的 `model_list_map` 加载到内存，运行时不重读。修改后必须重启 Trae 才能生效。

`activate_native()` 流程：AT-Switch 调用系统 `open` 命令（macOS）让 Trae 成为 frontmost app，弹出自定义对话框：

> "AT-Switch 已更新 Trae 模型配置，请在 Trae 重启后验证。"

### 5.2 常驻状态

Trae 是 Electron 应用，可配置为 **LSUIElement**（无 Dock 图标，仅菜单栏）或普通应用。退出由用户手动完成。

---

## 6. 需要调用的 Trae 辅助能力（无需反向调用，仅读取）

| 能力 | 目的 | 实现方式 |
| --- | --- | --- |
| 读取 vscdb | 检测 + 写入配置 | sqlite3 CLI / rusqlite |
| 写入 vscdb | 更新 model_list_map | SQL `INSERT OR REPLACE` |
| Trae 进程查询 | 检测运行状态 | `ps aux \| grep -i trae` |
| Trae 重启提示 | 用户交互 | 前端弹窗（非系统调用） |

---

## 7. 错误码（`trae_` 前缀）

| 错误码 | 场景 |
| --- | --- |
| `trae_vscdb_not_found` | 找不到 `state.vscdb` |
| `trae_vscdb_read_err` | 读取 key 失败 |
| `trae_vscdb_write_err` | 写入 key 失败 |
| `trae_vscdb_json_err` | JSON 反序列化失败 |
| `trae_no_model_config` | `model_list_map` 不存在 |
| `trae_model_not_found` | 目标模型条目不存在 |
| `trae_invalid_api_mode` | api_mode 不在支持集合 |
| `trae_custom_install_invalid` | 自定义安装路径不可用 |

---

## 8. 实现概览

### 8.1 Rust Adapter (`src-tauri/src/agents/traecode.rs`)

```
TraeCodeAdapter
├── id()                    → "traecode"
├── display_name()          → "TraeCode"
├── detect()                → 两个 Bundle ID 逐一探测
│   ├── locate_desktop_app("Trae CN.app", "cn.trae.app", ...)
│   └── locate_desktop_app("TRAE SOLO CN.app", "cn.trae.solo.app", ...)
│   └── 读取 vscdb → 解析 model_list_map → 构建 AgentDetection
├── source_protocol()       → Direct: 保持 api_mode; Proxy: OpenaiChatCompletions
├── validate_binding()      → api_mode 合法性检查
├── build_config()          → SQLite 事务：更新 builder 条目
│   ├── ak → desired.credential (from SecretStore)
│   ├── base_url → desired.base_url
│   ├── name / custom_model_id → desired.model_id
│   ├── api_mode → 对应协议
│   ├── is_default → true
│   ├── provider → "at-switch"
│   └── is_custom_base_url → true
├── build_native_config()   → 新建空 vscdb + 默认 model 条目
├── companion_paths()       → 空（无伴随文件）
├── build_companion()       → 空
├── restore_companion()     → 空
├── verify_config()         → 逐字段 SQL 读取 + 比对
├── activation_required()   → true（必须重启 Trae）
└── native_activation_required() → true
```

### 8.2 启动激活流程

```
激活时:
1. 记录 Trae 当前 PID（用于确认重启成功）
2. 弹出前端对话框 → 用户点击"重启 Trae"
3. AT-Switch 执行: osascript -e 'tell application "Trae CN" to quit'
4. 等待进程退出 (超时 30s)
5. 执行: open "/Applications/Trae CN.app"
6. 轮询 ps 直到新 PID 出现（超时 60s）
7. 通知前端"Trae 已重启"
```

---

## 9. 前端改动

| 文件 | 改动 |
| --- | --- |
| `src/lib/agentCapabilities.ts` | `SWITCHABLE_AGENT_IDS` 加 `"traecode"`；`supportsDirectBinding("traecode")` → true；`directBindingRequirement("traecode")` → `"needs_restart"` |
| `src/lib/api.ts` | `mockAgentDisplayNames["traecode"] = "TraeCode"` |
| `src/components/AgentLogo.tsx` | 加 `traecode: TraeIcon`（新 SVG / 截图提取） |
| `src/assets/agents/traecode.png` | 图标资源 |

---

## 10. 测试矩阵

| 测试项 | 位置 | 覆盖内容 |
| --- | --- | --- |
| detect — installed | `traecode_tests.rs` | 找到 Trae CN / SOLO，能读到 model_list_map |
| detect — not installed | `traecode_tests.rs` | 无应用时返回 not_installed |
| detect — vscdb missing | `traecode_tests.rs` | 无数据库时返回 needs_setup |
| detect — no model config | `traecode_tests.rs` | 有 DB 但无 model_list_map key |
| build_config — new | `traecode_tests.rs` | 首次写入创建新条目 |
| build_config — update | `traecode_tests.rs` | 已有 NMauto 条目时覆盖更新 |
| build_config — preserve | `traecode_tests.rs` | 非目标条目不变 |
| build_native_config — new db | `traecode_tests.rs` | 新建空 vscdb + 默认条目 |
| verify_config — pass | `traecode_tests.rs` | 写入后验证通过 |
| verify_config — fail | `traecode_tests.rs` | 被外部修改后验证失败 |
| verify_config — api_mode | `traecode_tests.rs` | 不支持 api_mode 时 validate_binding 失败 |
| 双平台 | 手动 | macOS: Trae CN + TRAE SOLO CN 各测一次直连+代理 |

---

## 11. 已知限制

1. **Trae 仅接受明文 ak 字段**（无 key_env 机制），AT-Switch 必须将真实 Key 写入 vscdb。Key 在磁盘上由 Trae 自己的 SQLCipher（或类似加密）保护，AT-Switch 不额外加密。
2. **NMauto 模型名称冲突**：如果用户自定义模型名与 Trae 内置模型同名，`is_custom_base_url: true` 可区分。
3. **多工作区**：冒号分隔的 workspace key 优先于下划线 key；写入时两边都写。
4. **Windows**：本次不覆盖（后续可加）。

---

## 12. 文件清单（交付物）

| 文件 | 说明 |
| --- | --- |
| `src-tauri/src/agents/traecode.rs` | Adapter 实现 |
| `src-tauri/src/agents/traecode_tests.rs` | 单测 |
| `src-tauri/src/agents/mod.rs` | `mod traecode;` + `Box::new(TraeCodeAdapter)` |
| `src/lib/agentCapabilities.ts` | 前端能力声明 |
| `src/lib/api.ts` | display name mock |
| `src/components/AgentLogo.tsx` | 图标映射 |
| `src/assets/agents/traecode.png` | 图标资源 |
| `doc/traecode-integration.md` | 本文档 |
