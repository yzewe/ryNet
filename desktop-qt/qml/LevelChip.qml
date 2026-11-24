import QtQuick

Surface {
    id: c

    property string text
    property bool on: false
    property real k: 0

    implicitHeight: 32
    implicitWidth: label.implicitWidth + 32 + 18 * Math.max(0, k)
    radius: M.sm
    interactive: true
    squish: true
    squishScale: 0.94
    color: on ? Theme.secondaryContainer : "transparent"
    borderWidth: on ? 0 : 1
    borderColor: Theme.outlineVariant
    tint: on ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
    Accessible.name: text
    Accessible.checked: on

    Spring on k { to: c.on ? 1 : 0; stiffness: M.fastK; damping: 0.7 }

    Icon {
        x: 8
        anchors.verticalCenter: parent.verticalCenter
        visible: c.k > 0.02
        name: "check"
        size: 18
        weight: 600
        color: Theme.onSecondaryContainer
        opacity: Math.max(0, Math.min(1, c.k * 1.5))
        scale: 0.3 + 0.7 * Math.max(0, c.k)
        rotation: -45 * (1 - Math.min(1, c.k))
    }
    T {
        id: label
        x: 16 + 18 * Math.max(0, c.k)
        anchors.verticalCenter: parent.verticalCenter
        text: c.text
        type: "labelLarge"
        ColorTo on color { to: c.on ? Theme.onSecondaryContainer : Theme.onSurfaceVariant }
    }
}
