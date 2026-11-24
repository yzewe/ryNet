import QtQuick

Item {
    id: sw

    property bool checked: false
    property bool interactive: true
    property bool pressedOutside: false
    property bool hoveredOutside: false
    signal toggled(bool value)

    readonly property bool down: area.pressed || held.running || pressedOutside
    readonly property bool hovered: (area.containsMouse || hoveredOutside) && enabled
    readonly property bool keyboardFocus: activeFocus
    readonly property bool lit: enabled && (down || hovered || keyboardFocus)

    property real pos: 16
    property real thumb: 16
    property real trail: 16
    readonly property real stretch: Math.min(10, Math.abs(pos - trail))

    readonly property color trackColor: !enabled ? (checked ? Theme.alpha(Theme.onSurface, 0.12) : Theme.alpha(Theme.surfaceContainerHighest, 0.12))
        : checked ? Theme.primary : Theme.surfaceContainerHighest
    readonly property color outlineColor: !enabled ? (checked ? "transparent" : Theme.alpha(Theme.onSurface, 0.12))
        : checked ? Theme.primary : Theme.outline
    readonly property color handleColor: !enabled ? (checked ? Theme.surface : Theme.alpha(Theme.onSurface, 0.38))
        : checked ? (lit ? Theme.primaryContainer : Theme.onPrimary) : (lit ? Theme.onSurfaceVariant : Theme.outline)

    implicitWidth: 52
    implicitHeight: 32
    activeFocusOnTab: interactive && enabled
    Accessible.role: Accessible.Switch
    Accessible.ignored: !interactive
    Accessible.checkable: true
    Accessible.checked: checked
    Accessible.onPressAction: if (interactive && enabled) sw.toggled(!sw.checked)
    Accessible.onToggleAction: if (interactive && enabled) sw.toggled(!sw.checked)
    Keys.onPressed: (e) => {
        if (interactive && (e.key === Qt.Key_Space || e.key === Qt.Key_Return || e.key === Qt.Key_Enter)) {
            held.restart()
            sw.toggled(!sw.checked)
            e.accepted = true
        }
    }

    Spring on pos {
        id: travel
        to: sw.checked ? 36 : 16
        stiffness: M.fastK
        damping: 0.7
    }
    Spring on thumb {
        id: grow
        to: sw.down ? 28 : sw.checked ? 24 : 16
        stiffness: sw.down ? M.pressInK : M.fastK
        damping: sw.down ? 1 : 0.85
    }
    Spring on trail {
        id: trailing
        to: sw.checked ? 36 : 16
        stiffness: 420
        damping: 0.9
    }
    Component.onCompleted: {
        travel.snap(checked ? 36 : 16)
        trailing.snap(checked ? 36 : 16)
        grow.snap(down ? 28 : checked ? 24 : 16)
    }

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        border.width: sw.enabled || !sw.checked ? 2 : 0
        ColorTo on color { to: sw.trackColor }
        ColorTo on border.color { to: sw.outlineColor }
    }
    StateLayer {
        x: sw.pos - 20
        y: (sw.height - 40) / 2
        width: 40
        height: 40
        radii: Qt.vector4d(20, 20, 20, 20)
        tint: sw.checked ? Theme.primary : Theme.onSurface
        hovered: sw.hovered
        focused: sw.keyboardFocus
        pressed: area.pressed || held.running || (sw.pressedOutside && sw.hovered)
    }
    StateLayer {
        anchors.fill: parent
        anchors.margins: -inset
        radii: Qt.vector4d(16, 16, 16, 16)
        focusRing: sw.interactive
        focused: sw.keyboardFocus
        stateAlpha: 0
    }
    Rectangle {
        readonly property real across: sw.thumb - sw.stretch * 0.2
        width: sw.thumb + sw.stretch
        height: across
        radius: across / 2
        x: sw.pos >= sw.trail ? sw.pos + sw.thumb / 2 - width : sw.pos - sw.thumb / 2
        y: (sw.height - across) / 2
        ColorTo on color { to: sw.handleColor }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        enabled: sw.interactive && sw.enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onPressed: held.restart()
        onClicked: sw.toggled(!sw.checked)
    }
    Timer {
        id: held
        interval: M.minPressMs
    }
}
