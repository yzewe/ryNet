import QtQuick

Surface {
    id: w

    property var colors: []
    property string icon
    property bool selected: false
    property color gap: Theme.surfaceContainerHigh
    property real sel: 0
    property real spin: 0
    readonly property bool filled: colors.length === 3

    function play() { spinner.to = spinner.to + 90 }

    implicitWidth: 52
    implicitHeight: 52
    radius: width / 2
    interactive: true
    squish: true
    squishScale: 0.88
    Accessible.role: Accessible.RadioButton
    Accessible.checkable: true
    Accessible.checked: selected

    Spring on sel {
        id: picking
        to: w.selected ? 1 : 0
        stiffness: M.fastK
        damping: 0.6
    }
    Spring on spin {
        id: spinner
        to: 0
        stiffness: M.bouncyK
        damping: M.bouncyZ
    }
    Component.onCompleted: picking.snap(selected ? 1 : 0)

    MShape {
        anchors.centerIn: parent
        width: 50
        height: 50
        visible: w.sel > 0.01
        shape: "Cookie12Sided"
        color: Theme.primary
        opacity: Math.min(1, w.sel * 2)
        scale: 0.7 + 0.3 * w.sel
        rotation: w.sel > 0.999 ? 0 : -30 * (1 - w.sel)
    }
    Rectangle {
        anchors.centerIn: parent
        width: 40
        height: 40
        radius: 20
        visible: w.sel > 0.01
        color: w.gap
        scale: 1 - 0.1 * w.sel
    }
    Rectangle {
        id: dot
        anchors.centerIn: parent
        width: 40
        height: 40
        radius: 20
        visible: w.filled
        scale: 1 - 0.2 * w.sel
        color: w.filled ? w.colors[1] : "transparent"
        Repeater {
            model: [[0, 0, 40, 0], [20, 20, 20, 2]]
            Item {
                required property var modelData
                x: modelData[0]
                y: modelData[1]
                width: modelData[2]
                height: 20
                clip: true
                Rectangle {
                    x: -parent.x
                    y: -parent.y
                    width: 40
                    height: 40
                    radius: 20
                    color: w.filled ? w.colors[parent.modelData[3]] : "transparent"
                }
            }
        }
    }
    Rectangle {
        anchors.centerIn: parent
        width: 40
        height: 40
        radius: 20
        visible: !w.filled
        color: "transparent"
        border.width: 1
        border.color: Theme.outline
        Icon {
            anchors.centerIn: parent
            name: w.icon
            size: 20
            color: Theme.onSurfaceVariant
            rotation: w.spin
        }
    }
}
