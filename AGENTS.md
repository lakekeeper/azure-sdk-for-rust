# AGENTS.md

## Meta-rules for this file

- Keep this file concise. For each line, ask: would removing it cause mistakes? If not, cut it.
- Write commands and rules, not prose. Be imperative.

## Project

Lakekeeper's fork of the legacy, community-maintained Azure SDK for Rust (`azure_core`, `azure_identity`, `azure_storage`, `azure_storage_blobs`, `azure_storage_datalake`, all `0.21`).

- Upstream: the `legacy` branch of <https://github.com/Azure/azure-sdk-for-rust>. It gets no more fixes, so every fix lands here.
- Consumer: <https://github.com/lakekeeper/lakekeeper> pins this repository by `rev` in its workspace `Cargo.toml`. A change only reaches Lakekeeper when that `rev` is bumped.
- Fork branches start from the commit Lakekeeper pins. Keep each change a small, separate commit on top.

## Build & Test

Toolchain: `rust-toolchain.toml`. Mirror CI (`eng/scripts/code_style.sh`, `eng/scripts/sdk_tests.sh`) with `RUSTFLAGS=-Dwarnings`:

- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy --all --all-targets --all-features`
- Docs: `RUSTDOCFLAGS="-D warnings" cargo doc --all --no-deps`
- Build: `cargo check -p azure_core --no-default-features` and `cargo check --all --features azurite_workaround`
- Test: `cargo test --all --features hmac_rust`

Test a change against Lakekeeper too: in a Lakekeeper checkout, add a `[patch."https://github.com/lakekeeper/azure-sdk-for-rust.git"]` section that points each crate at this checkout by `path`. Keep this checkout outside the Lakekeeper directory: crates under `services/` are not workspace members, and Cargo then resolves their workspace from the wrong root.

## Code Style

- Follow existing patterns in adjacent files.
- Fix the code, don't silence the lint.
- Keep the public API of the crates stable. Lakekeeper uses it directly; a rename, removal or signature change needs a matching Lakekeeper change.
- Minimize new dependencies. Copying a small, permissively licensed type is fine: keep its license notice at the top of the file and name the source crate and version.
- Describe current behavior in comments: no "rather than", "instead of", "no longer", "previously".

## Rules

- Never skip or disable tests.
- Add a test for every fix that fails without it. Mock HTTP with an `azure_core::HttpClient` implementation; build environments with `Env::from(&[(key, value)][..])` and `TokenCredentialOptions::new` in tests.
- Assert exact expected values. Never accept one of several values.
- Keep commit messages short: describe what the commit changes, not why or how it was found.
