<p align="center">
  <img src="./src-tauri/icons/128x128.png" width="112" alt="AT-Switch logo" />
</p>

<h1 align="center">AT-Switch</h1>

<p align="center">
  <a href="https://github.com/atswitch/at-switch/actions/workflows/ci.yml"><img src="https://github.com/atswitch/at-switch/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="MIT License" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg" alt="macOS and Windows" />
</p>

<p align="center">
  <a href="./README.md">简体中文</a> | English
</p>

AT-Switch is a local-first desktop application for managing AI provider and model
configuration across multiple AI agents on macOS and Windows. It is built with
Tauri 2, Rust, React, and TypeScript.

AT-Switch stores Provider API keys in macOS Keychain or Windows Credential Manager.
Direct mode also writes the selected key to the target agent's native configuration;
proxy mode avoids that extra copy. AT-Switch does not persist prompts, model
responses, tool arguments, or per-request logs.

## Features

- Manage providers and model catalogs in one place.
- Keep an independent provider, model, and connection mode for each agent.
- Switch between AT-Switch-managed configuration and an agent's original setup.
- Use direct connections by default, with an optional local proxy for protocol
  conversion and credential isolation.
- Support OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
- Preserve streaming and tool-calling semantics across supported conversions.
- Back up agent configuration before atomic writes, validate the result, and roll
  back failures.
- Detect installed agents at startup, when the window regains focus, and on manual
  refresh.
- Restore the configuration captured before AT-Switch first managed an agent.
- Switch between Simplified Chinese and English without restarting the app.

## Supported Platforms

| Platform | Minimum | Packages |
| --- | --- | --- |
| macOS | macOS 12 | Apple Silicon, Intel, and Universal `.app` / `.dmg` |
| Windows | Windows 10/11 | x64 executable and NSIS installer |

CI runs frontend checks, Rust formatting, Clippy, Rust tests, and a Tauri desktop
link build on macOS and Windows.

## Supported Agents

| Agent | Detection | Configuration | Native protocol |
| --- | --- | --- | --- |
| WorkBuddy | macOS / Windows | Supported | OpenAI Chat Completions |
| CodeBuddy CN | macOS / Windows | Supported | OpenAI Chat Completions |
| QClaw | macOS / Windows | Supported | OpenAI Chat Completions |
| AutoClaw | macOS / Windows | Supported | OpenAI Chat Completions |
| Codex | macOS / Windows | Supported | OpenAI Responses |
| ima | macOS / Windows | Detection only | OpenAI-compatible |
| TRAE | macOS / Windows | Detection only | OpenAI-compatible |

AT-Switch does not read login cookies or modify undocumented internal databases
for detection-only agents.

## Direct and Proxy Modes

Direct mode writes the selected provider endpoint, real model ID, and API key to
the agent's native configuration. Requests do not pass through AT-Switch, and the
provider must support the protocol used by the selected agent.

The optional local proxy listens only on `127.0.0.1`. Agent configuration contains
a high-entropy local routing token, while the real provider key remains in the
operating system credential store. The proxy performs protocol conversion only
when required.

## Install, Upgrade, and Uninstall

Download public, platform-signed packages from
[GitHub Releases](https://github.com/atswitch/at-switch/releases/latest). Draft,
unsigned, and ad-hoc-signed artifacts are for maintainer verification only.

Download the matching `.sha256` file and verify it before installation:

```bash
# macOS: replace the placeholder with the downloaded DMG filename
shasum -a 256 -c "AT-Switch.dmg.sha256"
```

```powershell
# Windows: replace the placeholder with the downloaded EXE or MSI filename
$installer = "AT-Switch-installer.exe"
$expected = (Get-Content "$installer.sha256").Split()[0].ToLowerInvariant()
$actual = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "Checksum mismatch" }
```

On macOS, open the DMG and drag AT-Switch into Applications. On Windows, run the
signed EXE or MSI and follow the installer. To upgrade, quit AT-Switch completely
and install the new version over the existing installation; local data is retained.

Before uninstalling, restore every managed agent to **Agent original configuration**
and verify the restoration. Otherwise, endpoints, model selections, and direct-mode
API keys already written to agent configuration are not removed with AT-Switch.
A normal uninstall may retain app data and OS credentials for upgrades or reinstalls.
For an irreversible full cleanup, first back up anything needed, then remove the
following app-data directory and OS credential entries whose service name is
`com.atswitch.desktop`:

- macOS: `~/Library/Application Support/com.atswitch.desktop/` and the matching
  items in Keychain Access;
- Windows: `%APPDATA%\com.atswitch.desktop\` and the matching items in Credential
  Manager.

## Getting Started

Requirements:

- Node.js 20 or later
- npm 10 or later
- Rust 1.85 or later
- The [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/)

```bash
git clone https://github.com/atswitch/at-switch.git
cd at-switch
npm ci
```

Run the browser mock:

```bash
npm run dev
```

Run the native desktop application:

```bash
npm run tauri:dev
```

## Quality Checks

```bash
npm run build
npm test -- --run
npm run licenses:check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Tests use temporary directories, test databases, and in-memory credentials. They
do not read real API keys or modify real agent configuration.

## Packaging and Releases

Build an installer on its native platform:

```bash
npm run tauri:build
```

Artifacts are written to `src-tauri/target/release/bundle/`.

Pushing a semantic `v*` tag that matches all version manifests creates a draft
GitHub Release, builds macOS and Windows installers, and uploads SHA-256 checksum
files. Maintainers must verify platform signatures and run installation smoke
tests before publishing the draft. Without Apple credentials, the macOS build uses
an ad-hoc signature and is not notarized. Never publish an unsigned or ad-hoc-signed
draft.

## Security and Privacy

| Data | Storage |
| --- | --- |
| Providers, models, agent bindings, settings | Local SQLite database |
| Provider API keys | AT-Switch stores them in the OS credential store; direct mode also writes the selected key to the target agent's native configuration |
| Local routing tokens | OS credential store |
| Original agent configuration backups | Encrypted `.atsb` files in the app data directory |
| Prompts, responses, tool arguments, per-request logs | Not stored |

Do not post API keys, complete configuration files, credentials, or private request
content in public issues. See [SECURITY_EN.md](./SECURITY_EN.md) for private vulnerability
reporting.

## Community

- [Contributing guide](./CONTRIBUTING_EN.md)
- [Code of Conduct](./CODE_OF_CONDUCT.md)
- [Support](./SUPPORT.md)
- [Changelog](./CHANGELOG.md)
- [Third-party notices](./THIRD_PARTY_NOTICES.md)

## License

AT-Switch source code is available under the [MIT License](./LICENSE). Third-party
names, logos, and trademarks remain the property of their respective owners; see
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).
