#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
dest="$HOME/.local/opt/rynet"
pkill -f "$dest/bin/rynet" 2>/dev/null || true
pkill -f "$dest/bin/rynet-core" 2>/dev/null || true
sleep 2
pkill -9 -f "$dest/bin/rynet" 2>/dev/null || true
pkill -9 -f "$dest/bin/rynet-core" 2>/dev/null || true
rm -rf "$dest"
mkdir -p "$dest" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/256x256/apps" "$HOME/.local/bin"
cp -r "$here/rynet/." "$dest/"
chmod +x "$dest/bin/rynet" "$dest/bin/rynet-core" "$dest/bin/rynet-cli"
cp "$here/rynet.png" "$HOME/.local/share/icons/hicolor/256x256/apps/rynet.png"
ln -sf "$dest/bin/rynet" "$HOME/.local/bin/rynet"
ln -sf "$dest/bin/rynet-cli" "$HOME/.local/bin/rynet-cli"
cat > "$HOME/.local/share/applications/rynet.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=ryNet
Comment=VPN-клиент
Exec=$dest/bin/rynet %U
Icon=rynet
Categories=Network;
StartupWMClass=rynet
MimeType=x-scheme-handler/rynet;application/vnd.rynet.plugin;
DESK
mkdir -p "$HOME/.local/share/mime/packages"
cp "$here/rynet-plugin.xml" "$HOME/.local/share/mime/packages/rynet-plugin.xml"
update-mime-database "$HOME/.local/share/mime" 2>/dev/null || true
xdg-mime default rynet.desktop x-scheme-handler/rynet 2>/dev/null || true
xdg-mime default rynet.desktop application/vnd.rynet.plugin 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "ryNet установлен: меню приложений или команда rynet"
echo "Режим «Весь трафик» после обновления нужно разрешить заново (Настройки → Сеть)."
