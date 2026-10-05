# Changelog

All notable changes to **AT-Switch** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [v3.15.2] - 2026-10-05

### Added
- Tencent ima model switching on macOS and Windows through its signed-in account model settings, limited to public OpenAI Chat Completions endpoints.
- Account-scoped encrypted recovery checkpoints, two-scene selection verification, interruption recovery, and preservation of existing custom models and unrelated local settings.

### Fixed
- Request a normal ima shutdown on Windows before switching, preventing incorrect-shutdown prompts after restart; preserve the stopped state when ima is not running.
- Isolate relaunched Windows agents' standard streams so their debug output does not enter AT-Switch logs.

### Usage
- Install and sign in to ima, then refresh Agent status. First connection asks permission to save the provider URL, API key, and model name in the current Tencent ima account.
- Localhost, private-network endpoints, and the AT-Switch local proxy are not supported by the ima adapter. Save work before restarting and verify both entry points in new conversations.
- Restoration preserves existing user models and leaves the AT-Switch-created model unselected for reuse.

### Validation
- The macOS account round trip covers original model → third-party model → repeated switch → original model → third-party model → final restoration without creating duplicate model rows.
- Windows installation, native credentials, switching and restoration while running and stopped, normal restarts, real responses in Ask ima and My copilot, and actual copilot tool execution passed with GLM-5.2 and DeepSeek V4 Flash.
- Frontend (107 tests), Windows Rust (187 tests), formatting, Clippy, dependency-license, and existing-Agent regression gates passed. Platform functionality and dependency security audits are recorded separately in the release verification record.

## [v3.15.1] - 2026-10-03

### Fixed
- DuMate switching now verifies every native model alias in the effective account override, preventing a stale route from being reported as successfully applied.
- The settings footer now reads the packaged application version and current runtime platform instead of showing a stale hard-coded version and both platforms.
- macOS release builds now require a complete Developer ID signing configuration and remain draft-only until signature, notarization, and clean-machine checks are complete.

## [v3.14.2] - 2026-09-08

### Added
- Baidu DuMate detection and model switching on macOS and Windows.
- Account-scoped `opencode.jsonc` overrides that persist across restarts, including default models, small models, and native aliases used by existing conversations and artifact validation.
- Restoration of original DuMate configuration while preserving unrelated user settings and custom providers.

### Usage
- Sign in to DuMate and open its coding agent once before refreshing Agent status.
- Direct mode requires OpenAI Chat Completions. Use the local proxy for other upstream protocols.
- Windows x64 and macOS Universal installers are identical on the official website and GitHub Releases, with published SHA-256 checksums.

## [v3.14.1] - 2026-09-04

### Added
- **Initial Open-Source Release**: Open-sourced the core AT-Switch desktop application under the MIT License.
- **Agent Support Matrix**:
  - Support for **WorkBuddy**: Automated config synchronization with `~/.workbuddy/models.json`.
  - Support for **CodeBuddy CN**: Workspace model syncing via `~/.codebuddy/models.json`.
  - Support for **QClaw**: Automatic OpenClaw configuration adaptation via `~/.qclaw/qclaw.json`.
  - Support for **AutoClaw**: Electron user data authoritative catalog switching.
  - Support for **Codex**: Full support for config-only switching in `$CODEX_HOME/config.toml` or `~/.codex/config.toml`.
- **Protocol Translation & Local Proxy**:
  - Dual-mode switching: Direct mode (default, zero latency) and Local Proxy mode (on `127.0.0.1`).
  - Seamless bidirectional translation across OpenAI Chat, OpenAI Responses, and Anthropic Messages.
  - SSE streaming and Tool Calling / Function Calling compatibility layer.
- **Security & Privacy**:
  - OS-native credential storage via macOS Keychain and Windows Credential Manager.
  - Zero plain-text persistence of API keys in application databases.
  - Zero storage of prompts, model answers, or request bodies.
- **Transactional Config Safety**: Pre-write snapshot encryption, atomic writes, and rollback on failure.
- **Multi-language Documentation**: Added Chinese, English, Japanese, and Arabic READMEs.
