#!/usr/bin/env bash
set -euo pipefail
root=${RYNET_ROOTFS:-/opt/rynet-env/jammy}
env=${RYNET_ENV:-$HOME/env/linux}
qt_ver=6.11.2
cmake_ver=3.31.12
qrencode_ver=4.1.1
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)

qt_mirrors=${RYNET_QT_MIRRORS:-https://mirrors.ocf.berkeley.edu/qt https://mirror.yandex.ru/mirrors/qt.io https://download.qt.io}
fetch() { # url sha256 file; url вида qt:/путь — с зеркал Qt
    [ -f "$3" ] && echo "$2  $3" | sha256sum -c --quiet - 2>/dev/null && return
    local urls=("$1") u
    [[ $1 == qt:* ]] && { urls=(); for u in $qt_mirrors; do urls+=("$u${1#qt:}"); done; }
    for u in "${urls[@]}"; do
        curl -sSfL --connect-timeout 15 --retry 2 -o "$3.part" "$u" && echo "$2  $3.part" | sha256sum -c --quiet - && { mv "$3.part" "$3"; return; }
    done
    echo "не скачалось или контрольная сумма не совпала: $1" >&2; exit 1
}

if [ "${1:-}" = root ]; then
    [ "$(id -u)" = 0 ] || { echo "нужен root" >&2; exit 1; }
    if [ ! -x "$root/usr/bin/g++-12" ]; then
        mkdir -p "$root"
        fetch https://cdimage.ubuntu.com/ubuntu-base/releases/22.04/release/ubuntu-base-22.04.5-base-amd64.tar.gz \
            242cd8898b33ea806ef5f13b1076ed7c76f9f989d18384452f7166692438ff1a /root/ubuntu-base-22.04.5.tar.gz
        tar -xzf /root/ubuntu-base-22.04.5.tar.gz -C "$root"
        bwrap --bind "$root" / --dev /dev --proc /proc --perms 01777 --tmpfs /tmp --ro-bind /etc/resolv.conf /etc/resolv.conf \
            env DEBIAN_FRONTEND=noninteractive PATH=/usr/sbin:/usr/bin:/sbin:/bin sh -ec '
            apt-get update -qq
            apt-get install -y -qq --no-install-recommends ca-certificates curl xz-utils file make perl python3 pkg-config \
                gcc-12 g++-12 ninja-build ccache \
                libgl-dev libegl-dev libfontconfig1-dev libfreetype-dev zlib1g-dev libssl-dev libdbus-1-dev \
                libx11-dev libx11-xcb-dev libxext-dev libxrender-dev libxi-dev libsm-dev libice-dev \
                libxcb1-dev libxcb-cursor-dev libxcb-glx0-dev libxcb-icccm4-dev libxcb-image0-dev libxcb-keysyms1-dev \
                libxcb-randr0-dev libxcb-render0-dev libxcb-render-util0-dev libxcb-shape0-dev libxcb-shm0-dev \
                libxcb-sync-dev libxcb-util-dev libxcb-xfixes0-dev libxcb-xinerama0-dev libxcb-xkb-dev libxcb-xinput-dev \
                libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libwayland-bin wayland-protocols >/dev/null
            apt-get clean'
    fi
    mkdir -p "$root/opt/env" "$root/src" "$root/build" "$root/ccache"
    echo "корень сборки: $root"
    exit 0
fi

[ -d "$root/ccache" ] || { echo "нет корня сборки $root: один раз от root — $0 root" >&2; exit 1; }
mkdir -p "$env" "$HOME/build" "${CCACHE_DIR:-$HOME/.cache/ccache}"
run() {
    bwrap --ro-bind "$root" / --dev /dev --proc /proc --perms 01777 --tmpfs /tmp --ro-bind /etc/resolv.conf /etc/resolv.conf \
        --bind "$env" /opt/env --bind "$repo" /src --bind "$HOME/build" /build \
        --bind "${CCACHE_DIR:-$HOME/.cache/ccache}" /ccache --setenv CCACHE_DIR /ccache --setenv HOME /build \
        --setenv PATH /opt/env/cmake/bin:/usr/bin:/bin --setenv CC gcc-12 --setenv CXX g++-12 \
        --unshare-pid --die-with-parent --chdir /build "$@"
}
if [ "${1:-}" = run ]; then shift; run "$@"; exit; fi

jobs=${RYNET_QT_JOBS:-$(nproc)}
dl=$env/dl
mkdir -p "$dl"
if [ ! -x "$env/cmake/bin/cmake" ]; then
    fetch "https://cmake.org/files/v${cmake_ver%.*}/cmake-$cmake_ver-linux-x86_64.tar.gz" \
        0dc2e9a6860f06bf10bd8fadc03e35d9eeb4df46e33763a7e480e987758f385c "$dl/cmake.tar.gz"
    rm -rf "$env/cmake" && mkdir -p "$env/cmake" && tar -xzf "$dl/cmake.tar.gz" -C "$env/cmake" --strip-components=1
fi

if [ ! -f "$env/deps/lib/libqrencode.a" ]; then
    fetch "https://github.com/fukuchi/libqrencode/archive/refs/tags/v$qrencode_ver.tar.gz" \
        5385bc1b8c2f20f3b91d258bf8ccc8cf62023935df2d2676b5b67049f31a049c "$dl/qrencode.tar.gz"
    rm -rf "$env/src/qrencode" && mkdir -p "$env/src/qrencode" && tar -xzf "$dl/qrencode.tar.gz" -C "$env/src/qrencode" --strip-components=1
    run sh -ec 'cmake -S /opt/env/src/qrencode -B /opt/env/src/qrencode/b -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_INSTALL_PREFIX=/opt/env/deps -DBUILD_SHARED_LIBS=OFF \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DWITH_TOOLS=NO -DWITH_TESTS=NO >/dev/null
        cmake --build /opt/env/src/qrencode/b >/dev/null && cmake --install /opt/env/src/qrencode/b >/dev/null'
    rm -rf "$env/src/qrencode"
fi

qt_base_opts="-prefix /opt/env/qt -release -shared -nomake examples -nomake tests -no-pch
    -no-icu -no-glib -no-widgets -no-cups -no-feature-sql -no-feature-testlib -no-feature-printsupport
    -no-feature-concurrent -no-feature-vulkan -no-feature-sessionmanager
    -system-zlib -qt-pcre -qt-harfbuzz -qt-libpng -no-libjpeg -system-freetype -fontconfig -no-zstd
    -openssl-runtime -dbus-runtime -no-feature-brotli -no-feature-gssapi -no-libproxy -no-feature-sctp
    -xcb -bundled-xcb-xinput -no-eglfs -no-linuxfb -no-kms -no-gbm -no-directfb -no-libinput -no-evdev -no-tslib -no-mtdev
    -no-libudev"
qt_decl_opts="-DFEATURE_qml_debug=OFF -DFEATURE_qml_profiler=OFF -DFEATURE_qml_preview=OFF -DFEATURE_quick_designer=OFF"
qt_way_opts="-DFEATURE_wayland_server=OFF"
stamp="$qt_ver|$qt_base_opts|$qt_decl_opts|$qt_way_opts"
if [ "$(cat "$env/qt/.rynet-build" 2>/dev/null)" != "$stamp" ]; then
    declare -A sums=(
        [qtbase]=5b2e00eccaf5a4d8c14134ffa0ea8dfd0a35ae1ffc7f8d87fa4305a1ed23cf22
        [qtshadertools]=805046b8b7757665586890b375940047e874ae3ab00adb6d3f2b38fc6b200b1c
        [qtdeclarative]=215b7b70517e380123eabc6b92243f3c47b6f016a91d126057dbe53551c6b430
        [qtwayland]=8eb7615e39332a10f506e8dd70f02d5954bb5949ff54f6dcbf8bd6168222f9df
    )
    rm -rf "$env/qt" "$env/src/qt"
    for m in qtbase qtshadertools qtdeclarative qtwayland; do
        fetch "qt:/official_releases/qt/${qt_ver%.*}/$qt_ver/submodules/$m-everywhere-src-$qt_ver.tar.xz" \
            "${sums[$m]}" "$dl/$m-$qt_ver.tar.xz"
        mkdir -p "$env/src/qt/$m" && tar -xJf "$dl/$m-$qt_ver.tar.xz" -C "$env/src/qt/$m" --strip-components=1
    done
    run sh -ec "
        mkdir -p /opt/env/src/qt/b-qtbase && cd /opt/env/src/qt/b-qtbase
        /opt/env/src/qt/qtbase/configure $(echo $qt_base_opts) -- -DCMAKE_C_COMPILER=gcc-12 -DCMAKE_CXX_COMPILER=g++-12 >config.log 2>&1 || { tail -40 config.log; exit 1; }
        cmake --build . --parallel $jobs >build.log 2>&1 || { grep -m20 -B2 -A8 'error' build.log; exit 1; }
        cmake --install . >/dev/null
        for m in qtshadertools qtdeclarative qtwayland; do
            mkdir -p /opt/env/src/qt/b-\$m && cd /opt/env/src/qt/b-\$m
            opts=; [ \$m = qtdeclarative ] && opts='$qt_decl_opts'; [ \$m = qtwayland ] && opts='$qt_way_opts'
            /opt/env/qt/bin/qt-configure-module /opt/env/src/qt/\$m -- \$opts >config.log 2>&1 || { tail -40 config.log; exit 1; }
            cmake --build . --parallel $jobs >build.log 2>&1 || { grep -m20 -B2 -A8 'error' build.log; exit 1; }
            cmake --install . >/dev/null
        done"
    rm -rf "$env/src/qt"
    echo "$stamp" > "$env/qt/.rynet-build"
fi
echo "окружение: $env (Qt $qt_ver)"
