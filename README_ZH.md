<div align="center">

# AT-Switch

### WorkBuddy、CodeBuddy、QClaw、AutoClaw、Codex、百度搭子、Hermes、OpenCode、Kimi Work、AionClaw、ZCode 与 ima 的全方位管理与模型切换工具

[![Version](https://img.shields.io/github/v/release/atswitch/at-switch?color=blue&label=version)](https://github.com/atswitch/at-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/atswitch/at-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/github/downloads/atswitch/at-switch/total)](https://github.com/atswitch/at-switch/releases/latest)

### 🌐 唯一官方网站：**[atswitch.io](https://atswitch.io)**

中文 | [English](README_EN.md) | [日本語](README_JA.md) | [العربية](README_AR.md) | [更新日志](CHANGELOG.md)

</div>

---

> [!WARNING]
>
> ## 唯一官方渠道声明（请务必阅读）
>
> AT-Switch 是**完全免费、开源**的桌面应用，**不会向用户收取任何费用**。请仅通过下列官方渠道获取本软件：
>
> | 类别 | 唯一官方链接 |
> | :--- | :--- |
> | **官网** | **[atswitch.io](https://atswitch.io)** |
> | **源码** | **[github.com/atswitch/at-switch](https://github.com/atswitch/at-switch)** |
> | **下载** | **[GitHub Releases](https://github.com/atswitch/at-switch/releases)** |
> | **问题反馈** | **[GitHub Issues](https://github.com/atswitch/at-switch/issues)** |
>
> **任何向您收费、要求充值或索取个人账号密码的“AT-Switch”网站或客户端均为假冒**。

---

## 软件简介

**AT-Switch** 是一款面向 macOS 和 Windows 的本地 AI Agent Provider 与大模型一键切换工具。

它将各个 AI Agent 繁琐分散的配置方式统一为同一套直观的桌面工作流：**选择智能体 → 维护供应商 → 选择模型 → 秒级切换**。

- **直连优先**：应用默认使用 Agent 的原生模型配置机制，不经过 AT-Switch 本地代理。
- **本地代理**：当需要跨协议转换（如 Codex Responses 与通用 Chat 协议互转）或密钥隔离时，可一键启用本地代理。
- **本地安全**：基于 Tauri 2、Rust、React 与 TypeScript 构建。敏感 API Key 存储在系统凭据库（macOS Keychain 或 Windows Credential Manager），应用绝不收集任何 Prompt 提示词、模型回复或日志。

---

## ✨ 核心特性

- **集中管理模型目录**：支持 DeepSeek、Moonshot Kimi、智谱 GLM、字节豆包、MiniMax、阿里通义千问等主流大模型及自定义兼容 Endpoint。
- **Agent 独立配置**：每个智能体独立维护绑定的 Provider、当前模型以及直连/代理模式。
- **多协议双向转换**：支持 **OpenAI Chat Completions**、**OpenAI Responses** 与 **Anthropic Messages** 协议之间的无损相互转换。
- **流式传输与工具调用**：内置专业编解码器，跨协议调用时完美支持 SSE 流式传输与 Function Calling。
- **事务级配置安全**：写入前自动生成加密快照备份，采用原子写、写后数据校验与失败自动回滚机制。
- **智能体生命周期感知**：自动发现已安装的 Agent，切换配置时支持安全重启正在运行的 Agent 进程。
- **一键复原原生状态**：随时可一键撤销 AT-Switch 接管，无缝恢复各 Agent 的原始配置。

---

## 💻 平台支持与安装包下载

所有正式版本均通过 [GitHub Releases](https://github.com/atswitch/at-switch/releases) 分发。

| 平台 | 最低系统要求 | 芯片架构 | 安装包类型 |
| :--- | :--- | :--- | :--- |
| **macOS** | macOS 12 Monterey 及以上 | Apple Silicon (M系列) / Intel / 通用 | `.dmg` 镜像 |
| **Windows** | Windows 10 / 11 | x64 | `.msi` 安装包 / 便携免安装版 (`.zip`) |

---

## 🤖 智能体（Agent）支持矩阵

| Agent | 自动检测 | 自动配置 | 默认请求协议 | 配置更新机制 |
| :--- | :--- | :--- | :--- | :--- |
| **WorkBuddy** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新 `~/.workbuddy/models.json`，保留用户自定义配置与思考模型设定 |
| **CodeBuddy CN** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新 `~/.codebuddy/models.json`，同步工作区默认值与当前会话选择 |
| **QClaw** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 基于 `~/.qclaw/qclaw.json` 定位并同步 OpenClaw 模型配置 |
| **AutoClaw** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新 Electron 用户数据目录中的权威模型设定 |
| **Codex** | macOS / Windows | ✅ 支持 | OpenAI Responses | 精确更新 `$CODEX_HOME/config.toml` 或 `~/.codex/config.toml`，保留原有注释 |
| **百度搭子（DuMate）** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新当前账号的 XDG 持久覆盖配置，兼容所有会话目录和内置模型别名 |
| **Hermes Agent** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新 `$HERMES_HOME/config.yaml` 或 `~/.hermes/config.yaml`，保留其余 YAML 字段 |
| **OpenCode** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 更新用户配置中的受管 Provider 与默认模型，保留 JSONC 注释、尾逗号和第三方 Provider |
| **ZCode** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | 更新 `~/.zcode/v2/provider_config.json` 中的受管 Provider、模型规则与默认模型，保留其他 Provider 与未知字段 |
| **Trae CN** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 在用户已配置于 Trae 内的自定义模型之间切换（改写 `state.vscdb` 的选中记录）；凭据由 Trae 加密自管，AT-Switch 不读取也不写入 |
| **TRAE SOLO CN** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions | 0.1.69 起选中模型已回到明文 `state.vscdb`，可像 Trae CN 一样在应用内已配置的自定义模型间切换；0.1.66 曾把选中态放在加密库，故需 0.1.69+ |
| **千问办公** | macOS / Windows | 🟡 仅检测 | — | 自定义模型是"客户端开关 + 服务端授权"双重门控：本地只能解除前者，模型能被选中但实际调用仍被服务端以 403 拒绝（`You do not have access to this model service`），因此不写入其配置 |
| **豆包工作** | macOS / Windows | 🟡 仅检测 | — | 未发现用户级 Provider / BYOK 配置入口，暂仅展示安装状态 |
| **扣子** | macOS / Windows | 🟡 仅检测 | — | 本地数据为运行态与登录态，不是模型供应商配置，暂仅展示安装状态 |
| **Kimi Work** | macOS / Windows | 🟡 仅检测 | — | Daimon 会在每次启动时用服务端下发的默认模型覆盖 `model.current`（实测连官方模型名 `k3-agent` 也会被重置为 `k2d8-preview`），运行态 TOML 的 `default_model` 因此永远回落到官方模型。写入的自有 Provider 虽会出现在运行态 TOML 中，但不会被选中，本地无可用写入通道，故不修改其配置 |
| **AionClaw** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | 更新沙盒内 `openclaw/state/openclaw.json` 的受管 Provider 与默认模型，与 QClaw 复用同一套实现 |
| **EasyClaw** | macOS / Windows | ✅ 支持 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | 同样基于 OpenClaw 内核（应用内 `gateway.asar/openclaw.mjs`），更新 `~/.easyclaw/easyclaw.json` 的受管 Provider 与 `agents.defaults.model.primary`；该文件即权威配置（`EASYCLAW_CONFIG_DIR` 指向 `~/.easyclaw`），无需再写第二处 |
| **腾讯 ima** | macOS / Windows | 当前实现，验收范围见下文 | 公网 OpenAI Chat Completions | 连接本机已登录账号，同步 ima 账号模型设置与两个入口的本地选择 |

### ima 使用说明

顶部选择 **ima**，在现有供应商与模型列表中点击「切换」。首次连接需确认使用本机已登录的 ima 账号；macOS 还可能提示钥匙串访问授权，无需手动复制登录凭据。所选接口地址、API Key 和模型名会按 ima 的自定义模型机制保存到腾讯 ima，并同时用于「问问 ima」和「我的 copilot」。

ima 当前仅提供**公网 OpenAI Chat 直连**，不支持 AT-Switch 本地代理或仅本机可访问的接口。运行中切换会安全退出并重新打开 ima，请先等待当前生成完成。「恢复原始模型」恢复首次接管前两个入口各自的选择，保留已有自定义模型。

当前实现与验证范围见 [ima 接入与验收记录](IMA_INTEGRATION.md)。macOS 完整切换与恢复验收尚未完成；Windows 目前仅完成静态研究与编译检查，未经真机验收。此说明不代表新增功能已经发布。

---

## 🛠️ 本地编译与构建

### 前置要求
- [Node.js](https://nodejs.org/) (>= 20)
- [Rust](https://www.rust-lang.org/) (稳定版工具链)
- 操作系统编译依赖：
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools & WebView2 Runtime

### 开发运行

```bash
# 克隆仓库
git clone https://github.com/atswitch/at-switch.git
cd at-switch

# 安装前端依赖
npm ci

# 启动桌面开发模式（热重载）
npm run tauri dev
```

### 运行质量门禁与测试

```bash
# 前端编译与自动化测试
npm run build
npm test -- --run

# Rust 格式化与 Clippy 静态检查
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Rust 单元与集成测试
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 📄 开源许可证

本项目基于 [MIT License](LICENSE) 开源。
