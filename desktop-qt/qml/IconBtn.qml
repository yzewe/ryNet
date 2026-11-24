import QtQuick

Surface {
    id: b

    property string icon
    property string kind: "standard"
    property int size: 40
    property real iconSize: 24
    property bool toggle: false
    property bool checked: false
    property string tip
    readonly property bool selected: toggle && checked
    property color iconColor: {
        if (!enabled)
            return Theme.alpha(Theme.onSurface, 0.38)
        switch (kind) {
        case "filled": return toggle && !checked ? Theme.onSurfaceVariant : Theme.onPrimary
        case "tonal": return selected ? Theme.onSecondary : Theme.onSecondaryContainer
        case "outlined": return selected ? Theme.inverseOnSurface : Theme.onSurfaceVariant
        default: return selected ? Theme.primary : Theme.onSurfaceVariant
        }
    }
    property real shape: 20
    property real glyphFill: 0

    implicitWidth: size
    implicitHeight: size
    interactive: true
    color: {
        if (!enabled)
            return kind === "filled" || kind === "tonal" ? Theme.alpha(Theme.onSurface, 0.12) : "transparent"
        switch (kind) {
        case "filled": return toggle && !checked ? Theme.surfaceContainer : Theme.primary
        case "tonal": return selected ? Theme.secondary : Theme.secondaryContainer
        case "outlined": return selected ? Theme.inverseSurface : "transparent"
        default: return "transparent"
        }
    }
    tint: iconColor
    borderWidth: kind === "outlined" && !selected ? 1 : 0
    borderColor: enabled ? Theme.outlineVariant : Theme.alpha(Theme.onSurface, 0.12)
    radius: shape
    Accessible.name: tip
    Accessible.checkable: toggle
    Accessible.checked: selected

    Spring on shape {
        to: b.down ? (b.size >= 56 ? 12 : 8) : b.selected ? (b.size >= 56 ? 16 : 12) : b.size / 2
        stiffness: b.down ? M.pressInK : M.pressOutK
        damping: b.down ? 1 : M.pressOutZ
    }
    Spring on glyphFill { to: b.selected ? 1 : 0; stiffness: M.effectsK; damping: 1 }

    onSelectedChanged: {
        if (selected) {
            pop.kick(2.6)
            twist.kick(-300)
        } else {
            pop.kick(-2.2)
        }
    }

    Item {
        id: glyph
        property real dip: 1
        property real swell: 1
        property real turn: 0
        anchors.centerIn: parent
        width: b.iconSize
        height: b.iconSize
        scale: dip * swell
        rotation: turn

        Spring on dip {
            to: b.down ? 0.86 : 1
            stiffness: b.down ? M.pressInK : M.pressOutK
            damping: b.down ? 1 : M.pressOutZ
        }
        Spring on swell { id: pop; to: 1; stiffness: 520; damping: 0.4 }
        Spring on turn { id: twist; to: 0; stiffness: 420; damping: 0.38 }

        Icon {
            anchors.centerIn: parent
            visible: opacity > 0
            opacity: 1 - b.glyphFill
            name: b.icon
            size: b.iconSize
            ColorTo on color { to: b.iconColor; linear: true }
        }
        Loader {
            anchors.centerIn: parent
            active: b.toggle
            sourceComponent: Icon {
                visible: opacity > 0
                opacity: b.glyphFill
                name: b.icon
                size: b.iconSize
                fill: 1
                renderType: Text.CurveRendering
                ColorTo on color { to: b.iconColor; linear: true }
            }
        }
    }

    Timer {
        interval: 600
        running: b.tip !== "" && !b.tipSpent && ((b.hovered && !b.pressed) || b.keyboardFocus)
        onTriggered: Ui.showTip(b, b.tip)
    }
    property bool tipSpent: false
    onHoveredChanged: {
        if (!hovered)
            tipSpent = false
        if (!hovered && !keyboardFocus)
            Ui.hideTip(b)
    }
    onKeyboardFocusChanged: if (!keyboardFocus && !hovered) Ui.hideTip(b)
    onPressedChanged: if (pressed) { tipSpent = true; Ui.hideTip(b) }
    Component.onDestruction: Ui.hideTip(b)
}
