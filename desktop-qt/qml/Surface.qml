import QtQuick

Item {
    id: s

    property color color: "transparent"
    property real radius: 0
    property real topLeftRadius: radius
    property real topRightRadius: radius
    property real bottomLeftRadius: radius
    property real bottomRightRadius: radius
    property color borderColor: "transparent"
    property real borderWidth: 0
    property bool interactive: false
    property bool rightClicks: false
    property color tint: Theme.onSurface
    property bool squish: false
    property real squishScale: 0.96
    property bool animateColor: true
    property bool focusRing: true
    readonly property bool hovered: press.containsMouse
    readonly property bool pressed: press.mousePressed
    readonly property bool down: press.down
    readonly property bool keyboardFocus: activeFocus
    readonly property real squeeze: press.squeeze

    signal clicked(real x, real y)
    signal rightClicked(real x, real y)
    signal longPressed(real x, real y)
    signal holdMoved(real x, real y)
    signal holdReleased(real x, real y)

    activeFocusOnTab: (interactive && enabled) || activeFocus
    scale: squeeze
    Accessible.role: interactive ? Accessible.Button : Accessible.NoRole
    Accessible.onPressAction: if (interactive && enabled) s.clicked(width / 2, height / 2)

    Rectangle {
        anchors.fill: parent
        topLeftRadius: s.topLeftRadius
        topRightRadius: s.topRightRadius
        bottomLeftRadius: s.bottomLeftRadius
        bottomRightRadius: s.bottomRightRadius
        border.width: s.borderWidth
        visible: s.color.a > 0 || s.borderWidth > 0
        ColorTo on color { to: s.color; animate: s.animateColor }
        ColorTo on border.color { to: s.borderColor; linear: true }
    }

    Press {
        id: press
        interactive: s.interactive
        rightClicks: s.rightClicks
        squish: s.squish
        squishScale: s.squishScale
        focusRing: s.focusRing && s.interactive
        radii: Qt.vector4d(s.topLeftRadius, s.topRightRadius, s.bottomRightRadius, s.bottomLeftRadius)
        tint: s.tint
        fadeTint: s.animateColor
        onClicked: (x, y) => s.clicked(x, y)
        onRightClicked: (x, y) => s.rightClicked(x, y)
        onLongPressed: (x, y) => s.longPressed(x, y)
        onHoldMoved: (x, y) => s.holdMoved(x, y)
        onHoldReleased: (x, y) => s.holdReleased(x, y)
    }
}
