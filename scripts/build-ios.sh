#!/usr/bin/env bash
# Prepare the Rust static library on a Mac. Xcode device signing/host testing follows.
set -euo pipefail
[ "$(uname -s)" = Darwin ] || { echo "iOS builds need macOS and the Xcode iOS SDK." >&2; exit 2; }
cd "$(dirname "$0")/.."
if [ "$(uname -m)" = arm64 ]; then DEFAULT_TARGET=aarch64-apple-ios-sim; else DEFAULT_TARGET=x86_64-apple-ios; fi
TARGET="${1:-$DEFAULT_TARGET}"
case "$TARGET" in aarch64-apple-ios|aarch64-apple-ios-sim|x86_64-apple-ios) ;; *) echo "Unsupported iOS target" >&2; exit 2;; esac
export IPHONEOS_DEPLOYMENT_TARGET=15.0
rustup target add "$TARGET"
cargo build --locked -p rysmo_mobile --target "$TARGET" --profile portable
cargo rustc --locked -p rysmo_mobile --target "$TARGET" --profile portable -- --print native-static-libs
printf 'Static library: target/%s/portable/librysmo_mobile.a\nHost main/Info.plist/icons: packaging/ios\n' "$TARGET"
