# Changelog

All notable changes to **AT-Switch** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Changed
- Replace the retired TraeCode/TraeWork entries with one new-only TRAE adapter and public ID `trae`; support TRAE CN 3.4.2 only, without importing legacy bindings, paths or recovery journals.
- Align Windows and macOS on the same version-gated native host, account checks, model services, three Agent selectors, independent IDE selection, and exact encrypted baseline restore. Remove platform-specific model DI and visible UI fallbacks.
- Keep at most one initial target restart, retaining the native control session for subsequent switches/restores without another restart; passive scans never start the bridge. Delayed control-pipe initialization retries within the same process.
- Scope Windows TRAE shutdown to the complete normalized installation path, including its pipe owner; never terminate another installation by image name. Other Agent adapters are unchanged.
- Retire only old TRAE metadata inside AT-Switch; preserve providers, credentials, other agents, external application data and recovery backups.

### Validation
- Windows TRAE 3.4.2 running-state local-mock roundtrip and consecutive model changes pass with one initial exit/start, no warm-switch/restore restarts, and unchanged other-agent main processes. Final cold-state and build results are recorded in the release verification document.
- macOS TRAE CN 3.4.2 signed-build acceptance covers one initial restart, Agent and IDE visible selection, IDE/Agent warm model changes without another restart, and original-selection restoration. Windows validation remains the separate recorded run.
- Local-mock integration acceptance does not imply real-provider inference, Streaming or Tool Calling acceptance.

### Fixed
- Update the initialized Agent SDK and IDE current-session/global stores together; prevent stale renderer selection maps from restoring the previous model and wait for durable storage flush.
- Display verified TRAE switch results immediately and ignore older refresh responses. Cache the shared selector catalogue within each operation and remove unconditional settling delays.
- Suppress repeated restart prompts while the same-installation/account native session is reusable; passive refresh does not recreate the initial warning, and context changes invalidate reuse.
- Run every selector rollback even if another restore fails, preserving exact field presence and user models. Reject unsupported multiple IDE workspaces rather than reporting partial success.
- Use an exact-path native Windows TRAE process scan and deduplicated graceful-close/force fallback, without changing other agents' lifecycle behavior.
- Protect all current-session selection labels from stale whole-map writeback, including existing Agent conversations; restore absent `solo_agent_lite` overrides by explicit label without losing unrelated selections.
- Flush IDE defaults even when only the Agent window is open. Restore selectors before deleting temporary models, and retain the same TRAE process after a fully verified rollback.
- Retain an acknowledged native control session after read-only planning failures, so transient catalogue failures do not force another initial restart. Record credential-free operation timings and expose a predefined failure-stage hint.
- On macOS, notify the mounted IDE workbench model service after a native selection change, so its visible picker updates without opening settings or restarting; reject a missing versioned service or a stale visible label.
- On macOS, wait for Trae's persisted model-list cache to drop a deleted managed model before reporting restoration complete.
- On macOS, load Trae's native model catalogue before restoring saved selector keys, and reattach a replaced renderer inside the same running process; leave the verified Windows bridge order unchanged.
- On macOS, notify and verify the mounted Agent model picker after a native model switch, so a stale visible selection cannot be reported as success; Windows and other Agent adapters are unchanged.

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
