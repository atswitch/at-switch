<div align="center">

# AT-Switch

### WorkBuddy、CodeBuddy、QClaw、AutoClaw 和 Codex 的全方位管理与模型切换工具

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

- **直连优先**：应用默认以原生直连方式修改 Agent 配置文件，零代理开销、无网络延迟。
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
