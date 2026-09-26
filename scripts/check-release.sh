#!/usr/bin/env bash
# Repository checks only. Never publishes, tags, pushes or removes build output.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

cargo fmt --all -- --check
python3 scripts/check-docs.py --examples
cargo test --locked --no-default-features
cargo clippy --locked --no-default-features --all-targets -- -D warnings
cargo check --locked --no-default-features --features fonts
cargo check --locked --no-default-features --features headless-text
cargo check --locked --no-default-features --features desktop,headless-text
cargo test --locked --all-features
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --manifest-path tests/consumers/budget_api/Cargo.toml --target-dir target
RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D warnings" cargo doc --locked --all-features --no-deps
cargo package --locked --all-features --allow-dirty
python3 scripts/check-package.py
git diff --check

printf '%s\n' 'Local release checks passed. MSRV, native acceptance and clean-tree publish dry-run remain separate.'
