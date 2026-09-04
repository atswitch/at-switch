# Changelog

All notable changes to AT-Switch are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Removed the built-in preferred-provider designation so all providers are shown
  without commercial preference.
- Hardened CI and draft release automation with version checks, checksums, signing
  safeguards, automated dependency update monitoring, and scheduled vulnerability
  audits.
- Clarified direct-mode credential storage and third-party asset ownership.

## [0.1.7] - 2026-09-04

### Added

- Provider and model management for WorkBuddy, CodeBuddy CN, QClaw, AutoClaw,
  Codex, ima, and TRAE.
- Direct connections and an optional local protocol-converting proxy.
- OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages support,
  including streaming and tool calls.
- Encrypted configuration backups, atomic writes, post-write validation, rollback,
  and original-configuration restoration.
- macOS and Windows agent discovery and lifecycle handling.
- Simplified Chinese and English user interfaces.
- Cross-platform CI and draft GitHub Release automation.

[Unreleased]: https://github.com/atswitch/at-switch/compare/v0.1.7...HEAD
[0.1.7]: https://github.com/atswitch/at-switch/releases/tag/v0.1.7
