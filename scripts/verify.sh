#!/usr/bin/env bash
# Replays the GUI workflow scripts in scripts/verify through the release app.
# Each prints its `log` lines; screenshots and saved models land in target/verify.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target/verify
if [ $# -eq 0 ]; then set -- scripts/verify/*.txt; fi
for script in "$@"; do
    [ -f "$script" ] || script="scripts/verify/$script.txt"
    echo "== $script"
    rm -f target/verify/preferences.json
    RYSMO_PREFERENCES_PATH=target/verify/preferences.json RYSMO_SCREENSHOT=target/verify/unused.png \
        RYSMO_INPUT_SCRIPT="$script" ./target/release/rysmo 2>/dev/null | grep '^\[' || true
done
