set shell := ["bash", "-c"]
set export

RUST_LOG := "debug"

check-format:
	cargo +nightly fmt --all -- --check

check-clippy:
    cargo clippy --no-default-features --all-targets --workspace -- -D warnings
    cargo clippy --all-targets --all-features --workspace -- -D warnings
    cargo clippy -p lakekeeper --no-default-features -- -D warnings
    cargo clippy -p lakekeeper --no-default-features --features "test-utils" -- -D warnings
    cargo clippy -p lakekeeper-io --no-default-features --features "storage-in-memory" -- -D warnings
    cargo clippy -p lakekeeper-io --all-features -- -D warnings
    cargo clippy -p lakekeeper --no-default-features --features "sqlx-postgres,s3-signer,router" -- -D warnings
    cargo clippy -p lakekeeper-bin --all-features -- -D warnings
    cargo clippy -p lakekeeper-bin --no-default-features -- -D warnings

check-cargo-sort:
	cargo sort -c -w

check: check-clippy check-format check-cargo-sort

fix-format:
    cargo +nightly fmt --all
    cargo sort -w

fix:
    cargo clippy --all-targets --all-features --workspace --fix --allow-staged
    cargo +nightly fmt --all
    cargo sort -w

doc-test:
	cargo test --no-fail-fast --doc --all-features --workspace

unit-test: doc-test
	cargo test --profile ci --lib --all-features --workspace

test: doc-test
	cargo test --all-targets --all-features --workspace
