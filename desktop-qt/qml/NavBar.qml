import QtQuick

Rectangle {
    id: bar

    readonly property Item sel: picker.selected
    readonly property Item hov: picker.hovered
    Picker {
        id: picker
        items: [home, servers, settings]
    }

    height: 80
    color: Theme.surfaceContainer

    NavIndicator {
        anchors.fill: parent
        vertical: false
        linger: 90
        color: Qt.rgba(Theme.onSurface.r, Theme.onSurface.g, Theme.onSurface.b, bar.hov && bar.hov.keyboardFocus ? 0.10 : 0.08)
        target: bar.hov ? ({
            key: bar.hov.key,
            x: row.x + bar.hov.x + bar.hov.ix,
            y: row.y + bar.hov.y + bar.hov.iy,
            w: bar.hov.iw,
            h: bar.hov.ih
        }) : ({ key: "" })
    }
    Item {
        id: under
        anchors.fill: parent
    }
    NavIndicator {
        anchors.fill: parent
        vertical: false
        target: bar.sel ? ({
            key: bar.sel.key,
            x: row.x + bar.sel.x + bar.sel.ix,
            y: row.y + bar.sel.y + bar.sel.iy,
            w: bar.sel.iw,
            h: bar.sel.ih
        }) : ({ key: "" })
        down: bar.sel ? bar.sel.down : false
        tint: !bar.sel ? 0 : bar.sel.keyboardFocus ? 0.10 : bar.sel.hovered ? 0.08 : 0
    }

    Row {
        id: row
        anchors.fill: parent
        NavItem { id: home; under: under; width: bar.width / 3; topWidth: 64; topPad: 12; topHeight: 80; selected: App.nav === key || App.nav === "subs"; key: "home"; icon: "home"; label: Ui.tr("Главная", "Home") }
        NavItem { id: servers; under: under; width: bar.width / 3; topWidth: 64; topPad: 12; topHeight: 80; selected: App.nav === key; key: "servers"; icon: "dns"; label: Ui.tr("Серверы", "Servers") }
        NavItem { id: settings; under: under; width: bar.width / 3; topWidth: 64; topPad: 12; topHeight: 80; selected: ["settings", "rules", "log"].includes(App.nav); key: "settings"; icon: "settings"; motion: "spin"; label: Ui.tr("Настройки", "Settings") }
    }
}
