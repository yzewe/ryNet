import QtQuick

Surface {
    id: card

    property var server: ({})
    property string sub
    property bool auto: false
    readonly property string autoLine: auto && App.currentSub !== undefined ? Ui.joined([Ui.tr("авто", "auto"), App.subName(sub)], " · ") : ""
    readonly property bool has: server.remarks !== undefined
    readonly property bool elsewhere: sub !== "" && sub !== App.currentSub
    readonly property var ping: (App.rev < 0 ? undefined : !has ? ({}) : elsewhere ? App.pingIn(sub, server.remarks) : App.ping(server.remarks))
    readonly property bool waiting: has && (App.rev >= 0 && App.pingWaitingFor(server.remarks, sub))
    readonly property string shownKey: has ? server.remarks : ""
    property var held: ({ server: ({}), ping: ({}) })

    function hold() {
        held = { server: card.server, ping: card.ping }
    }

    height: 72
    radius: M.xl
    color: Theme.surfaceContainerHigh
    interactive: true
    squish: true
    squishScale: 0.97
    Accessible.name: has ? (server.name || "") : Ui.tr("Выберите сервер", "Choose a server")

    onShownKeyChanged: {
        ghost.server = held.server
        ghost.ping = held.ping
        ghost.leave()
        live.enter()
        Qt.callLater(hold)
    }
    onPingChanged: Qt.callLater(hold)
    onWaitingChanged: if (!waiting && held.server.remarks === shownKey) live.bump()
    Component.onCompleted: hold()

    component Face: Item {
        id: f

        property var server: ({})
        property var ping: ({})
        property bool waiting: false
        property bool hovered: false
        property bool resting: true
        readonly property bool has: server.remarks !== undefined
        property real dy: 0
        property real fade: 1
        property real nudge: 0

        function enter() {
            z = 1
            fader.stiffness = M.effectsK
            slide.damping = 0.72
            slide.snap(26)
            fader.snap(0)
            slide.to = 0
            showing.restart()
        }
        function leave() {
            z = 0
            showing.stop()
            fader.stiffness = M.effectsFastK
            slide.damping = 0.9
            slide.snap(0)
            fader.snap(1)
            slide.to = -22
            fader.to = 0
        }
        function bump() {
            pingText.bump()
        }

        anchors.fill: parent
        opacity: fade
        visible: fade > 0.004
        transform: Translate { y: f.dy }

        Spring on dy { id: slide; to: 0; stiffness: 420; damping: 0.72 }
        Spring on fade { id: fader; to: f.resting ? 1 : 0; stiffness: M.effectsK; damping: 1 }
        Spring on nudge { to: f.hovered ? 3 : 0; stiffness: M.fastK; damping: M.fastZ }
        Timer {
            id: showing
            interval: 50
            onTriggered: fader.to = 1
        }

        Flag {
            id: flag
            x: 8
            y: 8
            width: 75
            height: 56
            radius: M.lgPlus
            flag: f.server.flag || ""
        }
        Column {
            anchors.left: flag.right
            anchors.leftMargin: 16
            anchors.right: tail.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            T {
                width: parent.width
                text: f.has ? f.server.name : Ui.tr("Выберите сервер", "Choose a server")
                type: "titleMediumE"
            }
            T {
                readonly property string info: App.settings.serverInfo
                width: parent.width
                visible: f.has && (info !== "none" || card.autoLine !== "")
                text: card.autoLine !== "" ? card.autoLine : info === "description" ? (f.server.description || f.server.kind || "") : (f.server.kind || "")
                type: "bodyMedium"
                color: Theme.onSurfaceVariant
            }
        }
        Row {
            id: tail
            anchors.right: parent.right
            anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            LoadingIndicator {
                visible: f.waiting
                running: visible
                width: 32
                height: 32
                color: Theme.primary
                anchors.verticalCenter: parent.verticalCenter
            }
            Surface {
                visible: f.has && !f.waiting
                width: pingText.implicitWidth + 16
                height: 48
                radius: M.full
                color: "transparent"
                tint: Theme.primary
                interactive: f.resting && !App.pinging
                Accessible.name: Ui.tr("Проверить пинг", "Check ping")
                onClicked: App.pingSome([f.server.remarks], card.sub)
                anchors.verticalCenter: parent.verticalCenter
                PingText {
                    id: pingText
                    anchors.centerIn: parent
                    ping: f.ping
                    type: "titleLargeE"
                }
            }
            Icon {
                name: "chevron_right"
                anchors.verticalCenter: parent.verticalCenter
                transform: Translate { x: f.nudge }
            }
        }
    }

    Item {
        anchors.fill: parent
        clip: true
        Face {
            id: ghost
            resting: false
        }
        Face {
            id: live
            server: card.server
            ping: card.ping
            waiting: card.waiting
            hovered: card.hovered
        }
    }
}
