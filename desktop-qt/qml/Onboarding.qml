import QtQuick

Rectangle {
    id: o
    property bool shown: false

    color: Theme.surface

    IconBtn {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 16
        icon: "settings"
        tip: Ui.tr("Настройки", "Settings")
        onClicked: { App.adding = false; App.nav = "settings" }
    }

    ServerImportDialog { id: serverImport }
    IconBtn {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.topMargin: 16
        anchors.rightMargin: 64
        icon: "add"
        tip: Ui.tr("Добавить сервер", "Add server")
        onClicked: serverImport.show()
    }

    function go() {
        if (field.text.trim() !== "" && !App.busy)
            App.add(field.text.trim())
    }

    Keys.onEscapePressed: if (App.subs.length > 0) App.adding = false

    Connections {
        target: o.Window.window
        enabled: !o.shown
        function onFrameSwapped() { o.shown = true }
    }

    component Beat: Item {
        id: beat
        property int order: 0
        property bool live: false
        property real lift: 0
        signal started()

        width: parent ? parent.width : 0
        height: childrenRect.height
        opacity: 0
        transform: Translate { y: beat.lift }

        Spring on lift {
            id: lifting
            to: 28
            stiffness: M.spatialK
            damping: 0.75
        }
        NumberAnimation on opacity {
            id: showing
            running: false
            to: 1
            duration: 260
            easing.type: Easing.BezierSpline
            easing.bezierCurve: M.decelerate
        }
        Timer {
            running: beat.live
            interval: 1 + beat.order * 70
            onTriggered: {
                lifting.to = 0
                showing.start()
                beat.started()
            }
        }
    }

    Column {
        anchors.centerIn: parent
        width: Math.min(520, o.width - 48)
        spacing: 0

        Beat {
            order: 0
            live: o.shown
            onStarted: {
                growing.to = 1
                turning.to = 0
                marking.start()
            }
            Item {
                id: hero
                property real grow: 0.6
                property real turn: -40
                property real mark: 0.5

                width: o.height < 640 ? 128 : 180
                height: width
                anchors.horizontalCenter: parent.horizontalCenter
                scale: grow

                Spring on grow {
                    id: growing
                    to: 0.6
                    stiffness: 300
                    damping: 0.55
                }
                Spring on turn {
                    id: turning
                    to: -40
                    stiffness: 160
                    damping: 0.7
                }
                Spring on mark {
                    id: popping
                    to: 0.5
                    stiffness: 420
                    damping: 0.5
                }
                Timer {
                    id: marking
                    interval: 120
                    onTriggered: popping.to = 1
                }

                Item {
                    anchors.fill: parent
                    rotation: hero.turn
                    MShape {
                        anchors.fill: parent
                        shape: "Cookie9Sided"
                        color: Theme.primaryContainer
                        RotationAnimation on rotation {
                            from: 0
                            to: 360
                            duration: 24000
                            loops: Animation.Infinite
                        }
                    }
                }
                LogoMark {
                    anchors.centerIn: parent
                    width: hero.width * 0.47
                    height: width
                    color: Theme.onPrimaryContainer
                    scale: hero.mark
                    opacity: Math.max(0, Math.min(1, (hero.mark - 0.5) * 3))
                }
            }
        }
        Item { width: 1; height: 28 }
        Beat {
            order: 1
            live: o.shown
            T {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                text: Ui.tr("Добавьте подписку", "Add a subscription")
                type: o.width < 480 ? "headlineLarge" : "displaySmall"
                wrapMode: Text.Wrap
                elide: Text.ElideNone
            }
        }
        Item { width: 1; height: 28 }
        Beat {
            order: 2
            live: o.shown
            Field {
                id: field
                width: parent.width
                label: Ui.tr("Ссылка на подписку", "Subscription link")
                bg: Theme.surface
                onAccepted: o.go()
                Component.onCompleted: input.forceActiveFocus()
            }
        }
        Fold {
            open: App.hwidRetry !== "" && !App.busy
            Item { width: 1; height: 8 }
            Btn {
                kind: "tonal"
                text: Ui.tr("Включить HWID и повторить", "Turn on HWID and retry")
                onClicked: App.retryWithHwid()
            }
        }
        Item { width: 1; height: 16 }
        Beat {
            order: 3
            live: o.shown
            Btn {
                width: parent.width
                size: 56
                text: App.busy ? "" : Ui.tr("Продолжить", "Continue")
                busy: App.busy
                enabled: field.text.trim() !== "" || App.busy
                onClicked: o.go()
            }
        }
        Item { width: 1; height: 8; visible: App.subs.length > 0 }
        Beat {
            order: 4
            live: o.shown
            visible: App.subs.length > 0
            Btn {
                anchors.horizontalCenter: parent.horizontalCenter
                kind: "text"
                text: Ui.tr("Отмена", "Cancel")
                onClicked: App.adding = false
            }
        }
    }
}
