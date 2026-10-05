# GTK macro dependency backports

Tauri 2.12.1 still selects the GTK 0.18 bindings for Linux/BSD. Their two macro
crates depend on the unmaintained `proc-macro-error` 1.x
([RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370)).
These local patches remove that dependency by backporting upstream `syn::Error`
diagnostics. They are build-time dependencies for GTK; the Windows and macOS
application dependency graphs do not compile them. No advisory is ignored.

## Provenance and scope

| Crate base | Registry archive SHA-256 | Upstream diagnostics change |
| --- | --- | --- |
| `glib-macros` 0.18.5 | `0bb0228f477c0900c880fd78c8759b95c7636dbd7842707f49e132378aa2acdc` | [c74a40a16b23a1b9c8e67842d32d02bc187f551d](https://github.com/gtk-rs/gtk-rs-core/commit/c74a40a16b23a1b9c8e67842d32d02bc187f551d) |
| `gtk3-macros` 0.18.2 | `52ff3c5b21f14f0736fed6dcfc0bfb4225ebf5725f3c0209edeec181e4d73e9d` | [45782251fa35ecf8bfe759ffceb3db32659a0c97](https://github.com/gtk-rs/gtk3-rs/commit/45782251fa35ecf8bfe759ffceb3db32659a0c97) |

The registry sources, original `LICENSE`/`COPYRIGHT`, and available upstream
tests are retained. Registry cache metadata is excluded. `Cargo.toml` removes
`proc-macro-error`; native GTK/GLib dev dependencies are limited to non-Windows
hosts so the pure diagnostic tests can also run on Windows.

The GLib backport retains the 0.18 macro API and generated successful code. It
does not bring in the newer dynamic-type registration APIs. Error propagation
in the older flags implementation is adapted directly to `syn::Error`.
Validation of data-bearing enum variants runs before tag generation, preventing
a pre-existing panic for invalid enum/flags attributes when a unit variant comes
first. Diagnostics may have more precise spans than the old abort macros.
The GTK backport only adjusts imports and error handling for the 0.18 source.

`diagnostics_tests.rs` exercises rejected input and valid generated Rust items.
CI also runs the retained GLib integration tests with native GTK libraries on
Ubuntu. The independent vendor workspace keeps its own lockfile; the app uses
`src-tauri/Cargo.lock` and explicit `[patch.crates-io]` entries.

```sh
# Portable diagnostic regression tests
cargo test --manifest-path vendor/Cargo.toml --workspace --lib --locked

# Linux, with libgtk-3-dev installed: upstream and diagnostic tests
cargo test --manifest-path vendor/Cargo.toml --workspace --lib --tests --locked
```

Remove both patches, vendored crates and their dedicated tests once every Tauri
GTK dependency resolves to upstream versions without `proc-macro-error`.
Re-run the whole-lock advisory audit and platform regression tests at that time.
