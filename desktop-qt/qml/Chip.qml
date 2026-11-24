import QtQuick

Surface {
    id: c

    property string text
    property bool selected: false
    property string icon
    property real check: 0

    readonly property bool leading: icon !== ""
    readonly property real open: leading ? 1 : check
    readonly property color ink: !enabled ? Theme.alpha(Theme.onSurface, 0.38) : selected ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
    readonly property color fill: enabled ? Theme.secondaryContainer : Theme.alpha(Theme.onSurface, 0.12)

    property real corner: 8

    implicitHeight: 32
    implicitWidth: label.implicitWidth + 32 + 18 * open
    radius: corner
    interactive: true
    squish: true
    squishScale: 0.95
    color: selected ? fill : Theme.alpha(fill, 0)
    borderWidth: 1
    borderColor: selected ? fill : !enabled ? Theme.alpha(Theme.onSurface, 0.12) : activeFocus ? Theme.onSurfaceVariant : Theme.outlineVariant
    tint: selected ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
    Accessible.name: text
    Accessible.checked: selected

    Spring on check {
        id: checkSpring
        to: c.selected ? 1 : 0
        stiffness: M.fastK
        damping: c.selected ? M.fastZ : 0.8
    }
    Spring on corner { id: cornering; to: 8; stiffness: M.fastK; damping: 0.55 }
    Component.onCompleted: checkSpring.snap(selected ? 1 : 0)
    onSelectedChanged: cornering.animateFrom(16)

    Icon {
        x: 8
        anchors.verticalCenter: parent.verticalCenter
        width: 18
        height: 18
        name: c.leading ? c.icon : "check"
        size: 18
        weight: 600
        scale: c.leading ? 1 : 0.3 + 0.7 * Math.max(0, c.check)
        opacity: c.leading ? 1 : Math.max(0, Math.min(1, c.check * 1.6 - 0.3))
        visible: opacity > 0
        ColorTo on color { to: c.leading && !c.selected && c.enabled ? Theme.primary : c.ink }
    }
    T {
        id: label
        x: 16 + 18 * c.open
        anchors.verticalCenter: parent.verticalCenter
        text: c.text
        type: "labelLarge"
        ColorTo on color { to: c.ink }
    }
}
