<div align="center">

# AT-Switch

### The All-in-One Manager & Model Switcher for Codex, WorkBuddy, CodeBuddy, QClaw & AutoClaw

[![Version](https://img.shields.io/github/v/release/atswitch/at-switch?color=blue&label=version)](https://github.com/atswitch/at-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/atswitch/at-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/github/downloads/atswitch/at-switch/total)](https://github.com/atswitch/at-switch/releases/latest)

### 🌐 The Only Official Website: **[atswitch.io](https://atswitch.io)**

[中文](README.md) | English | [日本語](README_JA.md) | [Deutsch](README_DE.md) | [Changelog](CHANGELOG.md)

</div>

---

> [!WARNING]
>
> ## Official Channel Statement (Please Read)
>
> AT-Switch is a **completely free, open-source** desktop application and **never charges any fees**. Please obtain software exclusively via the following official channels:
>
> | Category | Official Link |
> | :--- | :--- |
> | **Website** | **[atswitch.io](https://atswitch.io)** |
> | **Source Code** | **[github.com/atswitch/at-switch](https://github.com/atswitch/at-switch)** |
> | **Releases** | **[GitHub Releases](https://github.com/atswitch/at-switch/releases)** |
> | **Issues** | **[GitHub Issues](https://github.com/atswitch/at-switch/issues)** |
>
> Any website or client that requests payment, balance recharges, or personal login credentials in the name of "AT-Switch" is fraudulent.

---

## Introduction

**AT-Switch** is a native desktop management tool designed for developers on macOS and Windows to easily configure and switch LLM providers and models across multiple coding Agents.

Instead of hunting down scattered, application-specific configuration files, AT-Switch standardizes the workflow: **Select Agent → Manage Providers → Pick Model → Switch**. 

By default, switching is executed directly against the native configurations of target agents. When protocol translation or credential isolation is required, an optional built-in local proxy can be activated with a single click.

Built with **Tauri 2, Rust, React, and TypeScript**, AT-Switch prioritizes security and performance. Sensitive API keys are securely stored in native system credential vaults (macOS Keychain or Windows Credential Manager) without ever logging prompts or raw request bodies.

---

## ✨ Key Features

- **Centralized Provider & Model Catalog**: Manage multiple upstream providers (DeepSeek, Kimi, Zhipu GLM, Doubao, MiniMax, Qwen, etc.) and their custom endpoints in one place.
- **Per-Agent Independence**: Maintain separate model bindings, active configurations, and connection modes for each Agent.
- **Direct Mode by Default**: Direct configuration writing removes network hops and latency; the local proxy is reserved under Advanced Settings for compatibility needs.
- **Multi-Protocol Translation**: Seamless interoperability between **OpenAI Chat Completions**, **OpenAI Responses**, and **Anthropic Messages** protocols.
- **Streaming & Tool Calling**: Built-in codec preserves streaming chunks and function call structures across protocol transitions.
- **Transactional Config Rollback**: Encrypted backups are automatically created prior to writing changes, with atomic file writes, validation pre-checks, and automatic rollback on failure.
- **Automatic Lifecycle Management**: Automatically detects running Agent processes and performs graceful restarts when configuration changes.
- **Native Security & Zero Cloud Dependency**: Zero collection of user prompts, completions, or telemetry. Runs strictly on `127.0.0.1`.

---

## 💻 Supported Platforms & Download

All official release binaries are hosted on [GitHub Releases](https://github.com/atswitch/at-switch/releases).

| Platform | Minimum OS | Architecture | Package Format |
| :--- | :--- | :--- | :--- |
| **macOS** | macOS 12 Monterey | Apple Silicon / Intel / Universal | `.dmg` |
| **Windows** | Windows 10 / 11 | x64 | `.msi` / `.exe` / `-Portable.zip` |

---

## 🤖 Supported Agents Matrix

| Agent | Auto-Detection | Direct Mode | Default Protocol | Configuration Mechanism |
| :--- | :--- | :--- | :--- | :--- |
| **WorkBuddy** | macOS / Windows | ✅ Supported | OpenAI Chat Completions | Updates `~/.workbuddy/models.json`, preserves user custom entries |
| **CodeBuddy CN** | macOS / Windows | ✅ Supported | OpenAI Chat Completions | Updates `~/.codebuddy/models.json`, syncs workspace defaults |
| **QClaw** | macOS / Windows | ✅ Supported | OpenAI Chat Completions | Locates and syncs OpenClaw configuration via `~/.qclaw/qclaw.json` |
| **AutoClaw** | macOS / Windows | ✅ Supported | OpenAI Chat Completions | Manages authoritative model catalogs in Electron user data |
| **Codex** | macOS / Windows | ✅ Supported | OpenAI Responses | Updates `$CODEX_HOME/config.toml` or `~/.codex/config.toml` cleanly |

---

## 🛠️ Local Development & Build

### Prerequisites
- [Node.js](https://nodejs.org/) (v20 or higher)
- [Rust](https://www.rust-lang.org/) (stable toolchain)
- Platform build dependencies:
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools & WebView2

### Setup & Run

```bash
# Clone the repository
git clone https://github.com/atswitch/at-switch.git
cd at-switch

# Install frontend dependencies
npm ci

# Run development mode (Hot Reload frontend + Rust backend)
npm run tauri dev
```

### Run Tests & Quality Gates

```bash
# Frontend build & unit tests
npm run build
npm test -- --run

# Rust format check & Clippy
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Rust unit & integration tests
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 📄 License

This project is open-sourced under the [MIT License](LICENSE).
