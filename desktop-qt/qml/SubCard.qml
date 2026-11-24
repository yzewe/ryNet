import QtQuick

Surface {
    id: card

    property var sub: ({})
    property string line
    readonly property bool current: sub.id === App.currentSub
    readonly property real usedPart: sub.total > 0 ? Math.min(1, sub.used / sub.total) : 0
    property real fill: 0
    property real picked: 0
    signal remove()
    signal edit()
    signal qr()

    function refill() {
        fillSpring.animateFrom(0)
    }

    implicitHeight: column.implicitHeight + 40
    radius: M.xl
    interactive: true
    squish: true
    squishScale: 0.98
    color: current ? Theme.secondaryContainer : Theme.surfaceContainerHigh
    tint: current ? Theme.onSecondaryContainer : Theme.onSurface
    Accessible.name: sub.name || ""
    Accessible.description: current ? Ui.tr("Текущая", "Current") : ""
    onClicked: {
        App.selectSub(sub.id)
        App.nav = "servers"
    }

    Spring on fill { id: fillSpring; to: card.usedPart; stiffness: M.slowK; damping: 0.9 }
    Spring on picked { to: card.current ? 1 : 0; stiffness: M.fastK; damping: 0.6 }

    Shadow {
        z: -1
        anchors.fill: parent
        radius: card.radius
        level: 1
        opacity: card.hovered ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: M.effectsMs; easing.type: Easing.OutCubic } }
    }

    Column {
        id: column
        x: 20
        y: 20
        width: parent.width - 40
        spacing: 0
        Item {
            width: parent.width
            height: 40
            Item {
                id: avatar
                width: 40
                height: 40
                MShape {
                    anchors.fill: parent
                    shape: "Circle"
                    morphTo: "Cookie9Sided"
                    progress: Math.max(0, Math.min(1, card.picked))
                    rotation: card.picked > 0.999 ? 0 : 40 * card.picked
                    ColorTo on color { to: card.current ? Theme.primary : Theme.primaryContainer }
                }
                T {
                    anchors.centerIn: parent
                    text: card.sub.letter || "•"
                    type: "titleMediumE"
                    ColorTo on color { to: card.current ? Theme.onPrimary : Theme.onPrimaryContainer }
                }
            }
            Column {
                anchors.left: avatar.right
                anchors.leftMargin: 16
                anchors.right: check.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                T {
                    width: parent.width
                    text: card.sub.name || ""
                    type: "titleMediumE"
                    color: card.current ? Theme.onSecondaryContainer : Theme.onSurface
                }
                T {
                    width: parent.width
                    text: card.sub.host || ""
                    type: "bodyMedium"
                    color: card.current ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
                }
            }
            Icon {
                id: check
                anchors.right: editBtn.left
                anchors.rightMargin: 4
                anchors.verticalCenter: parent.verticalCenter
                visible: card.picked > 0.02
                Accessible.role: Accessible.Graphic
                Accessible.name: Ui.tr("Текущая", "Current")
                name: "check"
                color: Theme.onSecondaryContainer
                opacity: Math.min(1, card.picked * 1.5)
                scale: Math.max(0, card.picked)
                rotation: -45 * (1 - Math.min(1, card.picked))
            }
            IconBtn {
                id: editBtn
                anchors.right: parent.right
                anchors.rightMargin: -8
                anchors.verticalCenter: parent.verticalCenter
                icon: "edit"
                iconSize: 20
                tip: Ui.tr("Изменить", "Edit")
                iconColor: card.current ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
                onClicked: card.edit()
            }
        }
        Item {
            visible: card.sub.total > 0
            width: parent.width
            height: 34
            WavyProgress {
                y: 16
                width: parent.width
                height: 10
                amplitude: 2
                moving: false
                value: Math.max(0, card.fill)
                color: Theme.primary
                trackColor: card.current ? Theme.mix(Theme.secondaryContainer, Theme.onSecondaryContainer, 0.18) : Theme.secondaryContainer
            }
        }
        T {
            visible: text !== ""
            width: parent.width
            topPadding: 12
            text: card.sub.facts || ""
            type: "bodyMedium"
            color: card.current ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
            wrapMode: Text.Wrap
        }
        Item {
            id: noteBox
            readonly property bool on: (card.sub.announce || "") !== "" && !(App.settings.dismissedNotices, App.noticeHidden(card.sub.id || "", card.sub.announce || ""))
            property real open: on ? 1 : 0
            Spring on open { to: noteBox.on ? 1 : 0; stiffness: M.spatialK; damping: 1 }
            visible: open > 0.01
            clip: true
            opacity: Math.min(1, open)
            width: parent.width
            height: (note.height + 12) * Math.max(0, Math.min(1, open))
            Notice {
                id: note
                y: 12
                width: parent.width
                text: card.sub.announce || ""
                closable: true
                onClosed: App.dismissNotice(card.sub.id || "", card.sub.announce || "")
                ColorTo on color { to: card.current ? Theme.mix(Theme.secondaryContainer, Theme.onSecondaryContainer, 0.12) : Theme.secondaryContainer }
            }
        }
        Item { width: 1; height: 16 }
        Flow {
            width: parent.width
            spacing: 8
            Btn {
                visible: card.sub.local !== true
                kind: card.current ? "filled" : "tonal"
                text: Ui.tr("Обновить", "Update")
                enabled: !App.busy
                onClicked: App.refresh(card.sub.id)
            }
            Btn {
                visible: (card.sub.supportUrl || "") !== ""
                kind: "outlined"
                borderColor: card.current ? Theme.outline : Theme.outlineVariant
                text: Ui.tr("Поддержка", "Support")
                onClicked: App.open(card.sub.supportUrl)
            }
            Btn {
                visible: (card.sub.webPage || "") !== ""
                kind: "outlined"
                borderColor: card.current ? Theme.outline : Theme.outlineVariant
                text: Ui.tr("Сайт", "Site")
                onClicked: App.open(card.sub.webPage)
            }
            Btn {
                visible: card.sub.local !== true
                kind: "outlined"
                borderColor: card.current ? Theme.outline : Theme.outlineVariant
                text: Ui.tr("QR-код", "QR code")
                onClicked: card.qr()
            }
            Btn {
                kind: "danger"
                text: Ui.tr("Удалить", "Delete")
                onClicked: card.remove()
            }
        }
    }
}
