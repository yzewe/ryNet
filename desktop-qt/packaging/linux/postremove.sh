#!/bin/sh
case "${1:-remove}" in
    remove|purge|0)
        rm -f /etc/polkit-1/rules.d/49-rynet-resolved.rules /etc/polkit-1/localauthority/50-local.d/49-rynet-resolved.pkla || true
        rm -f /usr/local/lib/rynet/rynet-core /usr/local/lib/rynet/rynet-core.new || true
        rmdir /usr/local/lib/rynet 2>/dev/null || true
        command -v update-mime-database >/dev/null 2>&1 && update-mime-database /usr/share/mime 2>/dev/null || true
        ;;
esac
exit 0
