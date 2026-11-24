import QtQuick

Item {
    id: it

    property string key
    property string icon
    property string label
    property bool selected: false
    property real wide: 0
    property real topWidth: 56
    property real topPad: 4
    property real topHeight: 64
    property string motion: "jelly"
    property Item under: null

    readonly property bool down: area.pressed
    readonly property bool hovered: area.containsMouse
    readonly property bool keyboardFocus: activeFocus
    readonly property real ix: lerp((width - topWidth) / 2, 0)
    readonly property real iy: lerp(topPad, 0)
    readonly property real iw: lerp(topWidth, width)
    readonly property real ih: lerp(32, 56)

    function lerp(from, to) {
        return from + (to - from) * wide
    }

    function play() {
        if (motion === "spin")
            spin.to = spin.to + 120
        else if (motion === "wiggle")
            spin.kick(-440)
        else {
            jx.kick(-3.4)
            jy.kick(3.8)
        }
    }

    height: lerp(topHeight, 56)
    activeFocusOnTab: true
    onSelectedChanged: if (selected) play()
    Accessible.role: Accessible.PageTab
    Accessible.name: label
    Accessible.selectable: true
    Accessible.selected: selected
    Accessible.onPressAction: App.nav = it.key
    Keys.onPressed: (e) => {
        if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Space) {
            App.nav = it.key
            e.accepted = true
        }
    }

    StateLayer {
        id: ripple
        property real squeeze: 1
        parent: it.under || it
        x: (it.under && it.parent ? it.parent.x + it.x : 0) + it.ix
        y: (it.under && it.parent ? it.parent.y + it.y : 0) + it.iy
        width: it.iw
        height: it.ih
        radii: Qt.vector4d(height / 2, height / 2, height / 2, height / 2)
        tint: Theme.onSurface
        pressed: area.pressed && !it.selected
        origin: Qt.point(area.at.x - it.ix, area.at.y - it.iy)
        scale: squeeze
        Spring on squeeze {
            to: area.pressed ? 0.94 : 1
            stiffness: area.pressed ? M.pressInK : M.pressOutK
            damping: area.pressed ? 1 : M.pressOutZ
        }
    }
    StateLayer {
        property real squeeze: 1
        x: it.ix
        y: it.iy
        width: it.iw
        height: it.ih
        radii: Qt.vector4d(height / 2, height / 2, height / 2, height / 2)
        tint: Theme.onSecondaryContainer
        pressed: area.pressed && it.selected
        origin: Qt.point(area.at.x - x, area.at.y - y)
        scale: squeeze
        Spring on squeeze {
            to: area.pressed && it.selected ? 0.92 : 1
            stiffness: area.pressed ? M.pressInK : M.pressOutK
            damping: area.pressed ? 1 : M.pressOutZ
        }
    }

    StateLayer {
        x: it.ix - inset
        y: it.iy - inset
        width: it.iw + 2 * inset
        height: it.ih + 2 * inset
        radii: Qt.vector4d(it.ih / 2, it.ih / 2, it.ih / 2, it.ih / 2)
        tint: "transparent"
        focusRing: true
        focused: it.keyboardFocus
    }

    Item {
        id: glyph
        property real sx: 1
        property real sy: 1
        property real turn: 0
        property real press: 1
        property real filled: it.selected ? 1 : 0
        readonly property color tint: it.selected ? Theme.onSecondaryContainer : Theme.onSurfaceVariant

        x: it.lerp((it.width - 24) / 2, 16)
        y: it.lerp(it.topPad + 4, 16)
        width: 24
        height: 24
        transform: [
            Rotation { origin.x: 12; origin.y: 12; angle: glyph.turn },
            Scale { origin.x: 12; origin.y: 12; xScale: glyph.sx * glyph.press; yScale: glyph.sy * glyph.press }
        ]

        Behavior on filled { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
        Spring on sx { id: jx; to: 1; stiffness: 380; damping: 0.42 }
        Spring on sy { id: jy; to: 1; stiffness: 380; damping: 0.42 }
        Spring on turn {
            id: spin
            to: 0
            stiffness: 220
            damping: 0.6
            onRunningChanged: if (!running) snap(0)
        }
        Spring on press {
            to: area.pressed ? 0.84 : 1
            stiffness: area.pressed ? M.pressInK : M.pressOutK
            damping: area.pressed ? 1 : M.pressOutZ
        }

        Icon {
            anchors.fill: parent
            name: it.icon
            opacity: 1 - glyph.filled
            visible: opacity > 0
            ColorTo on color { to: glyph.tint }
        }
        Icon {
            anchors.fill: parent
            name: it.icon
            fill: 1
            opacity: glyph.filled
            visible: opacity > 0
            ColorTo on color { to: glyph.tint }
        }
    }

    T {
        id: text
        x: it.lerp((it.width - implicitWidth) / 2, 52)
        y: it.lerp(it.topPad + 36, (56 - implicitHeight) / 2)
        width: it.wide > 0.5 ? Math.min(implicitWidth, it.width - 68) : implicitWidth
        font.pixelSize: Math.round(it.lerp(12, 14))
        opacity: Math.min(1, Math.abs(it.wide - 0.5) * 2.5)
        text: it.label
        type: it.selected ? "labelLargeE" : "labelLarge"
        ColorTo on color { to: it.selected ? Theme.mix(Theme.onSurface, Theme.onSecondaryContainer, it.wide) : Theme.onSurfaceVariant }
    }

    MouseArea {
        id: area
        property point at
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onPressed: (m) => at = Qt.point(m.x, m.y)
        onClicked: App.nav = it.key
    }
}
