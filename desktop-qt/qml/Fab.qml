import QtQuick

Item {
    id: f

    property string icon
    property string text
    property bool busy: false
    property bool extended: text !== ""
    property string label: text
    property color container: Theme.primaryContainer
    property color content: Theme.onPrimaryContainer
    signal clicked()

    property real lift: 3
    property real corner: 16
    property real wait: 0
    readonly property real waitShown: Math.max(0, Math.min(1, wait))

    implicitHeight: 56
    implicitWidth: extended ? row.implicitWidth + 32 : 56

    Spring on lift { to: surface.hovered && !surface.down ? 4 : 3; stiffness: M.effectsK; damping: 1 }
    Spring on corner {
        to: surface.down ? 12 : 16
        stiffness: surface.down ? M.pressInK : M.pressOutK
        damping: surface.down ? 1 : M.pressOutZ
    }
    Spring on wait { to: f.busy ? 1 : 0; stiffness: M.fastK; damping: 0.7 }

    Shadow {
        anchors.fill: surface
        radius: surface.radius
        level: f.lift
        scale: surface.scale
    }
    Surface {
        id: surface
        anchors.fill: parent
        radius: f.corner
        interactive: true
        squish: true
        squishScale: 0.94
        color: f.container
        tint: f.content
        Accessible.name: f.label
        onClicked: f.clicked()

        Row {
            id: row
            anchors.centerIn: parent
            spacing: 8
            Item {
                width: 24
                height: 24
                visible: f.icon !== "" || f.busy
                anchors.verticalCenter: parent.verticalCenter
                Icon {
                    anchors.centerIn: parent
                    visible: opacity > 0
                    opacity: 1 - f.waitShown
                    scale: 1 - 0.5 * f.waitShown
                    name: f.icon
                    color: f.content
                }
                LoadingIndicator {
                    anchors.centerIn: parent
                    width: 32
                    height: 32
                    visible: opacity > 0
                    opacity: f.waitShown
                    scale: 0.5 + 0.5 * f.wait
                    running: visible
                    color: f.content
                }
            }
            T {
                visible: f.extended
                text: f.text
                type: "titleMedium"
                color: f.content
                anchors.verticalCenter: parent.verticalCenter
            }
        }
    }
}
