import QtQuick

Item {
    id: rail

    property bool expanded: true
    readonly property bool tall: height >= 600
    readonly property real gap: tall ? 20 : 12
    readonly property bool roomy: height >= gap * 2 + 48 + upper.height + 8 + lower.height + 16
    property real wide: 0
    property bool live: false

    readonly property Item sel: picker.selected
    readonly property Item hov: picker.hovered
    Picker {
        id: picker
        items: [home, servers, rules, log, settings]
    }

    width: 96 + 124 * wide

    Spring on wide {
        enabled: rail.live
        to: rail.expanded ? 1 : 0
        stiffness: M.spatialK
        damping: 0.86
    }
    Timer {
        interval: 400
        running: true
        onTriggered: rail.live = true
    }

    NavIndicator {
        anchors.fill: parent
        linger: 90
        color: Qt.rgba(Theme.onSurface.r, Theme.onSurface.g, Theme.onSurface.b, rail.hov && rail.hov.keyboardFocus ? 0.10 : 0.08)
        target: rail.hov ? ({
            key: rail.hov.key,
            x: rail.hov.parent.x + rail.hov.x + rail.hov.ix,
            y: rail.hov.parent.y + rail.hov.y + rail.hov.iy,
            w: rail.hov.iw,
            h: rail.hov.ih
        }) : ({ key: "" })
    }
    Item {
        id: under
        anchors.fill: parent
    }
    NavIndicator {
        anchors.fill: parent
        target: rail.sel ? ({
            key: rail.sel.key,
            x: rail.sel.parent.x + rail.sel.x + rail.sel.ix,
            y: rail.sel.parent.y + rail.sel.y + rail.sel.iy,
            w: rail.sel.iw,
            h: rail.sel.ih
        }) : ({ key: "" })
        down: rail.sel ? rail.sel.down : false
        tint: !rail.sel ? 0 : rail.sel.keyboardFocus ? 0.10 : rail.sel.hovered ? 0.08 : 0
    }

    Item {
        id: brand
        width: rail.width
        y: rail.gap
        height: 48
        visible: rail.roomy
        LogoMark {
            id: mark
            width: 26
            height: 26
            x: 35 - 11 * rail.wide
            anchors.verticalCenter: parent.verticalCenter
            color: Theme.primary
        }
        T {
            x: 60
            anchors.verticalCenter: parent.verticalCenter
            opacity: Math.max(0, rail.wide * 2 - 1)
            visible: opacity > 0
            text: "ryNet"
            type: "titleLargeE"
        }
    }
    Column {
        id: upper
        y: rail.roomy ? brand.y + brand.height + rail.gap : 12
        x: 12
        width: rail.width - 24
        spacing: 4
        NavItem { id: home; under: under; width: parent.width; wide: rail.wide; selected: App.nav === key || App.nav === "subs" || (App.nav === "servers" && Ui.split); key: "home"; icon: "home"; label: Ui.tr("Главная", "Home") }
        NavItem { id: servers; visible: !Ui.split; under: under; width: parent.width; wide: rail.wide; selected: App.nav === key; key: "servers"; icon: "dns"; label: Ui.tr("Серверы", "Servers") }
        NavItem { id: rules; under: under; width: parent.width; wide: rail.wide; selected: App.nav === key; key: "rules"; icon: "edit_note"; motion: "wiggle"; label: Ui.tr("Правила", "Rules") }
    }
    Column {
        id: lower
        y: Math.max(upper.y + upper.height + 8, rail.height - 16 - height)
        x: 12
        width: rail.width - 24
        spacing: 4
        NavItem { id: log; under: under; width: parent.width; wide: rail.wide; selected: App.nav === key; key: "log"; icon: "terminal"; label: Ui.tr("Журнал", "Log") }
        NavItem { id: settings; under: under; width: parent.width; wide: rail.wide; selected: App.nav === key; key: "settings"; icon: "settings"; motion: "spin"; label: Ui.tr("Настройки", "Settings") }
    }
}
