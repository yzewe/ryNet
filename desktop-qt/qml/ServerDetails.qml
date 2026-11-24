import QtQuick

Item {
    id: d

    property string remarks
    property string shown
    readonly property var s: (App.rev < 0 ? undefined : shown !== "" ? App.server(shown) : ({}))
    readonly property bool has: s.remarks !== undefined
    readonly property var ping: (App.rev < 0 ? undefined : has ? App.ping(shown) : ({}))
    readonly property bool waiting: has && (App.rev >= 0 && App.pingWaitingFor(shown))
    readonly property bool selected: shown === App.settings.profile
    readonly property bool connected: App.phase === 2 && App.connectedServer.sub === App.currentSub && App.connectedServer.remarks === shown
    readonly property bool connecting: App.phase === 1 && App.connectingServer.sub === App.currentSub && App.connectingServer.remarks === shown
    readonly property var rows: {
        if (!has)
            return []
        const out = []
        if (s.server) out.push({ k: Ui.tr("Адрес", "Address"), v: s.server, copy: s.server, icon: "content_copy", line: true })
        if (s.sni) out.push({ k: "SNI", v: s.sni, copy: s.link || "", icon: s.link ? "link" : "", line: true })
        if (s.fingerprint) out.push({ k: Ui.tr("Отпечаток", "Fingerprint"), v: s.fingerprint })
        const group = App.groupOf(shown)
        if (group) out.push({ k: Ui.tr("Группа", "Group"), v: group })
        return out
    }
    property real fade: 1
    property real zoom: 1
    property bool made: false
    signal dns(string remarks)

    Component.onCompleted: {
        shown = remarks
        made = true
    }
    onRemarksChanged: if (made) swap.restart()
    onWaitingChanged: if (!swap.running) waiting ? busy.come() : bigPing.bump()

    SequentialAnimation {
        id: swap
        NumberAnimation { target: d; property: "fade"; to: 0; duration: 90; easing.type: Easing.BezierSpline; easing.bezierCurve: M.accelerate }
        ScriptAction {
            script: {
                d.shown = d.remarks
                grow.animateFrom(0.95)
            }
        }
        NumberAnimation { target: d; property: "fade"; to: 1; duration: 210; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate }
    }
    Spring on zoom { id: grow; to: 1; stiffness: M.spatialK; damping: 0.75 }

    T {
        anchors.centerIn: parent
        visible: !d.has
        opacity: d.fade
        text: Ui.tr("Выберите сервер", "Choose a server")
        type: "bodyLarge"
        color: Theme.onSurfaceVariant
    }

    Item {
        anchors.fill: parent
        visible: d.has
        opacity: d.fade
        scale: d.zoom
        transformOrigin: Item.Top

        Column {
            id: top
            width: parent.width
            spacing: 20

            Row {
                width: parent.width
                spacing: 16
                Flag {
                    width: 72
                    height: 54
                    radius: M.md
                    flag: d.s.flag || ""
                }
                Column {
                    width: parent.width - 88 - (qrBtn.visible ? 56 : 0)
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    T {
                        width: parent.width
                        text: d.s.name || ""
                        type: "titleLargeE"
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                    }
                    T {
                        width: parent.width
                        text: Ui.joined([d.s.kind || "", d.s.description || ""], " · ")
                        type: "bodyMedium"
                        color: Theme.onSurfaceVariant
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                    }
                }
                IconBtn {
                    id: qrBtn
                    visible: (d.s.link || "") !== ""
                    anchors.verticalCenter: parent.verticalCenter
                    icon: "qr_code"
                    tip: Ui.tr("QR-код", "QR code")
                    onClicked: qrDialog.showLink(d.s.name || "", d.s.link)
                }
            }

            Column {
                visible: d.s.supported === true
                width: parent.width
                spacing: 12
                Row {
                    height: 48
                    spacing: 4
                    LoadingIndicator {
                        id: busy
                        property real pop: 1
                        function come() {
                            popper.animateFrom(0.4)
                        }
                        visible: d.waiting
                        running: visible
                        width: 40
                        height: 40
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.primary
                        scale: pop
                        Spring on pop { id: popper; to: 1; stiffness: M.fastK; damping: 0.7 }
                    }
                    PingText {
                        id: bigPing
                        visible: !d.waiting
                        anchors.verticalCenter: parent.verticalCenter
                        ping: d.ping
                        type: "displaySmallE"
                        shade: d.ping.ms === undefined && d.ping.error === undefined ? Theme.outline : Ui.pingColor(ms)
                    }
                    T {
                        visible: !d.waiting && d.ping.ms !== undefined
                        text: Ui.tr(" мс", " ms")
                        type: "titleMedium"
                        color: Theme.onSurfaceVariant
                        anchors.baseline: bigPing.baseline
                    }
                }
                Row {
                    width: parent.width
                    spacing: 8
                    Btn {
                        width: (parent.width - 8) / 2
                        kind: "tonal"
                        icon: "show_chart"
                        text: Ui.tr("Скорость", "Speed")
                        onClicked: App.openSpeed(d.shown)
                    }
                    Btn {
                        width: (parent.width - 8) / 2
                        kind: "tonal"
                        icon: "bolt"
                        text: Ui.tr("Проверить", "Check")
                        enabled: !d.waiting
                        onClicked: App.pingSome([d.shown].concat(Ui.pluck(d.s.members || [], "remarks")))
                    }
                }
            }

            Notice {
                readonly property string problem: d.s.supported === false
                    ? (d.s.reason ? (d.s.reason.indexOf("не поддерживается") >= 0 || d.s.reason.indexOf("not supported") >= 0 ? d.s.reason : Ui.tr("Не поддерживается: ", "Not supported: ") + d.s.reason) : Ui.tr("Не поддерживается", "Not supported"))
                    : (d.ping.error || "")
                visible: problem !== ""
                width: parent.width
                icon: "close"
                error: true
                text: problem
            }
        }

        Scroll {
            anchors.top: top.bottom
            anchors.topMargin: 20
            anchors.bottom: actions.top
            anchors.bottomMargin: 16
            width: parent.width

            Column {
                width: parent.width
                spacing: 2
                Repeater {
                    model: (d.s.members || []).length > 0 ? [] : d.rows
                    Rectangle {
                        required property var modelData
                        required property int index
                        width: parent.width
                        height: Math.max(48, value.implicitHeight + 24)
                        color: Theme.surfaceContainer
                        topLeftRadius: index === 0 ? M.lgPlus : M.xs
                        topRightRadius: index === 0 ? M.lgPlus : M.xs
                        bottomLeftRadius: index === d.rows.length - 1 ? M.lgPlus : M.xs
                        bottomRightRadius: index === d.rows.length - 1 ? M.lgPlus : M.xs
                        T {
                            x: 16
                            width: 96
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.k
                            type: "labelLarge"
                            color: Theme.onSurfaceVariant
                        }
                        T {
                            id: value
                            x: 112
                            width: parent.width - 112 - (copy.visible ? 48 : 16)
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.v
                            type: "bodyMedium"
                            wrapMode: modelData.line ? Text.NoWrap : Text.WrapAnywhere
                            elide: modelData.line ? Text.ElideMiddle : Text.ElideRight
                            maximumLineCount: 2
                        }
                        IconBtn {
                            id: copy
                            visible: (modelData.copy || "") !== "" && (modelData.icon || "") !== ""
                            anchors.right: parent.right
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                            icon: modelData.icon || ""
                            iconSize: 20
                            tip: modelData.icon === "link" ? Ui.tr("Скопировать ссылку", "Copy link") : Ui.tr("Скопировать адрес", "Copy address")
                            onClicked: {
                                App.copy(modelData.copy)
                                App.notice = Ui.tr("Скопировано", "Copied")
                            }
                        }
                    }
                }
                Repeater {
                    model: d.s.members || []
                    Surface {
                        id: member
                        required property var modelData
                        required property int index
                        readonly property bool chosen: modelData.remarks === App.settings.profile
                        readonly property var mping: (App.rev < 0 ? undefined : App.ping(modelData.remarks))
                        readonly property bool mwaiting: (App.rev >= 0 && App.pingWaitingFor(modelData.remarks))
                        readonly property int total: (d.s.members || []).length
                        width: parent.width
                        height: 52
                        topLeftRadius: index === 0 ? M.lgPlus : M.xs
                        topRightRadius: index === 0 ? M.lgPlus : M.xs
                        bottomLeftRadius: index === total - 1 ? M.lgPlus : M.xs
                        bottomRightRadius: index === total - 1 ? M.lgPlus : M.xs
                        color: chosen ? Theme.secondaryContainer : Theme.surfaceContainer
                        tint: chosen ? Theme.onSecondaryContainer : Theme.onSurface
                        interactive: modelData.supported
                        opacity: modelData.supported ? 1 : 0.38
                        Accessible.name: modelData.name
                        Accessible.selected: chosen
                        onClicked: App.selectServer(modelData.remarks)
                        onMwaitingChanged: if (!mwaiting) mp.bump()
                        Flag {
                            id: mflag
                            x: 8
                            anchors.verticalCenter: parent.verticalCenter
                            width: 40
                            height: 30
                            radius: M.sm
                            flag: member.modelData.flag || ""
                        }
                        Column {
                            anchors.left: mflag.right
                            anchors.leftMargin: 12
                            anchors.right: mp.left
                            anchors.rightMargin: 8
                            anchors.verticalCenter: parent.verticalCenter
                            T {
                                width: parent.width
                                text: member.modelData.name
                                type: member.chosen ? "titleSmallE" : "titleSmall"
                                color: member.chosen ? Theme.onSecondaryContainer : Theme.onSurface
                            }
                            T {
                                width: parent.width
                                text: member.modelData.kind
                                type: "labelMedium"
                                color: member.chosen ? Theme.alpha(Theme.onSecondaryContainer, 0.72) : Theme.onSurfaceVariant
                            }
                        }
                        PingText {
                            id: mp
                            anchors.right: parent.right
                            anchors.rightMargin: 16
                            anchors.verticalCenter: parent.verticalCenter
                            visible: !member.mwaiting && (member.mping.ms !== undefined || member.mping.error !== undefined)
                            ping: member.mping
                        }
                        LoadingIndicator {
                            visible: member.mwaiting
                            running: visible
                            width: 24
                            height: 24
                            anchors.right: parent.right
                            anchors.rightMargin: 12
                            anchors.verticalCenter: parent.verticalCenter
                            color: member.chosen ? Theme.onSecondaryContainer : Theme.primary
                        }
                    }
                }
                Item {
                    width: 1
                    height: 10
                }
                Surface {
                    width: parent.width
                    height: 48
                    radius: M.lgPlus
                    color: Theme.surfaceContainer
                    interactive: true
                    onClicked: {
                        configDialog.active = true
                        configDialog.item.showFor(d.shown)
                    }
                    Icon {
                        x: 16
                        anchors.verticalCenter: parent.verticalCenter
                        name: "info"
                        size: 20
                        color: Theme.onSurfaceVariant
                    }
                    T {
                        x: 52
                        anchors.verticalCenter: parent.verticalCenter
                        text: Ui.tr("Подробности", "Details")
                        type: "labelLarge"
                    }
                    Icon {
                        anchors.right: parent.right
                        anchors.rightMargin: 12
                        anchors.verticalCenter: parent.verticalCenter
                        name: "chevron_right"
                        size: 20
                        color: Theme.onSurfaceVariant
                    }
                }
            }
        }

        Column {
            id: actions
            anchors.bottom: parent.bottom
            width: parent.width
            spacing: 8

            Btn {
                width: parent.width
                size: 56
                kind: d.connected ? "tonal" : "filled"
                icon: d.connected ? "power_settings_new" : ""
                text: d.connecting ? Ui.tr("Подключение…", "Connecting…") : d.connected ? Ui.tr("Отключиться", "Disconnect") : Ui.tr("Подключиться", "Connect")
                busy: d.connecting
                enabled: d.s.supported === true && d.s.placeholder !== true
                onClicked: d.connected ? App.disconnectVpn() : App.connectTo(d.shown)
            }
            Btn {
                visible: d.s.supported === true
                width: parent.width
                kind: "text"
                icon: "link"
                readonly property bool isEntry: App.settings.chainActive === true && App.settings.chainSub === App.currentSub
                    && App.settings.chainProfile === d.shown
                text: isEntry ? Ui.tr("Убрать цепочку", "Remove chain") : Ui.tr("Сделать входом цепочки", "Make chain entry")
                enabled: isEntry || !d.selected
                onClicked: App.setChainEntry(isEntry ? "" : d.shown)
            }
            Row {
                visible: d.s.supported === true
                width: parent.width
                spacing: 8
                Btn {
                    width: (parent.width - 8) / 2
                    kind: "text"
                    icon: "push_pin"
                    readonly property bool isPinned: (App.rev < 0 ? undefined : App.pinned(d.shown))
                    text: isPinned ? Ui.tr("Открепить", "Unpin") : Ui.tr("Закрепить", "Pin")
                    onClicked: App.pin(d.shown, !isPinned)
                }
                Btn {
                    width: (parent.width - 8) / 2
                    kind: "text"
                    icon: "language"
                    readonly property string value: (App.rev < 0 ? undefined : App.serverDns(d.shown))
                    text: Ui.dnsLabel(value)
                    onClicked: d.dns(d.shown)
                }
            }
        }
    }

    QrDialog { id: qrDialog }
    Loader {
        id: configDialog
        active: false
        sourceComponent: ConfigDialog {}
    }
}
