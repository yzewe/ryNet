import QtQuick

Item {
    id: page

    readonly property bool two: Ui.widthClass === 2 && width >= 900
    property string focusId
    readonly property var shown: App.currentSub !== undefined ? App.subOr(focusId) : ({})
    property var confirming: ({})
    property double enteredAt: 0
    ServerImportDialog { id: serverImport }

    function remove(sub) {
        confirming = sub
        confirm.show()
    }

    function appear() {
        enteredAt = Date.now()
        const flow = (two ? listLayout : gridLayout).item
        const k = flow ? flow.cascade(70, 45) : 0
        if (two && details.item)
            details.item.enter(90 + 45 * k)
    }

    property bool unseen: false
    Connections {
        target: page.parent
        function onOpacityChanged() {
            if (page.parent.opacity < 0.01) {
                page.unseen = true
            } else if (page.unseen) {
                page.unseen = false
                page.appear()
            }
        }
    }
    Component.onCompleted: {
        unseen = parent !== null && parent.opacity < 0.01
        if (!unseen)
            appear()
    }
    KeyedRows {
        id: rows
        items: App.subs
    }

    Panes {
        Pane {
            id: main
            width: page.two ? 380 : parent.width
            height: parent.height
            title: Ui.tr("Подписки", "Subscriptions")
            actions: [
                IconBtn {
                    icon: "arrow_back"
                    tip: Ui.tr("Назад", "Back")
                    onClicked: App.nav = "home"
                },
                IconBtn {
                    icon: "add"
                    tip: Ui.tr("Добавить сервер", "Add server")
                    onClicked: serverImport.show()
                },
                Item {
                    id: refresher
                    property real busy: 0
                    width: 40
                    height: 40
                    Spring on busy { to: App.busy ? 1 : 0; stiffness: M.fastK; damping: 0.8 }
                    Item {
                        anchors.fill: parent
                        visible: refresher.busy < 0.98
                        opacity: 1 - Math.min(1, refresher.busy)
                        scale: 1 - 0.5 * refresher.busy
                        rotation: 180 * refresher.busy
                        IconBtn {
                            anchors.fill: parent
                            enabled: !App.busy
                            icon: "refresh"
                            tip: Ui.tr("Обновить все", "Update all")
                            onClicked: App.refreshAll()
                        }
                    }
                    LoadingIndicator {
                        anchors.centerIn: parent
                        width: 32
                        height: 32
                        visible: refresher.busy > 0.02
                        running: visible
                        opacity: Math.min(1, refresher.busy)
                        scale: 0.5 + 0.5 * refresher.busy
                        color: Theme.primary
                    }
                }
            ]

            Surface {
                id: add
                width: parent.width
                height: 56
                radius: 28
                color: Theme.surfaceContainerHigh
                function go() {
                    if (input.text.trim() !== "" && !App.busy)
                        App.add(input.text.trim())
                }
                Connections {
                    target: App
                    function onAdded(link) {
                        if (input.text.trim() === link)
                            input.text = ""
                    }
                }
                HoverHandler { cursorShape: Qt.IBeamCursor }
                TapHandler { onTapped: input.forceActiveFocus() }
                StateLayer {
                    anchors.fill: parent
                    radii: Qt.vector4d(28, 28, 28, 28)
                    focused: input.activeFocus
                }
                Icon {
                    id: linkIcon
                    x: 16
                    anchors.verticalCenter: parent.verticalCenter
                    name: "link"
                }
                TextInput {
                    id: input
                    anchors.left: linkIcon.right
                    anchors.leftMargin: 12
                    anchors.right: addBtn.left
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    color: Theme.onSurface
                    font.family: M.font
                    font.pixelSize: 16
                    clip: true
                    selectByMouse: true
                    activeFocusOnTab: true
                    selectionColor: Theme.alpha(Theme.primary, 0.35)
                    Accessible.name: Ui.tr("Ссылка на подписку или сервер", "Subscription or server link")
                    onAccepted: add.go()
                }
                T {
                    anchors.left: input.left
                    anchors.right: input.right
                    anchors.verticalCenter: parent.verticalCenter
                    visible: input.text.length === 0
                    text: Ui.tr("Ссылка на подписку или сервер", "Subscription or server link")
                    type: "bodyLarge"
                    color: Theme.onSurfaceVariant
                }
                IconBtn {
                    id: addBtn
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    kind: "filled"
                    icon: "add"
                    iconSize: 20
                    tip: Ui.tr("Добавить", "Add")
                    enabled: input.text.trim() !== "" && !App.busy
                    onClicked: add.go()
                }
            }

            Fold {
                id: hwidFix
                anchors.top: add.bottom
                open: App.hwidRetry !== "" && !App.busy
                Item { width: 1; height: 8 }
                Btn {
                    kind: "tonal"
                    text: Ui.tr("Включить HWID и повторить", "Turn on HWID and retry")
                    onClicked: App.retryWithHwid()
                }
            }

            Scroll {
                visible: page.two
                anchors.top: hwidFix.bottom
                anchors.topMargin: 16
                anchors.bottom: parent.bottom
                width: parent.width
                Loader {
                    id: listLayout
                    active: page.two
                    width: parent.width
                    sourceComponent: CardFlow {
                        id: list
                        width: parent.width
                        gap: 2

                        Repeater {
                            model: rows
                            CardSlot {
                                id: entry
                                required item
                                required fresh
                                required leaving
                                required property int index
                                readonly property var sub: item
                                readonly property bool on: entry.sub.id === page.shown.id
                                readonly property bool current: entry.sub.id === App.currentSub
                                property real r: 0
                                property real picked: 0
                                order: index
                                cascadeFrom: page.unseen ? 0 : page.enteredAt
                                stamp: String(item.updated)
                                width: list.width
                                height: 72
                                Spring on r { to: entry.on ? 16 : 0; stiffness: M.fastK; damping: 0.7 }
                                Spring on picked { to: entry.current ? 1 : 0; stiffness: M.fastK; damping: 0.6 }

                                Surface {
                                    anchors.fill: parent
                                    interactive: true
                                    color: entry.on ? Theme.secondaryContainer : Theme.surfaceContainer
                                    tint: entry.on ? Theme.onSecondaryContainer : Theme.onSurface
                                    topLeftRadius: Math.max(entry.r, entry.first ? 20 : 4)
                                    topRightRadius: topLeftRadius
                                    bottomLeftRadius: Math.max(entry.r, entry.last ? 20 : 4)
                                    bottomRightRadius: bottomLeftRadius
                                    Accessible.name: entry.sub.name || ""
                                    Accessible.description: entry.current ? Ui.tr("Текущая", "Current") : ""
                                    Accessible.selected: entry.on
                                    onClicked: page.focusId = entry.sub.id
                                    Item {
                                        id: avatar
                                        x: 16
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: 40
                                        height: 40
                                        MShape {
                                            anchors.fill: parent
                                            shape: "Circle"
                                            morphTo: "Cookie9Sided"
                                            progress: Math.max(0, Math.min(1, entry.picked))
                                            rotation: entry.picked > 0.999 ? 0 : 40 * entry.picked
                                            ColorTo on color { to: entry.current ? Theme.primary : Theme.primaryContainer }
                                        }
                                        T {
                                            anchors.centerIn: parent
                                            text: entry.sub.letter || "•"
                                            type: "titleMediumE"
                                            ColorTo on color { to: entry.current ? Theme.onPrimary : Theme.onPrimaryContainer }
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
                                            text: entry.sub.name || ""
                                            type: "titleMediumE"
                                            color: entry.on ? Theme.onSecondaryContainer : Theme.onSurface
                                        }
                                        T {
                                            width: parent.width
                                            text: entry.sub.line || entry.sub.host || ""
                                            type: "bodyMedium"
                                            color: entry.on ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
                                        }
                                    }
                                    Icon {
                                        id: check
                                        anchors.right: parent.right
                                        anchors.rightMargin: 16
                                        anchors.verticalCenter: parent.verticalCenter
                                        visible: entry.picked > 0.02
                                        Accessible.role: Accessible.Graphic
                                        Accessible.name: Ui.tr("Текущая", "Current")
                                        name: "check"
                                        color: entry.on ? Theme.onSecondaryContainer : Theme.primary
                                        opacity: Math.min(1, entry.picked * 1.5)
                                        scale: Math.max(0, entry.picked)
                                        rotation: -45 * (1 - Math.min(1, entry.picked))
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Scroll {
                visible: !page.two
                anchors.top: hwidFix.bottom
                anchors.topMargin: 16
                anchors.bottom: parent.bottom
                width: parent.width
                bottomPadding: 24
                Loader {
                    id: gridLayout
                    active: !page.two
                    width: parent.width
                    sourceComponent: CardFlow {
                        id: grid
                        width: parent.width
                        cellMin: 360

                        Repeater {
                            model: rows
                            CardSlot {
                                id: slot
                                required item
                                required fresh
                                required leaving
                                required property int index
                                order: index
                                cascadeFrom: page.unseen ? 0 : page.enteredAt
                                stamp: String(item.updated)
                                width: grid.cellWidth
                                height: card.implicitHeight
                                onShownUp: card.refill()
                                onUpdated: card.refill()
                                SubCard {
                                    id: card
                                    width: parent.width
                                    sub: slot.item
                                    line: slot.item.line
                                    onRemove: page.remove(slot.item)
                                    onEdit: editing.edit(slot.item)
                                    onQr: qrDialog.showLink(slot.item.name || "", slot.item.url || "")
                                }
                            }
                        }
                    }
                }
            }
        }

        Pane {
            visible: page.two
            x: main.width + 24
            width: parent.width - main.width - 24
            height: parent.height
            Loader {
                id: details
                active: page.two
                anchors.fill: parent
                sourceComponent: SubDetails {
                    sub: page.shown
                    onRemove: page.remove(page.shown)
                    onEdit: editing.edit(page.shown)
                    onQr: qrDialog.showLink(page.shown.name || "", page.shown.url || "")
                }
            }
        }
    }

    SubEditDialog { id: editing }
    QrDialog { id: qrDialog }
    ConfirmDialog {
        id: confirm
        title: Ui.tr("Удалить подписку?", "Delete subscription?")
        text: page.confirming.name || ""
        onAccepted: App.remove(page.confirming.id)
    }
}
