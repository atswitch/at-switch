# Changelog

All notable changes to **AT-Switch** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Seven new switchable agents**:
  - **Hermes Agent** (macOS / Windows): Updates `$HERMES_HOME/config.yaml` or `~/.hermes/config.yaml`, preserving other YAML fields. Requires OpenAI Chat Completions for Direct mode.
  - **OpenCode** (macOS / Windows): Updates managed Provider and default model in user config, preserving JSONC comments, trailing commas, and third-party providers. Supports OpenAI Chat Completions.
  - **ZCode** (macOS / Windows): Updates `~/.zcode/v2/provider_config.json` managed Provider, model rules, and default model. Supports OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
  - **Trae CN** (macOS / Windows): Switches between user-configured custom models by rewriting `state.vscdb` selection records. Credentials are encrypted and managed by Trae; AT-Switch never reads or writes them.
  - **TRAE SOLO CN** (macOS / Windows): Same mechanism as Trae CN. Requires version 0.1.69+ (0.1.66 stored selection in an encrypted database, read-only).
  - **AionClaw** (macOS / Windows): Updates managed Provider and default model in sandbox `openclaw/state/openclaw.json`, sharing the QClaw implementation. Supports OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
  - **EasyClaw** (macOS / Windows): Updates `~/.easyclaw/easyclaw.json` managed Provider and `agents.defaults.model.primary`. Supports OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
- **Five detection-only agents** (installation status only, no config writing):
  - **QWEN Work** (千问办公): Custom models require server-side authorization; local toggle alone results in 403 rejection, so no configuration is written.
  - **Doubao Work** (豆包工作): No user-level Provider / BYOK configuration entry found.
  - **Coze** (扣子): Local data is runtime and login state, not model provider configuration.
  - **Kimi Work**: Daimon overwrites `model.current` with server-pushed default on every launch, making local writes ineffective.
  - **ima**: External writes to `kExtraSettingInfo` are reverted by ima within 11 seconds of launch; authoritative state is server-side.
- **Token usage logging**: In-memory usage log (cleared on process exit) records proxied request summaries and switch operations, with per-agent breakdown. Supports token extraction from OpenAI (`prompt_tokens` / `completion_tokens`), Anthropic (`input_tokens` / `output_tokens`), and streaming SSE responses.
- **Agent routing grid**: Per-agent proxy preference toggle. Enabling usage logging for an agent automatically switches it to Local Proxy mode.
- **GitHub Releases update check**: Automatic version comparison against the latest GitHub release, with semver-aware comparison including prerelease suffixes.
- **Proxy management UI**: Merged the standalone Local Proxy page into Settings. New components: `ProxyStatusBar`, `ProxyListenerSettings`, `AgentRoutingGrid`, `ProxyUsageByAgent`, `ProxyRecentRequests`, and `ProviderModelGroup`. Deep links to `?page=proxy` redirect to the Settings proxy tab.
- **Agent read-only details panel**: Displays detection-only agent status and configuration constraints.
- **`serde_yaml` dependency**: Added for Hermes `config.yaml` parsing.
- **THIRD_PARTY_NOTICES**: Added AionClaw and ZCode macOS application icons with maintainer-confirmed permission.

### Changed
- **Agent registry expanded** from 6 to 18 adapters (13 switchable, 5 detection-only).
- **README agent matrix** updated across all five language variants (Chinese, English, Japanese, Arabic, and default) to include the new agents with platform, protocol, and config-path details.
- **Settings page** restructured into tabs (`general` and `proxy`), replacing the removed standalone `ProxyPage`.
- **Switchboard page** enhanced with proxy usage panels and agent routing grid integration.
- **Proxy server** now extracts token usage from upstream JSON and streaming responses, recording per-agent summaries.
- **Streaming codec** gained a `CanonicalStreamEvent::Usage` variant and a `StreamUsageProbe` for transparent same-protocol usage tracking.

### Removed
- Standalone `ProxyPage.tsx` — functionality merged into `SettingsPage` under the `proxy` tab.

## [v3.15.2] - 2026-10-05

### Added
- Tencent ima model switching on macOS and Windows through its signed-in account model settings, limited to public OpenAI Chat Completions endpoints.
- Account-scoped encrypted recovery checkpoints, two-scene selection verification, interruption recovery, and preservation of existing custom models and unrelated local settings.

### Fixed
- Reuse an existing identical ima model before modifying a managed row, avoiding duplicate-model conflicts while preserving both rows and rollback behavior.
- Request a normal ima shutdown on Windows before switching, preventing incorrect-shutdown prompts after restart; preserve the stopped state when ima is not running.
- Isolate relaunched Windows agents' standard streams so their debug output does not enter AT-Switch logs.

### Security
- Upgrade rustls to 0.23.45 to fix RUSTSEC-2026-0285; upgrade Tauri to 2.12.1 and remove six unmaintained transitive dependencies, with upstream GTK macro diagnostic backports.
- Whole-lock Rust advisory checks and production npm audit pass without exceptions.

### Usage
- Install and sign in to ima, then refresh Agent status. First connection asks permission to save the provider URL, API key, and model name in the current Tencent ima account.
- Localhost, private-network endpoints, and the AT-Switch local proxy are not supported by the ima adapter. Save work before restarting and verify both entry points in new conversations.
- Restoration preserves existing user models and leaves the AT-Switch-created model unselected for reuse.

### Validation
- The macOS account round trip covers original model → third-party model → repeated switch → original model → third-party model → final restoration without creating duplicate model rows.
- The duplicate-model correction passes real-account GLM-5.2 and DeepSeek roundtrips, including repeated switching, stopped-app behavior and restoration with existing rows preserved. The earlier interpretation of code 100003 as a rate limit was incorrect.
- Frontend (107 tests), Windows Rust (189 tests), formatting, Clippy and Windows/macOS/Linux CI passed. The rebuilt Windows installer passes installation and startup checks; the user confirmed installed UI switching and restoration. The matching notarized Mac package was delivered and its native checks and functionality were confirmed by the builder. See the release verification record for evidence.

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
