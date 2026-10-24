# Changelog

All notable changes to **AT-Switch** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [v3.18.2] - 2026-10-24

### Changed
- Saving a provider now runs a connection test with the draft values before persisting. Text-model drafts with an unreachable endpoint, wrong key, or broken protocol are rejected with the real connection error (per-protocol detail) instead of being saved silently. Drafts without text models pass straight through; editing a provider without a new API key falls back to the stored credential for the check.
- When the save-time connection test passes and the tested text model declares no streaming/tool capabilities, it is marked Verified immediately, so saving once is enough to switch.

### Added
- 获取模型列表 in the provider form: fetch the upstream `GET {base}/models` list (OpenAI-compatible `type` field mapped to 文本/生图/语音/视频), pick models from a searchable dialog, and merge the selection into the model rows. The Mongyun preset now suggests `doubao-seedream-5.0-pro`.

## [v3.18.0] - 2026-10-21

### Added
- Hermes Agent integration ported from the Hermes delivery line: managed Provider and default model in `$HERMES_HOME/config.yaml`, `~/.hermes/config.yaml`, or the Hermes Agent CN Desktop data home `%LOCALAPPDATA%\Hermes Agent CN Desktop\data\hermes-home\` (YAML, serde_yaml), Direct limited to OpenAI Chat Completions with Proxy conversion for other protocols, and ConfigTransaction baseline, encrypted backup and rollback. The desktop installation is discovered first so pause/restart targets the desktop app; the API key is referenced via the `HERMES_AT_SWITCH_API_KEY` environment variable instead of plain text, and automatically upserted into the `.env` next to the managed config on every switch (preserving the user's other entries) so verification no longer fails on a missing credential file.

## [v3.17.6] - 2026-10-20

### Added
- AionClaw desktop (com.quyuanai.aionclaw) integration reusing the OpenClaw write kernel with the authoritative state in its sqlite database (`kv.app_config` + `agents.model` dual write): Direct passthrough for OpenAI Chat / Responses / Anthropic Messages and Proxy conversion, with ConfigTransaction baseline, encrypted backup and rollback.

## [v3.17.2] - 2026-10-20

### Added
- A ZCode integration that declares the managed provider in ZCode's personal provider configuration and syncs the selected model in its task index and Chromium local storage, with Direct and Proxy support for OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.

### Fixed
- macOS main-process detection now also matches processes that report only the executable name, so ZCode is paused and relaunched for a configuration change instead of being reported as not running.

## [v3.17.0] - 2026-10-20

### Added
- EasyClaw desktop (ai.easyclawcn.desktop) integration reusing the OpenClaw write kernel: managed Provider and `agents.defaults.model.primary` in `~/.easyclaw/easyclaw.json`, Direct passthrough for OpenAI Chat / Responses / Anthropic Messages and Proxy conversion, with ConfigTransaction baseline, encrypted backup and rollback.
- A `build_sidecar_updates` adapter hook that applies companion-state updates (session activation models, credential pointers) inside the same transaction window as the main configuration write, rolling back together on failure.

### Fixed
- OpenClaw-family agents (QClaw / AutoClaw / EasyClaw) kept requesting the pre-switch model after a switch: the active model is anchored in three additional places beyond `agents.defaults.model`. Switching now also rewrites `agents.list[*].model.primary` entries that point at custom providers, the per-session `model` fields in `agents/main/sessions/sessions.json`, and the bare model ids in session thread transcripts (`*.jsonl`) that the gateway resumes threads from. Official `easyclaw/…` prefixed ids and user configuration are preserved.

### Changed
- Restore the default Vite `dist/` cleanup in builds; the previous `emptyOutDir: false` bypass targeted the retired Z: network-drive packaging path.

## [v3.16.1] - 2026-10-12

### Added
- A unified TRAE integration for TRAE CN 3.4.2, with Direct-mode support for OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages.
- Encrypted, account-scoped recovery of the original selection and managed-model reuse.
- An accessible Agent selector that keeps the active Agent reachable when the list exceeds six entries.

### Changed
- Retire the old TraeCode / TraeWork adapters and support TRAE CN 3.4.2 through the new-only public Agent ID `trae`; legacy bindings, paths, and recovery journals are not imported.
- Synchronize the Windows and macOS native host, account checks, model services, Agent selectors, IDE selection, and exact baseline restoration.
- Keep at most one initial TRAE restart; subsequent switches and restoration reuse the control session. Passive status refresh does not start a bridge.
- Scope Windows TRAE shutdown to the complete normalized installation path and preserve providers, credentials, other agents, external app data, and recovery backups.

### Fixed
- Synchronize Agent SDK and IDE global/current-session stores and wait for durable persistence, preventing stale renderer selections from overwriting the active model.
- Update the visible TRAE selection immediately after verification and ignore stale refresh responses.
- Restore every selector independently even when another restoration step fails; preserve exact field presence and user-owned models.
- Retain a valid control session after read-only planning failures, avoiding unnecessary first-switch restart prompts after transient catalogue errors.
- On macOS, notify and verify the mounted IDE and Agent model pickers after switching or restoring without opening settings or restarting.
- Restrict support to one IDE workspace and fail clearly when multiple independent workspaces are detected.

### Usage
- Install and sign in to TRAE CN 3.4.2, refresh Agent status, and use a Direct-compatible provider. The first switch may restart TRAE once; save work first. Proxy mode and multiple independent IDE workspaces are unsupported.
- Agent original configuration restores the selection captured before AT-Switch first took over. Old TraeCode / TraeWork clients and their saved state are outside this release's support range.

### Validation
- Windows 11 x64 / TRAE CN 3.4.2 real-UI selection, persistence, repeated switching, restoration, and unrelated-agent isolation are recorded in `.github/releases/v3.16.1-verification.md`.
- The user verified the official macOS TRAE build. The exact notarized Mac package is checked by the macOS release workflow before GitHub publication.
- Local mock acceptance does not establish real-provider inference, Streaming, or Tool Calling support. Actual API access depends on provider permissions and model capability.

## [v3.15.2] - 2026-10-07

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
