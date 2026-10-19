#!/usr/bin/env bash
# Local NativeActivity preview APK. No install, upload or release signing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
ABI="${1:-arm64-v8a}"
case "$ABI" in arm64-v8a|x86_64) ;; *) echo "Usage: $0 [arm64-v8a|x86_64]" >&2; exit 2;; esac
if [ "$(uname -s)" = Darwin ]; then DEFAULT_SDK="$HOME/Library/Android/sdk"; else DEFAULT_SDK="$HOME/Android/Sdk"; fi
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$DEFAULT_SDK}}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)}"
TOOLS="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
PLATFORM="$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)/android.jar"
if [ -n "${JAVA_HOME:-}" ]; then export PATH="$JAVA_HOME/bin:$PATH"; fi
if ! command -v java >/dev/null; then
    JDK="${JAVA_HOME:-$(ls -d "$HOME"/.local/opt/jdk-* 2>/dev/null | sort -V | tail -1)}"
    export PATH="$JDK/bin:$PATH"
fi
command -v java >/dev/null
command -v cargo-ndk >/dev/null
OUT="$ROOT/target/android"
STAGE="$OUT/stage-$ABI"
mkdir -p "$STAGE/lib" "$STAGE/assets/licenses"
cp LICENSE NOTICE.md docs/licenses/phosphor-icons-MIT.txt "$STAGE/assets/licenses/"
# NDK supplies the target C compiler, linker, sysroot and 16 KiB page alignment.
cargo ndk -t "$ABI" --platform 28 -o "$STAGE/lib" build --offline -p rysmo_mobile --profile portable
NDK_BIN="$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -mindepth 2 -maxdepth 2 -type d -name bin | head -1)"
"$NDK_BIN/llvm-strip" --strip-debug "$STAGE/lib/$ABI/librysmo_mobile.so"
"$TOOLS/aapt2" compile --dir packaging/android/res -o "$OUT/resources.zip"
"$TOOLS/aapt2" link -o "$OUT/unsigned-$ABI.apk" -I "$PLATFORM" --manifest packaging/android/AndroidManifest.xml \
    --min-sdk-version 28 --target-sdk-version 34 --version-code 1 --version-name 0.1.0 -A "$STAGE/assets" "$OUT/resources.zip"
(cd "$STAGE" && zip -q -r "$OUT/unsigned-$ABI.apk" lib)
"$TOOLS/zipalign" -f -P 16 4 "$OUT/unsigned-$ABI.apk" "$OUT/aligned-$ABI.apk"
if [ ! -f "$OUT/debug.keystore" ]; then
    keytool -genkeypair -keystore "$OUT/debug.keystore" -storepass android -keypass android -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Rysmo Debug,O=Rysmo,C=CZ"
fi
"$TOOLS/apksigner" sign --ks "$OUT/debug.keystore" --ks-pass pass:android --key-pass pass:android \
    --out "$OUT/rysmo-$ABI-debug.apk" "$OUT/aligned-$ABI.apk"
"$TOOLS/apksigner" verify "$OUT/rysmo-$ABI-debug.apk"
echo "Built $OUT/rysmo-$ABI-debug.apk (local preview, not a store release)"
