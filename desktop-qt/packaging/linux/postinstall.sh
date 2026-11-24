#!/bin/sh
if command -v setcap >/dev/null 2>&1; then
    for core in /opt/rynet/bin/rynet-core /opt/rynet/bin/rynet-cli; do
        setcap cap_net_admin,cap_net_raw+ep "$core" || true
    done
fi
rm -f /usr/local/lib/rynet/rynet-core /usr/local/lib/rynet/rynet-core.new
rmdir /usr/local/lib/rynet 2>/dev/null || true
if [ -d /etc/polkit-1/rules.d ]; then
    install -m 644 /opt/rynet/share/polkit/49-rynet-resolved.rules /etc/polkit-1/rules.d/49-rynet-resolved.rules 2>/dev/null || true
fi
if [ -d /etc/polkit-1/localauthority/50-local.d ]; then
    install -m 644 /opt/rynet/share/polkit/49-rynet-resolved.pkla /etc/polkit-1/localauthority/50-local.d/49-rynet-resolved.pkla 2>/dev/null || true
fi
if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    firewall-cmd --permanent --zone=trusted --add-interface=rynet0 >/dev/null 2>&1 && firewall-cmd --reload >/dev/null 2>&1 || true
fi
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q 'Status: active'; then
    ufw allow in on rynet0 >/dev/null 2>&1 || true
fi
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications 2>/dev/null || true
command -v update-mime-database >/dev/null 2>&1 && update-mime-database /usr/share/mime 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
exit 0
