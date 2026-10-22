#!/usr/bin/env bash
# The Rust checks used locally and in GitHub Actions. GUI code is type-checked;
# the headless libraries run tests without a GPU or desktop session.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ $# -gt 1 || ( $# -eq 1 && "$1" != --offline ) ]]; then
    echo "Usage: scripts/check-rust.sh [--offline]" >&2
    exit 2
fi
cargo_options=(--locked)
if [[ $# -eq 1 ]]; then cargo_options+=(--offline); fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo fmt --all --check
cargo clippy --workspace --all-targets "${cargo_options[@]}" -- -D warnings
cargo test --lib -p rysmo_math -p rysmo_mesh -p rysmo_model -p rysmo_infer \
    -p rysmo_tools -p rysmo_io -p rysmo_mcp -p rysmo_library "${cargo_options[@]}"
# Exercise browser recovery scheduling without linking Bevy's GPU test binary.
mkdir -p target/checks
rustc --edition=2024 --test -D warnings crates/rysmo_app/src/autosave.rs -o target/checks/autosave-tests
target/checks/autosave-tests
cargo check -p rysmo_app --target wasm32-unknown-unknown --profile web-release "${cargo_options[@]}"
