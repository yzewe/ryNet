#!/usr/bin/env bash
set -euo pipefail
OUT=${1:?jniLibs dir}
ABI=${2:-arm64-v8a}
if [[ $ABI == *,* ]]; then
  for a in ${ABI//,/ }; do bash "$0" "$OUT" "$a"; done
  exit
fi
case "$ABI" in
  arm64-v8a) TARGET=aarch64-linux-android; CLANG=aarch64-linux-android ;;
  armeabi-v7a) TARGET=armv7-linux-androideabi; CLANG=armv7a-linux-androideabi ;;
  x86_64) TARGET=x86_64-linux-android; CLANG=x86_64-linux-android ;;
  *) echo "unknown ABI $ABI" >&2; exit 1 ;;
esac
T=$(echo "$TARGET" | tr 'a-z-' 'A-Z_')
t=$(echo "$TARGET" | tr '-' '_')
NDK=${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/29.0.14206865}
API=${API:-26}
TC=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
export PATH=$HOME/.cargo/bin:$PATH
export CC_$t=$TC/$CLANG$API-clang
export AR_$t=$TC/llvm-ar
export CARGO_TARGET_${T}_LINKER=$TC/$CLANG$API-clang
FLAGS="-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,--icf=all -C link-arg=-Wl,--pack-dyn-relocs=android"
cd "$(dirname "$0")"
NIGHTLY=${RYNET_NIGHTLY:-nightly}
if rustup run "$NIGHTLY" rustc --print sysroot >/dev/null 2>&1 \
    && [ -d "$(rustup run "$NIGHTLY" rustc --print sysroot)/lib/rustlib/src/rust/library" ]; then
  export CARGO_TARGET_${T}_RUSTFLAGS="$FLAGS -Zlocation-detail=none -C force-unwind-tables=no -Zunstable-options -Cpanic=immediate-abort"
  cargo +"$NIGHTLY" build --release -p rv-android --target "$TARGET" -Zbuild-std=std,panic_abort
else
  export CARGO_TARGET_${T}_RUSTFLAGS="$FLAGS"
  cargo build --release -p rv-android --target "$TARGET"
fi
mkdir -p "$OUT/$ABI"
rm -f "$OUT/$ABI"/*.so
cp "target/$TARGET/release/librynet.so" "$OUT/$ABI/librynet.so"
