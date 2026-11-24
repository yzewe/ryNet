#!/usr/bin/env bash
set -euo pipefail
umask 022
src=/src/desktop-qt
b=/build/desktop-qt
st=/build/desktop-qt-stage/rynet
qt=/opt/env/qt

cmake -S "$src" -B "$b" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$qt;/opt/env/deps" \
    -DCMAKE_C_COMPILER_LAUNCHER=ccache -DCMAKE_CXX_COMPILER_LAUNCHER=ccache \
    -DCMAKE_SKIP_BUILD_RPATH=ON -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON '-DCMAKE_INSTALL_RPATH=$ORIGIN/../lib' >/dev/null
cmake --build "$b"

rm -rf "$st" && mkdir -p "$st/bin" "$st/lib" "$st/plugins" "$st/qml"
cp "$b/rynet" "$st/bin/"
printf '[Paths]\nPrefix = ..\n' > "$st/bin/qt.conf"

cd "$qt/plugins"
for p in platforms/libqxcb.so platforms/libqwayland*.so platforms/libqoffscreen.so xcbglintegrations/*.so \
    wayland-shell-integration/libxdg-shell.so wayland-decoration-client/*.so wayland-graphics-integration-client/libqt-plugin-wayland-egl.so \
    platforminputcontexts/*.so tls/libqopensslbackend.so; do
    [ -e "$p" ] || continue
    mkdir -p "$st/plugins/$(dirname "$p")" && cp "$p" "$st/plugins/$p"
done

"$qt/libexec/qmlimportscanner" -rootPath "$src/qml" -importPath "$qt/qml" | python3 -c '
import json, sys
for m in json.load(sys.stdin):
    if m.get("type") == "module" and m.get("path", "").startswith("/opt/env/qt/qml/"):
        print(m["path"])' | sort -u | while read -r d; do
    rel=${d#"$qt/qml/"}
    mkdir -p "$st/qml/$rel"
    find "$d" -maxdepth 1 -type f ! -name '*.qmltypes' ! -name '*.debug' -exec cp {} "$st/qml/$rel/" \;
done

bundle='libxcb-cursor.so.0 libxcb-icccm.so.4 libxcb-image.so.0 libxcb-keysyms.so.1 libxcb-render-util.so.0 libxcb-util.so.1 libxkbcommon-x11.so.0'
while :; do
    new=0
    while read -r name path; do
        [ -e "$st/lib/$name" ] && continue
        case "$path" in
            "$qt"/lib/*) ;;
            *) [[ " $bundle " == *" $name "* ]] || continue ;;
        esac
        cp -L "$path" "$st/lib/$name"; new=1
    done < <(find "$st" -type f \( -name '*.so*' -o -path '*/bin/rynet' \) -exec env LD_LIBRARY_PATH="$qt/lib" ldd {} \; 2>/dev/null \
        | awk '$2 == "=>" && $3 ~ /^\// { print $1, $3 }' | sort -u)
    [ $new = 1 ] || break
done
missing=$(find "$st" -type f \( -name '*.so*' -o -path '*/bin/rynet' \) -exec ldd {} \; 2>/dev/null | grep 'not found' | sort -u || true)
[ -z "$missing" ] || { echo "не нашлись библиотеки: $missing" >&2; exit 1; }
find "$st" -type f -name '*.so*' -exec strip --strip-unneeded {} +
du -sh "$st"
