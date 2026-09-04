# Contributing to AT-Switch

[简体中文](./CONTRIBUTING.md) | English

Contributions are welcome, including bug reports, documentation improvements,
security reports, and pull requests.

## Development requirements

- Node.js 20 or later
- npm 10 or later
- Rust stable 1.85 or later
- macOS 12 or later, or Windows 10/11
- The [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/)

## Local development

1. Fork and clone the repository:

   ```bash
   git clone https://github.com/<your-username>/at-switch.git
   cd at-switch
   ```

2. Install dependencies with `npm ci`.
3. Run `npm run dev` for the browser mock or `npm run tauri:dev` for the native
   desktop application.

## Quality gates

Run every check before opening a pull request:

```bash
npm run build
npm test -- --run
npm run licenses:check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Tests use isolated temporary directories, test databases, and in-memory
credentials. They do not read real API keys or modify real agent configuration.

## Pull requests

1. Branch from the latest `main` using a focused name such as
   `feat/<feature-name>` or `fix/<issue-number>`.
2. Keep each pull request focused on one responsibility.
3. Account for both macOS and Windows when changing agent discovery or
   configuration logic.
4. Add tests for new behavior and bug fixes.
5. Explain the motivation, implementation, and verification steps clearly.
6. Add user-visible changes to the `Unreleased` section of
   [CHANGELOG.md](./CHANGELOG.md).

## Releases and third-party assets

Semantic `v*` tags must match all four version manifests. GitHub Actions creates
only a Draft Release; maintainers must complete signing, checksum verification, and
clean-machine smoke tests before publication. Follow the
[release checklist](./.github/RELEASE_CHECKLIST.md).

When adding or replacing third-party names, icons, or trademarks, record their
source, confirm redistribution rights, and update
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).
