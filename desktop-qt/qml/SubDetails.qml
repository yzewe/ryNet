import QtQuick

Item {
    id: d

    property var sub: ({})
    readonly property bool has: sub.id !== undefined
    readonly property bool current: sub.id === App.currentSub
    readonly property int days: sub.expire > 0 ? Math.floor((sub.expire - Date.now() / 1000) / 86400) : 0
    readonly property real usedPart: sub.total > 0 ? Math.min(1, sub.used / sub.total) : 0
    property string shownId
    property real fill: 0
    signal remove()
    signal edit()
    signal qr()

    function enter(delay) {
        infoLift.play(delay, 24)
        actionsLift.play(delay + 70, 24)
    }
    onSubChanged: {
        if ((sub.id || "") === shownId)
            return
        shownId = sub.id || ""
        infoLift.play(0, 16)
        actionsLift.play(50, 16)
    }
    Component.onCompleted: shownId = sub.id || ""

    Spring on fill { id: fillSpring; to: d.usedPart; stiffness: M.slowK; damping: 0.9 }

    component Lift: Item {
        id: lift
        property Item target
        property real dy: 0
        signal started()
        function play(delay, from) {
            wait.stop()
            fade.stop()
            target.opacity = 0
            rise.snap(from)
            wait.interval = Math.max(1, delay)
            wait.restart()
        }
        visible: false
        Spring on dy { id: rise; to: 0; stiffness: M.spatialK; damping: M.spatialZ }
        NumberAnimation {
            id: fade
            target: lift.target
            property: "opacity"
            to: 1
            duration: 240
            easing.type: Easing.BezierSpline
            easing.bezierCurve: M.decelerate
        }
        Timer {
            id: wait
            onTriggered: {
                rise.to = 0
                fade.restart()
                lift.started()
            }
        }
    }
    Lift {
        id: infoLift
        target: info
        onStarted: fillSpring.animateFrom(0)
    }
    Lift {
        id: actionsLift
        target: actions
    }

    Column {
        id: info
        visible: d.has
        width: parent.width
        spacing: 0
        transform: Translate { y: infoLift.dy }
        Item {
            width: parent.width
            height: name.height
            T {
                id: name
                width: parent.width - 56
                text: d.sub.name || ""
                type: "displaySmallE"
                wrapMode: Text.Wrap
                maximumLineCount: 2
            }
            IconBtn {
                anchors.right: parent.right
                y: Math.max(0, (Math.min(name.height, 48) - height) / 2)
                icon: "edit"
                tip: Ui.tr("Изменить", "Edit")
                onClicked: d.edit()
            }
        }
        T {
            width: parent.width
            text: Ui.joined([d.sub.host || "", d.sub.updated > 0 ? Ui.tr("обновлена ", "updated ") + App.ago(Date.now() / 1000 - d.sub.updated) : ""], " · ")
            type: "bodyLarge"
            color: Theme.onSurfaceVariant
        }
        Item { width: 1; height: 28 }
        Row {
            spacing: 8
            T {
                id: usedText
                text: App.bytes(d.sub.used || 0)
                type: "headlineLarge"
                font.weight: 600
            }
            T {
                anchors.baseline: usedText.baseline
                text: d.sub.total > 0 ? Ui.tr("из %1", "of %1").arg(App.bytes(d.sub.total)) : Ui.tr("без лимита", "unlimited")
                type: "titleMedium"
                color: Theme.onSurfaceVariant
            }
        }
        Item {
            visible: d.sub.total > 0
            width: parent.width
            height: 28
            WavyProgress {
                y: 12
                width: parent.width
                height: 12
                moving: false
                value: Math.max(0, d.fill)
                color: Theme.primary
                trackColor: Theme.secondaryContainer
            }
        }
        Item { width: 1; height: 16 }
        Flow {
            width: parent.width
            spacing: 8
            Rectangle {
                id: expChip
                readonly property bool alarm: d.days < 3
                visible: d.sub.expire > 0
                height: 32
                width: expRow.implicitWidth + 24
                radius: M.sm
                color: alarm ? Theme.errorContainer : "transparent"
                border.width: alarm ? 0 : 1
                border.color: Theme.outlineVariant
                Row {
                    id: expRow
                    x: 8
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 8
                    Icon {
                        name: "timer"
                        size: 18
                        color: expChip.alarm ? Theme.onErrorContainer : Theme.primary
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    T {
                        text: d.days < 0 ? Ui.tr("закончилась", "expired") : d.days === 0 ? Ui.tr("последний день", "last day") : Ui.tr("осталось %1 дн.", "%1 d left").arg(d.days)
                        type: "labelLarge"
                        color: expChip.alarm ? Theme.onErrorContainer : Theme.onSurface
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }
            Rectangle {
                visible: d.sub.intervalHours !== undefined && d.sub.intervalHours !== null
                height: 32
                width: intRow.implicitWidth + 24
                radius: M.sm
                color: "transparent"
                border.width: 1
                border.color: Theme.outlineVariant
                Row {
                    id: intRow
                    x: 8
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 8
                    Icon { name: "refresh"; size: 18; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
                    T {
                        text: Ui.tr("обновление раз в %1 ч", "updates every %1 h").arg(d.sub.intervalHours)
                        type: "labelLarge"
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }
        }
        Item {
            id: noteBox
            readonly property bool on: (d.sub.announce || "") !== "" && !(App.settings.dismissedNotices, App.noticeHidden(d.sub.id || "", d.sub.announce || ""))
            property real open: on ? 1 : 0
            Spring on open { to: noteBox.on ? 1 : 0; stiffness: M.spatialK; damping: 1 }
            visible: open > 0.01
            clip: true
            opacity: Math.min(1, open)
            width: parent.width
            height: (note.height + 24) * Math.max(0, Math.min(1, open))
            Notice {
                id: note
                y: 24
                width: parent.width
                text: d.sub.announce || ""
                closable: true
                onClosed: App.dismissNotice(d.sub.id || "", d.sub.announce || "")
            }
        }
    }

    Flow {
        id: actions
        property bool gliding: false
        visible: d.has
        anchors.bottom: parent.bottom
        width: parent.width
        spacing: 8
        transform: Translate { y: actionsLift.dy }
        move: gliding ? glideMove : null
        Transition {
            id: glideMove
            NumberAnimation {
                properties: "x,y"
                duration: 380
                easing.type: Easing.OutBack
                easing.overshoot: 1.2
            }
        }
        Timer {
            id: settled
            interval: 600
            onTriggered: actions.gliding = false
        }
        Btn {
            visible: !d.current
            size: 56
            text: Ui.tr("Выбрать", "Select")
            onClicked: {
                actions.gliding = true
                settled.restart()
                App.selectSub(d.sub.id)
            }
        }
        Btn {
            visible: d.sub.local !== true
            size: 56
            kind: d.current ? "filled" : "tonal"
            icon: "refresh"
            busy: App.busy
            text: Ui.tr("Обновить", "Update")
            onClicked: App.refresh(d.sub.id)
        }
        Btn {
            visible: (d.sub.supportUrl || "") !== ""
            size: 56
            kind: "tonal"
            icon: "support_agent"
            text: Ui.tr("Поддержка", "Support")
            onClicked: App.open(d.sub.supportUrl)
        }
        Btn {
            visible: (d.sub.webPage || "") !== ""
            size: 56
            kind: "tonal"
            icon: "public"
            text: Ui.tr("Сайт", "Site")
            onClicked: App.open(d.sub.webPage)
        }
        Btn {
            visible: d.sub.local !== true
            size: 56
            kind: "tonal"
            icon: "qr_code"
            text: Ui.tr("QR-код", "QR code")
            onClicked: d.qr()
        }
        Btn {
            size: 56
            kind: "danger"
            icon: "delete"
            text: Ui.tr("Удалить", "Delete")
            onClicked: d.remove()
        }
    }
}
