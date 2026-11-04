#!/usr/bin/env bash
# Native line/function coverage. Stable Rust does not provide branch coverage here.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
scope=all
cargo_options=(--locked)
for arg in "$@"; do
    case "$arg" in
        --core) scope=core ;;
        --offline) cargo_options+=(--offline) ;;
        *) echo "Usage: scripts/coverage.sh [--core] [--offline]" >&2; exit 2 ;;
    esac
done
cargo llvm-cov --version >/dev/null
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
# Remove old counters without discarding instrumented dependency builds.
cargo llvm-cov clean --profraw-only "${cargo_options[@]}"
cargo llvm-cov --no-report --profile coverage --lib \
    -p rysmo_math -p rysmo_mesh -p rysmo_model -p rysmo_infer \
    -p rysmo_tools -p rysmo_io -p rysmo_mcp -p rysmo_library "${cargo_options[@]}"
cargo llvm-cov --no-report --profile coverage -p rysmo_mcp \
    --test stdio --test live "${cargo_options[@]}"
if [[ "$scope" == all ]]; then
    cargo llvm-cov --no-report --profile coverage --lib -p rysmo_app "${cargo_options[@]}"
fi
output="target/coverage/$scope"
mkdir -p "$output"
ignore='(/vendor/|/tests/|/examples/|/tests[^/]*\.rs$|/[^/]*_tests\.rs$)'
# A core report excludes app objects left by an earlier full build.
if [[ "$scope" == core ]]; then ignore="$ignore|/crates/(rysmo_app|rysmo_mobile)/"; fi
report_options=(--profile coverage --ignore-filename-regex "$ignore")
cargo llvm-cov report "${report_options[@]}" --html --output-dir "$output"
cargo llvm-cov report "${report_options[@]}" --json --summary-only --output-path "$output/summary.json"
cargo llvm-cov report "${report_options[@]}" --lcov --output-path "$output/lcov.info"
printf 'Coverage scope: %s\nHTML: %s/html/index.html\nJSON: %s/summary.json\n' "$scope" "$output" "$output"
