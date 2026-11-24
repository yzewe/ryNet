import QtQuick
import QtQuick.Window

Item {
    id: page

    readonly property var live: App.phase === 2 ? App.connectedServer : ({})
    readonly property string profile: live.remarks !== undefined ? live.remarks : App.settings.profile || ""
    readonly property string sub: live.remarks !== undefined ? live.sub : App.currentSub
    readonly property var server: {
        if (App.rev < 0 || profile === "")
            return ({})
        const own = sub === App.currentSub ? App.server(profile) : ({})
        return own.remarks !== undefined || live.remarks === undefined ? own : live
    }
    readonly property bool hasServer: server.remarks !== undefined
    readonly property bool statsShown: App.phase === 2 && !page.unseen && Window.window !== null && Window.window.visible && Window.window.visibility !== Window.Minimized
    onStatsShownChanged: App.setStats(statsShown)
    Component.onDestruction: App.setStats(false)
    property string hiddenAlert

    function enter() {
        const d = parent && parent.dir !== undefined ? parent.dir : 1
        alertIn.play(d)
        orbIn.play(d)
        statusIn.play(d)
        bottom.play(d)
        listIn.play(d)
    }
    property bool unseen: false
    Connections {
        target: page.parent
        function onOpacityChanged() {
            if (page.parent.opacity < 0.01) {
                page.unseen = true
            } else if (page.unseen) {
                page.unseen = false
                page.enter()
            }
        }
    }
    Component.onCompleted: {
        unseen = parent !== null && parent.opacity < 0.01
        if (!unseen)
            enter()
        App.setStats(statsShown)
    }

    Panes {
        Pane {
            anchors.fill: parent

            Item {
                id: body
                anchors.fill: parent
                readonly property bool wide: Ui.split
                readonly property real leftWidth: Math.min(420, Math.max(320, width * 0.4))

                Item {
                    id: hero
                    width: body.wide ? body.leftWidth : body.width
                    height: body.wide ? body.height : nPick.y - 8
                }
                Item {
                    id: nPick
                    visible: !body.wide
                    width: parent.width
                    height: bottom.height
                    y: parent.height - height - (Ui.compact ? 16 : 0)
                }

                Appear {
                    id: listIn
                    index: 3
                    lift: 28
                    visible: body.wide
                    x: body.leftWidth + 24
                    width: body.width - x
                    height: body.height

                    Loader {
                        active: body.wide
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.bottom: wPick.top
                        anchors.bottomMargin: 12
                        sourceComponent: ServersPage { embedded: true }
                    }
                    Item {
                        id: wPick
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: bottom.height
                    }
                }
            }

            Item {
                id: column
                parent: hero
                width: parent ? Math.min(640, parent.width) : 0
                height: parent ? parent.height : 0
                anchors.horizontalCenter: parent ? parent.horizontalCenter : undefined

                Appear {
                    id: alertIn
                    index: 0
                    y: 8
                    width: parent.width
                    height: alert.height

                    Rectangle {
                        id: alert
                        readonly property bool shown: App.expiryAlert !== "" && App.expiryAlert !== page.hiddenAlert
                        property real open: 0
                        width: parent.width
                        height: 48 * Math.max(0, Math.min(1, open))
                        radius: M.lg
                        color: Theme.surfaceContainerHigh
                        clip: true
                        opacity: Math.max(0, Math.min(1, open * 1.5 - 0.5))
                        visible: open > 0.01
                        Spring on open {
                            to: alert.shown ? 1 : 0
                            stiffness: alert.shown ? M.spatialK : M.effectsK
                            damping: alert.shown ? 0.8 : 1
                        }
                        Icon {
                            x: 16
                            anchors.verticalCenter: parent.verticalCenter
                            name: "timer"
                            size: 20
                        }
                        T {
                            x: 48
                            width: parent.width - 48 - 52
                            anchors.verticalCenter: parent.verticalCenter
                            text: App.expiryAlert
                            type: "bodyMedium"
                        }
                        IconBtn {
                            anchors.right: parent.right
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                            icon: "close"
                            iconSize: 20
                            tip: Ui.tr("Скрыть", "Hide")
                            onClicked: page.hiddenAlert = App.expiryAlert
                        }
                    }
                }

                Item {
                    id: stage
                    anchors.top: alertIn.bottom
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 8
                    width: parent.width
                    readonly property real orb: Math.max(84, Math.min(260, width * 0.5, height * 0.45, (height - words.room - modeBox.height - 4) / orb.field))

                    Column {
                        anchors.centerIn: parent
                        spacing: 4

                        Appear {
                            id: orbIn
                            index: 1
                            from: 0.86
                            lift: 12
                            anchors.horizontalCenter: parent.horizontalCenter
                            ConnectOrb {
                                id: orb
                                width: stage.orb * field
                                height: width
                                status: App.status
                                pressed: orbArea.pressed
                                ink: shown === 2 ? Theme.onPrimary : shown === 3 ? Theme.onErrorContainer : Theme.onPrimaryContainer
                                container: shown === 2 ? Theme.primary : shown === 3 ? Theme.errorContainer : Theme.primaryContainer
                                Behavior on ink { enabled: !Theme.fading; ColorAnimation { duration: 140; easing.type: Easing.BezierSpline; easing.bezierCurve: M.standard } }
                                Behavior on container { enabled: !Theme.fading; ColorAnimation { duration: 320; easing.type: Easing.BezierSpline; easing.bezierCurve: M.standard } }
                                MouseArea {
                                    id: orbArea
                                    anchors.centerIn: parent
                                    width: stage.orb
                                    height: stage.orb
                                    cursorShape: Qt.PointingHandCursor
                                    activeFocusOnTab: true
                                    onClicked: App.toggle()
                                    Keys.onPressed: (e) => {
                                        if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Space) {
                                            App.toggle()
                                            e.accepted = true
                                        }
                                    }
                                    Accessible.role: Accessible.Button
                                    Accessible.name: App.phase === 2 || App.phase === 1 || App.blocked ? Ui.tr("Отключить", "Disconnect") : Ui.tr("Подключить", "Connect")
                                    Accessible.onPressAction: App.toggle()
                                    StateLayer {
                                        id: orbRing
                                        property real corner: 0
                                        anchors.fill: parent
                                        anchors.margins: -inset
                                        radii: Qt.vector4d(corner, corner, corner, corner)
                                        tint: "transparent"
                                        focusRing: true
                                        focused: orbArea.activeFocus
                                        Spring on corner {
                                            to: orb.shown === 3 ? orbArea.width * 0.32 : orbArea.width / 2
                                            stiffness: M.fastK
                                            damping: 0.8
                                        }
                                    }
                                }
                            }
                        }
                        Appear {
                            id: statusIn
                            index: 2
                            lift: 20
                            anchors.horizontalCenter: parent.horizontalCenter
                            Status {
                                id: words
                                width: stage.width
                                status: orb.shown
                                cap: stage.height - 84 * orb.field - 4 - modeBox.height
                            }
                        }
                        Item {
                            id: modeBox
                            width: stage.width
                            height: mode.height + 16 + totals.height + 16
                            Choice {
                                id: mode
                                y: 16
                                width: Math.min(parent.width, 340)
                                anchors.horizontalCenter: parent.horizontalCenter
                                options: ["tun", "proxy"]
                                labels: [Ui.tr("Весь трафик", "All traffic"), Ui.tr("Прокси", "Proxy")]
                                selected: App.settings.connMode
                                onPicked: (v) => App.set("connMode", v, true)
                            }
                            Row {
                                id: totals
                                y: mode.y + mode.height + 16
                                anchors.horizontalCenter: parent.horizontalCenter
                                spacing: 8
                                opacity: App.phase === 2 ? 1 : 0
                                Behavior on opacity { NumberAnimation { duration: M.effectsMs } }
                                Chip {
                                    icon: "arrow_downward"
                                    text: App.bytes(App.download)
                                    interactive: false
                                }
                                Chip {
                                    icon: "arrow_upward"
                                    text: App.bytes(App.upload)
                                    interactive: false
                                }
                            }
                        }
                    }
                }
            }

            Appear {
                id: bottom
                index: 3
                lift: 28
                parent: body.wide ? wPick : nPick
                width: parent ? parent.width : 0
                height: chain.height + card.height

                ChainLink {
                    id: chain
                    width: parent.width
                }
                ServerCard {
                    id: card
                    anchors.top: chain.bottom
                    width: parent.width
                    server: page.server
                    sub: page.sub
                    auto: App.settings.autoSelect === true
                    onClicked: auto ? why.show() : App.nav = "servers"
                }
            }
        }
    }

    AutoWhyDialog { id: why }
}
