import QtQuick

Column {
    id: picker

    property string sub: App.currentSub
    property string profile
    property var servers: []
    property double loadedAt: 0
    signal picked(string sub, string profile)

    width: parent ? parent.width : 0
    spacing: 8

    function load() {
        servers = []
        if (sub !== "")
            App.loadServersOf(sub)
    }
    onSubChanged: Qt.callLater(load)
    Component.onCompleted: load()
    Connections {
        target: App
        function onServersOfReady(id, list) {
            if (id === picker.sub) {
                picker.loadedAt = Date.now()
                picker.servers = Ui.usable(list)
            }
        }
    }

    Item {
        visible: App.subs.length > 1
        width: parent.width
        height: 32
        Flickable {
            x: -6
            y: -6
            width: parent.width + 12
            height: 44
            contentWidth: chips.implicitWidth + 12
            flickableDirection: Flickable.HorizontalFlick
            boundsBehavior: Flickable.StopAtBounds
            clip: true
            Row {
                id: chips
                x: 6
                y: 6
                spacing: 8
                Repeater {
                    model: App.subs
                    Chip {
                        required property var modelData
                        text: modelData.name
                        selected: modelData.id === picker.sub
                        onClicked: picker.sub = modelData.id
                    }
                }
            }
        }
    }
    Rectangle {
        width: parent.width
        height: Math.min(280, Math.max(56, list.contentHeight + 8))
        radius: M.lg
        color: Theme.surfaceContainer
        ListView {
            id: list
            anchors.fill: parent
            anchors.margins: 4
            clip: true
            spacing: 2
            model: picker.servers
            boundsBehavior: Flickable.StopAtBounds
            delegate: Surface {
                id: row
                required property var modelData
                required property int index
                readonly property bool on: picker.profile === modelData.remarks
                property real round: 0
                property real rise: 0
                width: list.width
                height: 48
                radius: 12 + 12 * Math.max(0, round)
                interactive: true
                color: on ? Theme.secondaryContainer : "transparent"
                tint: on ? Theme.onSecondaryContainer : Theme.onSurface
                transform: Translate { y: row.rise }
                Accessible.name: modelData.name
                Accessible.selected: on
                onClicked: picker.picked(picker.sub, modelData.remarks)
                Spring on round { to: row.on ? 1 : 0; stiffness: M.fastK; damping: 0.6 }
                Spring on rise { id: riseSpring; to: 0; stiffness: M.spatialK; damping: M.spatialZ }
                NumberAnimation {
                    id: fadeIn
                    target: row
                    property: "opacity"
                    to: 1
                    duration: 220
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: M.decelerate
                }
                Timer {
                    id: wait
                    onTriggered: {
                        riseSpring.to = 0
                        fadeIn.start()
                    }
                }
                Component.onCompleted: {
                    if (Date.now() - picker.loadedAt < 300 && index < 8) {
                        opacity = 0
                        riseSpring.snap(12)
                        wait.interval = 1 + index * 22
                        wait.start()
                    }
                }
                Flag {
                    id: f
                    x: 12
                    anchors.verticalCenter: parent.verticalCenter
                    width: 24
                    height: 18
                    flag: row.modelData.flag || ""
                }
                T {
                    anchors.left: f.right
                    anchors.leftMargin: 12
                    anchors.right: parent.right
                    anchors.rightMargin: 12
                    anchors.verticalCenter: parent.verticalCenter
                    text: row.modelData.name
                    type: "bodyLarge"
                    color: row.on ? Theme.onSecondaryContainer : Theme.onSurface
                }
            }
        }
        LoadingIndicator {
            anchors.centerIn: parent
            width: 32
            height: 32
            visible: picker.servers.length === 0 && picker.sub !== ""
            running: visible
            color: Theme.primary
        }
    }
}
