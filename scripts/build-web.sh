#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" != "--package-only" ]]; then
    # Avoid overlapping Bevy app-library code generation with the memory-heavy WASM link.
    # Larger build hosts can opt back into parallel Cargo work explicitly.
    export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
    # Trunk's offline mode searches PATH before consulting its own download cache.
    bindgen_version="$(python3 -c 'import sys,tomllib; print(next(p["version"] for p in tomllib.load(open(sys.argv[1],"rb"))["package"] if p["name"]=="wasm-bindgen"))' "$project_root/Cargo.lock")"
    trunk_cache="${RYSMO_TRUNK_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/trunk}"
    cached_bindgen="$trunk_cache/wasm-bindgen-$bindgen_version"
    if [[ -x "$cached_bindgen/wasm-bindgen" ]]; then export PATH="$cached_bindgen:$PATH"; fi
    for cached_optimizer in "$trunk_cache"/wasm-opt-*/bin/wasm-opt; do
        if [[ -x "$cached_optimizer" ]]; then export PATH="${cached_optimizer%/*}:$PATH"; break; fi
    done
    cd "$project_root/crates/rysmo_app"
    env -u NO_COLOR trunk build --release true --offline "${RYSMO_WEB_OFFLINE:-false}" --public-url ./
fi
python3 "$project_root/scripts/package-web.py"
