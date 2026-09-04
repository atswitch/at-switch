# Release Checklist

Complete this checklist before publishing a GitHub Release from its draft.

## Signing configuration

Configure either every Secret in a platform group or none of them. A partial group
fails the workflow instead of silently creating a partially signed build.

- macOS: `APPLE_CERTIFICATE` is a base64-encoded `.p12` containing a valid
  **Developer ID Application** identity; `APPLE_CERTIFICATE_PASSWORD` unlocks that
  file; `KEYCHAIN_PASSWORD` is a temporary CI keychain password; `APPLE_ID`,
  `APPLE_PASSWORD` (an app-specific password), and `APPLE_TEAM_ID` are used for
  notarization.
- Windows: `WINDOWS_CERTIFICATE` is a base64-encoded, currently valid code-signing
  `.pfx`; `WINDOWS_CERTIFICATE_PASSWORD` unlocks it.

Do not put certificates or passwords in the repository. Store them as GitHub
Actions Secrets.

## Cut a release

1. Update the version in `package.json`, both root version fields in
   `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Move the relevant `CHANGELOG.md` entries from `Unreleased` into a dated version
   section and update its comparison links.
3. Run all quality checks below, then verify the intended tag locally:

   ```bash
   GITHUB_REF_NAME=vX.Y.Z node scripts/check-release-version.mjs
   ```

4. Commit and push the release changes. Create a `vX.Y.Z` tag, preferably signed,
   and push it to `origin`; the tag starts the release workflow.
5. Wait for both the macOS and Windows jobs. Review the resulting Draft Release.
   Universal macOS artifacts are built under
   `src-tauri/target/universal-apple-darwin/release/bundle/`; Windows artifacts use
   `src-tauri/target/release/bundle/`.
6. Download every installer together with its `.sha256` file and verify it. Also
   verify platform signatures:

   ```bash
   shasum -a 256 -c "AT-Switch.dmg.sha256"
   codesign --verify --deep --strict --verbose=2 "/Applications/AT-Switch.app"
   spctl --assess --type execute --verbose=2 "/Applications/AT-Switch.app"
   xcrun stapler validate "/Applications/AT-Switch.app"
   ```

   ```powershell
   $installer = "AT-Switch-installer.exe"
   $expected = (Get-Content "$installer.sha256").Split()[0].ToLowerInvariant()
   $actual = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant()
   if ($actual -ne $expected) { throw "Checksum mismatch" }
   if ((Get-AuthenticodeSignature $installer).Status -ne "Valid") {
     throw "Invalid Authenticode signature"
   }
   ```

7. Complete clean-machine smoke tests and the checklist below. Only then use
   GitHub's **Publish release** action.

## Publication gates

- [ ] `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and
      `src-tauri/tauri.conf.json` match the `v*` release tag.
- [ ] Frontend build and tests pass.
- [ ] `cargo fmt`, Clippy with warnings denied, and Rust tests pass.
- [ ] macOS Apple Silicon, macOS Intel, Windows 10, and Windows 11 smoke tests pass.
- [ ] Direct mode, local proxy mode, configuration rollback, and original
      configuration restoration are verified.
- [ ] macOS artifacts have a valid Developer ID signature and successful
      notarization; ad-hoc-signed drafts must not be published.
- [ ] Windows installers have a valid code signature; unsigned drafts must not be
      published.
- [ ] Every installer has an accompanying `.sha256` file and its checksum verifies.
- [ ] Release notes describe features, fixes, compatibility, known limitations,
      and upgrade impact.
- [ ] Resolved JavaScript and Rust dependency licenses have been reviewed, and all
      notices or license texts required for binary distribution are included.
- [ ] The provenance and redistribution evidence for every bundled third-party
      identification asset is still valid.
- [ ] A clean machine can download, install, launch, and remove each public package.

If signing Secrets are absent, the workflow still builds a Draft Release for
internal verification but never publishes it automatically. Do not manually publish
an unsigned or ad-hoc-signed draft.
