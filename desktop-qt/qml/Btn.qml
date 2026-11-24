import QtQuick

Surface {
    id: b

    property string text
    property string icon
    property string kind: "filled"
    property int size: 40
    property bool toggle: false
    property bool checked: false
    property bool busy: false

    readonly property bool flat: kind === "text" || kind === "danger"
    readonly property bool lead: busy || icon !== ""
    readonly property real pad: size >= 56 || (size >= 40 && !flat) ? 24 : 12
    readonly property real slotShown: Math.max(0, Math.min(1, slot))
    readonly property real padStart: size >= 40 && size < 56 && !flat ? pad - 8 * slotShown : pad
    readonly property real padEnd: size >= 40 && size < 56 && flat ? pad + 4 * slotShown : pad
    readonly property real iconSize: size >= 56 ? 24 : 20
    readonly property bool on: !toggle || checked
    readonly property bool selected: toggle && checked
    readonly property color fg: {
        if (!enabled)
            return Theme.alpha(Theme.onSurface, 0.38)
        switch (kind) {
        case "filled": return on ? Theme.onPrimary : Theme.onSurfaceVariant
        case "tonal": return selected ? Theme.onSecondary : Theme.onSecondaryContainer
        case "outlined": return selected ? Theme.inverseOnSurface : Theme.onSurfaceVariant
        case "elevated": return selected ? Theme.onPrimary : Theme.primary
        case "danger": return Theme.error
        default: return Theme.primary
        }
    }
    readonly property color bg: {
        if (!enabled)
            return flat || kind === "outlined" ? "transparent" : Theme.alpha(Theme.onSurface, 0.12)
        switch (kind) {
        case "filled": return on ? Theme.primary : Theme.surfaceContainer
        case "tonal": return selected ? Theme.secondary : Theme.secondaryContainer
        case "outlined": return selected ? Theme.inverseSurface : "transparent"
        case "elevated": return selected ? Theme.primary : Theme.surfaceContainerLow
        default: return "transparent"
        }
    }
    property real shape: 20
    property real glyphFill: 0
    property real slot: 0
    property real wait: 0
    readonly property real waitShown: Math.max(0, Math.min(1, wait))

    implicitHeight: size
    implicitWidth: Math.max(size + 8, row.implicitWidth + padStart + padEnd)
    interactive: !busy
    color: bg
    tint: fg
    borderWidth: kind === "outlined" && !selected ? 1 : 0
    borderColor: enabled ? Theme.outlineVariant : Theme.alpha(Theme.onSurface, 0.12)
    radius: shape
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.checkable: toggle
    Accessible.checked: selected

    Spring on shape {
        to: b.down ? (b.size >= 56 ? 12 : 8) : b.selected ? (b.size >= 56 ? 16 : 12) : b.size / 2
        stiffness: b.down ? M.pressInK : M.pressOutK
        damping: b.down ? 1 : M.pressOutZ
    }
    Spring on glyphFill { to: b.selected ? 1 : 0; stiffness: M.effectsK; damping: 1 }
    Spring on slot { id: opening; to: b.lead ? 1 : 0; stiffness: M.fastK; damping: 0.8 }
    Spring on wait { id: waiting; to: b.busy ? 1 : 0; stiffness: M.fastK; damping: 0.7 }
    Component.onCompleted: {
        opening.snap(lead ? 1 : 0)
        waiting.snap(busy ? 1 : 0)
    }

    Loader {
        z: -1
        anchors.fill: parent
        active: b.kind === "elevated" && b.enabled
        sourceComponent: Shadow {
            radius: b.shape
            level: b.hovered && !b.down ? 2 : 1
        }
    }

    Row {
        id: row
        x: Math.round(b.padStart + (b.width - b.padStart - b.padEnd - width) / 2)
        anchors.verticalCenter: parent.verticalCenter

        Item {
            readonly property real gap: b.text !== "" ? (b.size >= 40 ? 8 : 4) : 0
            width: (b.iconSize + gap) * Math.max(0, b.slot)
            height: b.iconSize
            visible: b.slot > 0.01
            anchors.verticalCenter: parent.verticalCenter
            Item {
                width: b.iconSize
                height: b.iconSize
                Item {
                    anchors.fill: parent
                    visible: b.icon !== "" && b.waitShown < 1
                    opacity: 1 - b.waitShown
                    scale: (1 - 0.5 * b.waitShown) * Math.min(1, Math.max(0, b.slot))
                    Icon {
                        anchors.centerIn: parent
                        visible: opacity > 0
                        opacity: 1 - b.glyphFill
                        name: b.icon
                        size: b.iconSize
                        ColorTo on color { to: b.fg; linear: true }
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
                            ColorTo on color { to: b.fg; linear: true }
                        }
                    }
                }
                LoadingIndicator {
                    anchors.centerIn: parent
                    width: b.iconSize + 4
                    height: width
                    visible: b.waitShown > 0.01
                    opacity: b.waitShown
                    scale: 0.5 + 0.5 * b.wait
                    running: visible
                    color: b.fg
                }
            }
        }
        T {
            visible: b.text !== ""
            text: b.text
            type: b.size >= 56 ? "titleMedium" : "labelLarge"
            anchors.verticalCenter: parent.verticalCenter
            ColorTo on color { to: b.fg; linear: true }
        }
    }
}
