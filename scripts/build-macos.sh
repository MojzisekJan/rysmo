#!/usr/bin/env bash
# Native Mac build and unsigned local .app bundle; no notarization or upload.
set -euo pipefail
[ "$(uname -s)" = Darwin ] || { echo "Run this script on macOS with Xcode Command Line Tools." >&2; exit 2; }
cd "$(dirname "$0")/.."
case "$(uname -m)" in arm64) TARGET=aarch64-apple-darwin;; x86_64) TARGET=x86_64-apple-darwin;; *) exit 2;; esac
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo build --locked -p rysmo_app --bin rysmo --target "$TARGET" --profile portable
APP=target/packages/macos/Rysmo.app
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" target/packages/macos/Rysmo.iconset
cp "target/$TARGET/portable/rysmo" "$APP/Contents/MacOS/"
cp LICENSE NOTICE.md docs/licenses/phosphor-icons-MIT.txt "$APP/Contents/Resources/"
cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" crates/rysmo_app/brand/icon-512.png --out "target/packages/macos/Rysmo.iconset/icon_${size}x${size}.png" >/dev/null
    doubled=$((size * 2))
    sips -z "$doubled" "$doubled" crates/rysmo_app/brand/icon-512.png --out "target/packages/macos/Rysmo.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns target/packages/macos/Rysmo.iconset -o "$APP/Contents/Resources/Rysmo.icns"
echo "Built $APP (unsigned local test bundle)"
