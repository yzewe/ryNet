import QtQuick

Item {
    id: st

    property int status: App.status
    property double now: Date.now()
    property int front: 0
    readonly property real need: (front === 0 ? one : two).implicitHeight
    property real cap: 150
    property real room: 150

    implicitHeight: room
    Spring on room {
        to: Math.max(st.need, Math.min(150, st.cap))
        stiffness: M.spatialK
        damping: 0.9
    }

    onStatusChanged: {
        const out = front === 0 ? one : two
        const into = front === 0 ? two : one
        front = 1 - front
        out.leave()
        into.enter(status)
    }
    Component.onCompleted: one.place(status)

    Timer {
        interval: 1000
        repeat: true
        running: st.status === 2 && App.since > 0 && st.visible && st.Window.visibility !== Window.Minimized
                 && st.Window.visibility !== Window.Hidden
        triggeredOnStart: true
        onTriggered: st.now = Date.now()
    }

    component Ticker: Row {
        id: tk

        property string text

        height: 24
        Repeater {
            model: tk.visible ? tk.text.length : 0
            Item {
                id: slot
                required property int index
                readonly property string ch: tk.text.charAt(index)
                property string shown
                property string was
                property real roll: 1

                width: now.implicitWidth
                height: tk.height
                onChChanged: {
                    was = shown
                    shown = ch
                    roller.animateFrom(0)
                }
                Component.onCompleted: shown = ch
                Spring on roll { id: roller; to: 1; stiffness: 520; damping: 0.8 }

                T {
                    id: now
                    text: slot.shown
                    type: "bodyLarge"
                    color: Theme.onSurfaceVariant
                    font.features: { "tnum": 1 }
                    y: (1 - slot.roll) * 9
                    opacity: Math.max(0, Math.min(1, slot.roll * 1.4 - 0.2))
                }
                T {
                    visible: slot.roll < 0.999
                    text: slot.was
                    type: "bodyLarge"
                    color: Theme.onSurfaceVariant
                    font.features: { "tnum": 1 }
                    y: -slot.roll * 9
                    opacity: Math.max(0, 1 - slot.roll * 1.6)
                }
            }
        }
    }

    component Face: Column {
        id: f

        property int status: -1
        property double now
        property real headY: 0
        property real headA: 0
        property real restY: 0
        property real restA: 0

        function place(s) {
            status = s
            hy.snap(0)
            ha.snap(1)
            ry.snap(0)
            ra.snap(1)
        }
        function enter(s) {
            status = s
            z = 1
            ha.stiffness = M.effectsK
            ra.stiffness = M.effectsK
            hy.snap(18)
            ha.snap(0)
            ry.snap(26)
            ra.snap(0)
            hy.to = 0
            show.restart()
            trail.restart()
        }
        function leave() {
            z = 0
            show.stop()
            trail.stop()
            ha.stiffness = M.effectsFastK
            ra.stiffness = M.effectsFastK
            hy.to = -12
            ha.to = 0
            ry.to = -8
            ra.to = 0
        }

        width: parent.width
        spacing: 4
        visible: headA > 0.004 || restA > 0.004

        Spring on headY { id: hy; to: 0; stiffness: 520; damping: 0.66 }
        Spring on headA { id: ha; to: 0; stiffness: M.effectsK; damping: 1 }
        Spring on restY { id: ry; to: 0; stiffness: 520; damping: 0.74 }
        Spring on restA { id: ra; to: 0; stiffness: M.effectsK; damping: 1 }
        Timer {
            id: show
            interval: 50
            onTriggered: ha.to = 1
        }
        Timer {
            id: trail
            interval: 110
            onTriggered: {
                ry.to = 0
                ra.to = 1
            }
        }

        T {
            width: Math.min(parent.width - 32, implicitWidth)
            fontSizeMode: Text.HorizontalFit
            minimumPixelSize: 18
            horizontalAlignment: Text.AlignHCenter
            anchors.horizontalCenter: parent.horizontalCenter
            opacity: Math.min(1, f.headA)
            transform: Translate { y: f.headY }
            text: f.status === 3 ? (App.blocked ? Ui.tr("Интернет заблокирован", "Internet blocked") : Ui.tr("Не удалось", "Failed"))
                : f.status === 2 ? Ui.tr("Защищено", "Protected")
                : f.status === 1 ? Ui.tr("Подключение…", "Connecting…")
                : Ui.tr("Не подключено", "Not connected")
            type: "headlineMediumE"
            color: f.status === 3 ? Theme.error : Theme.onSurface
        }
        Column {
            width: parent.width
            spacing: 4
            opacity: Math.min(1, f.restA)
            transform: Translate { y: f.restY }

            Ticker {
                visible: f.status === 2
                anchors.horizontalCenter: parent.horizontalCenter
                text: f.now > 0 && App.since > 0 ? App.uptime(App.since) : " "
            }
            Row {
                id: exit
                property real shown: App.exitIp !== "" ? 1 : 0
                visible: f.status === 2
                anchors.horizontalCenter: parent.horizontalCenter
                height: 28
                spacing: 8
                opacity: shown
                transform: Translate { y: (1 - exit.shown) * 6 }
                Behavior on shown { NumberAnimation { duration: 260; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate } }
                Flag {
                    visible: App.exitFlag !== ""
                    width: 24
                    height: 18
                    radius: 5
                    flag: App.exitFlag
                    anchors.verticalCenter: parent.verticalCenter
                }
                T {
                    text: App.exitIp
                    type: "labelLarge"
                    color: Theme.onSurfaceVariant
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
            T {
                id: why
                readonly property var reason: Ui.errorReason(App.failure)
                property bool open: false
                visible: f.status === 3
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(460, parent.width - 48)
                horizontalAlignment: Text.AlignHCenter
                text: why.reason.title + "\n" + (App.blocked ? Ui.tr("VPN не подключён — трафик закрыт", "VPN not connected — traffic blocked") : why.reason.hint)
                type: "bodyLarge"
                color: Theme.onSurfaceVariant
                wrapMode: Text.Wrap
                maximumLineCount: Math.max(2, Math.min(4, Math.floor((st.cap - 120) / 24)))
                elide: Text.ElideRight
                onReasonChanged: open = false
            }
            Fold {
                visible: f.status === 3 && why.reason.details !== ""
                open: why.open
                T {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(460, parent.width - 48)
                    horizontalAlignment: Text.AlignHCenter
                    topPadding: 4
                    text: why.reason.details
                    type: "bodySmall"
                    color: Theme.onSurfaceVariant
                    wrapMode: Text.Wrap
                    maximumLineCount: 4
                    elide: Text.ElideRight
                }
            }
            Btn {
                visible: f.status === 3 && why.reason.details !== ""
                anchors.horizontalCenter: parent.horizontalCenter
                kind: "text"
                text: why.open ? Ui.tr("Скрыть", "Hide") : Ui.tr("Подробнее", "Details")
                onClicked: why.open = !why.open
            }
            Item {
                visible: f.status === 3
                width: 1
                height: 12
            }
            Row {
                visible: f.status === 3
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 8
                Btn {
                    kind: "tonal"
                    icon: App.blocked ? "" : "refresh"
                    text: App.blocked ? Ui.tr("Отключить", "Disconnect") : Ui.tr("Повторить", "Retry")
                    onClicked: App.blocked ? App.disconnectVpn() : App.connectVpn()
                }
                Btn {
                    kind: "outlined"
                    text: Ui.tr("Другой сервер", "Another server")
                    onClicked: App.nav = "servers"
                }
            }
        }
    }

    Face {
        id: one
        now: st.now
    }
    Face {
        id: two
        now: st.now
    }
}
