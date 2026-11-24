#!/usr/bin/env bash
set -euo pipefail
umask 022
here=$(cd "$(dirname "$0")" && pwd)
qtdir=$(cd "$here/../.." && pwd)
repo=$(cd "$qtdir/.." && pwd)
out=$(mkdir -p "$1" && readlink -f "$1")
ver=$(tr -d ' \r\n' < "$qtdir/version.txt")
icons=$qtdir/assets/icons
tools=${XDG_CACHE_HOME:-$HOME/.cache}/rynet-build/tools
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$tools"
log() { echo "[$(date +%H:%M:%S)] $*"; }

fetch() {
    local name=$1 url=$2 sum=$3 inner=${4:-}
    [ -x "$tools/$name" ] && return
    local tmp="$work/dl"
    curl -sSfL -o "$tmp" "$url"
    echo "$sum  $tmp" | sha256sum -c --quiet - || { echo "$name: контрольная сумма не совпала ($url)" >&2; exit 1; }
    if [ -n "$inner" ]; then tar -xzf "$tmp" -C "$tools" "$inner"; else install -m 755 "$tmp" "$tools/$name"; fi
}
fetch nfpm https://github.com/goreleaser/nfpm/releases/download/v2.47.0/nfpm_2.47.0_Linux_x86_64.tar.gz \
    0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783 nfpm
fetch appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
    a6d71e2b6cd66f8e8d16c37ad164658985e0cf5fcaa950c90a482890cb9d13e0

"$here/buildenv.sh" >/dev/null
log "core"
for z in /opt/zig "$HOME"/.local/opt/zig-*; do [ -d "$z" ] && PATH="$z:$PATH"; done
(cd "$repo/core" && cargo zigbuild --release -q -p rv-desktop --target x86_64-unknown-linux-gnu.2.28)
log "app"
"$here/buildenv.sh" run bash /src/desktop-qt/packaging/linux/deploy.sh
app=$HOME/build/desktop-qt-stage/rynet
install -m 755 "$repo/core/target/x86_64-unknown-linux-gnu/release/rynet-core" "$app/bin/rynet-core"
install -m 755 "$repo/core/target/x86_64-unknown-linux-gnu/release/rynet-cli" "$app/bin/rynet-cli"
for f in "$app"/lib/*; do case $(basename "$f") in libQt6*) ;; *) patchelf --set-rpath '$ORIGIN' "$f" ;; esac; done
chmod -R go-w "$app"

log "packages"
{
    cat <<EOF
name: rynet
arch: amd64
platform: linux
version: $ver
release: 1
section: net
priority: optional
maintainer: yzewe <noreply@github.com>
vendor: yzewe
homepage: https://github.com/yzewe/ryNet
license: GPL-3.0-or-later
description: |
  VPN client: VLESS (REALITY, Vision, XHTTP), Trojan, Hysteria 2;
  full-tunnel (TUN) mode, routing rules, subscriptions.
# Qt лежит в пакете; из системы — графика, шрифты, раскладки, D-Bus и setcap.
overrides:
  deb:
    depends: [libcap2-bin, libdbus-1-3, libegl1, libgl1, libfontconfig1, libfreetype6, libxkbcommon0]
  rpm:
    depends: [libcap, "libdbus-1.so.3()(64bit)", "libEGL.so.1()(64bit)", "libGL.so.1()(64bit)", "libfontconfig.so.1()(64bit)", "libfreetype.so.6()(64bit)", "libxkbcommon.so.0()(64bit)"]
  archlinux:
    depends: [libcap, dbus, libglvnd, fontconfig, freetype2, libxkbcommon]
# Arch при обновлении зовёт post_upgrade: новому ядру — те же права.
archlinux:
  scripts:
    postupgrade: $here/postinstall.sh
contents:
  - src: $app/
    dst: /opt/rynet
    type: tree
  - src: /opt/rynet/bin/rynet
    dst: /usr/bin/rynet
    type: symlink
  - src: /opt/rynet/bin/rynet-cli
    dst: /usr/bin/rynet-cli
    type: symlink
  - src: $here/rynet.desktop
    dst: /usr/share/applications/rynet.desktop
    file_info:
      mode: 0644
  - src: $here/rynet-plugin.xml
    dst: /usr/share/mime/packages/rynet-plugin.xml
    file_info:
      mode: 0644
  - src: $here/polkit/49-rynet-resolved.rules
    dst: /opt/rynet/share/polkit/49-rynet-resolved.rules
    file_info:
      mode: 0644
  - src: $here/polkit/49-rynet-resolved.pkla
    dst: /opt/rynet/share/polkit/49-rynet-resolved.pkla
    file_info:
      mode: 0644
EOF
    for s in 16 24 32 48 64 128 256; do
        printf '  - src: %s/app-%s.png\n    dst: /usr/share/icons/hicolor/%sx%s/apps/rynet.png\n    file_info:\n      mode: 0644\n' "$icons" "$s" "$s" "$s"
    done
    cat <<EOF
scripts:
  postinstall: $here/postinstall.sh
  postremove: $here/postremove.sh
EOF
} > "$work/nfpm.yaml"
for p in deb rpm archlinux; do
    "$tools/nfpm" package -f "$work/nfpm.yaml" -p "$p" -t "$out/" >/dev/null
done

ad="$work/ryNet.AppDir"
mkdir -p "$ad/usr/lib" "$ad/usr/share/icons/hicolor/256x256/apps"
cp -a "$app" "$ad/usr/lib/rynet"
install -m 755 "$here/AppRun" "$ad/AppRun"
sed 's/^Exec=rynet %U/Exec=rynet/' "$here/rynet.desktop" > "$ad/rynet.desktop"
cp "$icons/app-256.png" "$ad/rynet.png"
cp "$icons/app-256.png" "$ad/usr/share/icons/hicolor/256x256/apps/rynet.png"
ARCH=x86_64 "$tools/appimagetool" --appimage-extract-and-run -n "$ad" "$out/ryNet-$ver-x86_64.AppImage" >/dev/null 2>&1

tgz="$work/ryNet-linux-$ver"
mkdir -p "$tgz"
cp -a "$app" "$tgz/rynet"
install -m 755 "$here/install.sh" "$tgz/install.sh"
cp "$here/rynet-plugin.xml" "$tgz/rynet-plugin.xml"
cp "$icons/app-256.png" "$tgz/rynet.png"
tar -C "$work" -cf - "ryNet-linux-$ver" | gzip -9n > "$out/ryNet-linux-$ver.tar.gz"

cli="$work/ryNet-cli-linux-$ver"
mkdir -p "$cli"
install -m 755 "$app/bin/rynet-cli" "$cli/rynet-cli"
tar -C "$work" -cf - "ryNet-cli-linux-$ver" | gzip -9n > "$out/ryNet-cli-linux-$ver.tar.gz"
ls -l "$out"
