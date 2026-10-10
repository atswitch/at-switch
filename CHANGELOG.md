# Changelog

All notable changes to **AT-Switch** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Changed
- Write and verify TraeCode's Agent selectors and IDE's separate model/Manual-mode state in one controlled process, and restore each original selection independently. Keep TraeWork's selector unchanged.
- Preserve TraeCode IDE's original application/workspace model keys and modes in the encrypted recovery checkpoint; ignore stale workspace keys, and retry delayed model-service initialization inside the same process.
- Reuse the standard short Provider/model switch-success message for TraeCode and TraeWork instead of displaying native-service implementation details.
- Ignore legacy unscoped Trae model-cache rows when validating account-scoped selection keys; retry transient native selector-list refreshes.
- Replace visible Trae model-form automation with a version-gated bridge to TraeCode 3.4.1 and TraeWork 0.1.69 native model services. Unsupported versions fail closed.
- Refresh Trae's authoritative model list before restoring a selection, and wait for the signed-in account before writing its current-session and recent selections.
- Refresh the visible lite model catalogs for both TraeCode and TraeWork separately from their remote catalogs, preserve distinct task/default selections, and recheck the current task before committing.
- Normalize lite custom-model names to Trae's stable restart-time model keys, and reject cached selections that cannot resolve to that key instead of reporting a false success before a later Auto fallback.
- Keep the first restarted Trae instance running after a successful switch, instead of closing and relaunching it again; passive status checks remain read-only.
- Preserve exact same-ID model and endpoint conflict checks, encrypted recovery checkpoints, and user-owned models; no changes to other Agent adapters.

### Validation
- TraeCode 3.4.1 passed a macOS local-mock add/select/restore run covering Agent and IDE persistent defaults and exact IDE baseline recovery; no temporary model remained. The installed, signed 3.16.1 app then selected GLM-5.2 in both visible modes after one TraeCode process restart. A real Agent task returned a Trae-side connection timeout (HTTP 500), so real-provider requests and Windows device validation are not passed.
- On macOS, both agents passed native-service local-mock add/select in their visible lite selectors, original-selection restore, and managed-row cleanup. Windows device and new-path real-provider default-session/Streaming/Tool validation remain pending.

## [v3.16.1] - 2026-10-08

### Added
- TraeCode and TraeWork Direct-mode integration through their official custom-model UI for OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages, with independent discovery and bindings.
- Encrypted, account-scoped Trae recovery journals, idempotent managed-model reuse, original-selection restoration, and separate macOS Accessibility / Windows UI Automation implementations.
- Collapse Agent navigation beyond six visible entries into an accessible status-aware selector while keeping the active Agent visible.

### Fixed
- Treat an empty ima account metadata record as a signed-out state instead of an unsupported login format, preserving installation detection and showing an actionable login prompt without changing ima switching behavior.
- Restore the exact pre-operation Trae model and clean up newly created managed rows when a later binding commit fails.
- Wait for Trae's accessibility model selector to finish loading, and allow release builds to use a stable macOS signing identity so Accessibility permission survives application updates.
- Treat Trae's `Auto` and `Auto Mode` labels as the same original selection, dismiss transient promotion overlays, and navigate the collapsed TraeCode settings drawer without fixed screen coordinates.
- Remove the `AT-Switch ·` implementation prefix from new Trae model names, migrate exact legacy managed rows after a successful switch, recover missing ownership only from authenticated account-scoped history, and avoid treating Trae's lagging model cache as a failed selector update.
- Reuse an existing Trae custom model with the same real model ID by selecting it directly, while preserving it as user-owned during later restore and cleanup.
- Return automatically from TraeCode model management or the TraeWork add-model page before reading the active selector, so an already configured model can be switched without manual page cleanup.
- Recognize the main Trae model selector even when its current official model is absent from Trae's cached catalog, while keeping add-model form comboboxes excluded.
- Recover within the same switch when Trae has already persisted a newly added model but its official UI briefly loses the completion confirmation, avoiding a false failure followed by a required second click.
- Retry an exact Trae model-menu item with a verified center click when Electron reports a successful accessibility action without changing the active selector.

### Usage
- Keep TraeCode or TraeWork open, grant AT-Switch Accessibility permission on macOS when prompted, and choose a Direct-compatible provider. Trae requests go directly to the provider; Proxy mode is intentionally unavailable.

### Validation
- TraeCode and TraeWork passed macOS local-mock add/select, third-party-to-third-party switching, full app restart persistence, original-selection restore, exact managed-row cleanup, and interrupted-operation recovery. Windows device validation and real-provider default-session/tool-call validation remain pending; see `TRAE_INTEGRATION.md`.

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
- Frontend (107 tests), Windows Rust (189 tests), formatting, Clippy and Windows/macOS/Linux CI passed. The rebuilt Windows installer passes installation and startup checks; installed UI switching and the matching notarized Mac package remain pending. See the release verification record for evidence and remaining gates.

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
