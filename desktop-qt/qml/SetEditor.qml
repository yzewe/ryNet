import QtQuick

Item {
    id: ed

    property string setId
    property var draft: ({})
    readonly property var saved: (App.rev < 0 ? undefined : setId !== "" ? App.ruleSet(setId) : ({}))
    readonly property bool isSaved: saved.id !== undefined
    readonly property var set: isSaved ? saved : draft
    property int epoch: 0
    readonly property int iconIndex: set.icon || 0
    property bool choosingServer: false
    readonly property string action: choosingServer ? "server" : (set.action || "proxy")
    signal back()
    signal adding(string key)

    onIconIndexChanged: glyph.play()

    function start(id) {
        epoch++
        if (id === "") {
            draft = App.newSet()
            setId = draft.id
        } else {
            draft = ({})
            setId = id
        }
        name.text = set.name || ""
        choosingServer = false
    }
    function update(next) {
        if (isSaved || next.size > 0 || (next.name || "").trim() !== "" || entriesOf(next) > 0)
            App.saveSet(next)
        else
            draft = next
    }
    function entriesOf(s) {
        return (s.domains || []).length + (s.procs || []).length + (s.geosite || []).length + (s.ips || []).length
    }
    function with_(key, value) {
        const n = Object.assign({}, set)
        n[key] = value
        return n
    }

    Pane {
        anchors.fill: parent
        title: (ed.set.name || "") !== "" ? ed.set.name : Ui.tr("Новый набор", "New set")
        actions: [
            IconBtn {
                icon: "arrow_back"
                tip: Ui.tr("Назад", "Back")
                onClicked: ed.back()
            },
            IconBtn {
                visible: ed.entriesOf(ed.set) > 0
                icon: "share"
                tip: Ui.tr("Поделиться", "Share")
                onClicked: share.shareSet(ed.set)
            },
            IconBtn {
                visible: ed.isSaved
                icon: "delete"
                tip: Ui.tr("Удалить", "Delete")
                onClicked: confirm.show()
            }
        ]

        Scroll {
            anchors.fill: parent
            anchors.rightMargin: -14
            bottomPadding: 48
            Column {
                width: Math.min(820, parent.width - 14)
                spacing: 2

                Rectangle {
                    width: parent.width
                    height: look.implicitHeight + 40
                    color: Theme.surfaceContainerHigh
                    topLeftRadius: 24
                    topRightRadius: 24
                    bottomLeftRadius: 6
                    bottomRightRadius: 6
                    Column {
                        id: look
                        x: 20
                        y: 20
                        width: parent.width - 40
                        spacing: 16
                        Row {
                            width: parent.width
                            spacing: 16
                            ShapeIcon {
                                id: glyph
                                size: 64
                                icon: SetLook.icon(ed.set.icon || 0)
                                shape: SetLook.shape(ed.set.icon || 0)
                            }
                            Field {
                                id: name
                                width: parent.width - 80
                                anchors.verticalCenter: parent.verticalCenter
                                label: Ui.tr("Название", "Name")
                                bg: Theme.surfaceContainerHigh
                                input.maximumLength: 40
                                onTextChanged: if (text.trim() !== (ed.set.name || "")) ed.update(ed.with_("name", text.trim()))
                            }
                        }
                        Flow {
                            width: parent.width
                            spacing: 8
                            Repeater {
                                model: SetLook.icons.length
                                Surface {
                                    id: pick
                                    required property int index
                                    readonly property bool on: index === ((ed.set.icon || 0) % SetLook.icons.length)
                                    width: 44
                                    height: 44
                                    radius: 22
                                    interactive: true
                                    squish: true
                                    squishScale: 0.86
                                    onClicked: ed.update(ed.with_("icon", index))
                                    onOnChanged: if (on) option.play()
                                    ShapeIcon {
                                        id: option
                                        anchors.centerIn: parent
                                        icon: SetLook.icons[pick.index]
                                        shape: pick.on ? SetLook.shape(pick.index) : "Circle"
                                        container: pick.on ? Theme.primaryContainer : "transparent"
                                        color: pick.on ? Theme.onPrimaryContainer : Theme.onSurfaceVariant
                                    }
                                }
                            }
                        }
                    }
                }

                Rectangle {
                    width: parent.width
                    height: where.implicitHeight + 40
                    topLeftRadius: 6
                    topRightRadius: 6
                    bottomLeftRadius: 24
                    bottomRightRadius: 24
                    color: Theme.surfaceContainerHigh
                    Column {
                        id: where
                        x: 20
                        y: 20
                        width: parent.width - 40
                        spacing: 0
                        T { text: Ui.tr("Куда", "Where"); type: "titleMediumE"; font.pixelSize: 15 }
                        Item { width: 1; height: 12 }
                        Choice {
                            width: parent.width
                            options: ["proxy", "direct", "block", "server"]
                            labels: [Ui.tr("Прокси", "Proxy"), Ui.tr("Напрямую", "Direct"), Ui.tr("Блок", "Block"), Ui.tr("Сервер", "Server")]
                            selected: ed.action
                            onPicked: (v) => {
                                ed.choosingServer = v === "server" && !ed.set.profile
                                if (ed.choosingServer)
                                    return
                                const n = ed.with_("action", v)
                                if (v !== "server") {
                                    n.sub = ""
                                    n.profile = ""
                                }
                                ed.update(n)
                            }
                        }
                        Item {
                            id: pickerBox
                            property real h: 0
                            width: parent.width
                            height: h
                            clip: unfold.running
                            Spring on h {
                                id: unfold
                                to: picker.item ? picker.item.height + 12 : 0
                                stiffness: M.spatialK
                                damping: 0.86
                            }
                            Loader {
                                id: picker
                                y: 12
                                width: parent.width
                                active: ed.action === "server"
                                opacity: pickerBox.h > 12 ? 1 : 0
                                Behavior on opacity { NumberAnimation { duration: M.effectsMs; easing.type: Easing.OutCubic } }
                                sourceComponent: ServerPicker {
                                    sub: ed.set.sub || App.currentSub
                                    profile: ed.set.profile || ""
                                    onPicked: (s, p) => {
                                        const n = ed.with_("sub", s)
                                        n.profile = p
                                        n.action = "server"
                                        ed.update(n)
                                        ed.choosingServer = false
                                    }
                                }
                            }
                        }
                    }
                }

                Repeater {
                    id: blocks
                    model: [
                        { key: "domains", title: Ui.tr("Сайты", "Sites"), icon: "language", add: Ui.tr("Сайт", "Site") },
                        { key: "procs", title: Ui.tr("Приложения", "Apps"), icon: "apps", add: Ui.tr("Приложение", "App") },
                        { key: "geosite", title: Ui.tr("Категории", "Categories"), icon: "list", add: Ui.tr("Категория", "Category") },
                        { key: "ips", title: Ui.tr("Сети", "Networks"), icon: "conversion_path", add: Ui.tr("Сеть", "Network") }
                    ]
                    Column {
                        id: block
                        required property var modelData
                        readonly property var items: ed.set[modelData.key] || []
                        property bool all: false
                        property bool gliding: false
                        function glide() {
                            gliding = true
                            settled.restart()
                        }
                        Connections {
                            target: ed
                            function onAdding(key) {
                                if (key === block.modelData.key)
                                    block.glide()
                            }
                        }
                        width: parent.width
                        KeyedRows {
                            id: shown
                            items: block.all ? block.items : block.items.slice(0, 40)
                            linger: 0
                            epoch: ed.epoch
                        }
                        Timer {
                            id: settled
                            interval: 700
                            onTriggered: block.gliding = false
                        }
                        Transition {
                            id: glideMove
                            NumberAnimation {
                                properties: "x,y"
                                duration: 340
                                easing.type: Easing.OutBack
                                easing.overshoot: 1.1
                            }
                        }
                        Header {
                            text: block.items.length > 0 ? block.modelData.title + " · " + block.items.length : block.modelData.title
                        }
                        Flow {
                            width: parent.width
                            spacing: 8
                            move: block.gliding ? glideMove : null
                            Repeater {
                                model: shown
                                Surface {
                                    id: entry
                                    required property string key
                                    required property bool fresh
                                    readonly property string value: key
                                    property real grow: 1
                                    property bool going: false
                                    height: 32
                                    width: Math.min(320, chipRow.implicitWidth + 16)
                                    radius: M.sm
                                    interactive: !going
                                    Accessible.name: geoKind !== "" ? value : Ui.tr("Убрать", "Remove") + " " + value
                                    color: "transparent"
                                    borderWidth: 1
                                    borderColor: Theme.outlineVariant
                                    tint: Theme.onSurfaceVariant
                                    transform: Scale {
                                        origin.x: entry.width / 2
                                        origin.y: entry.height / 2
                                        xScale: entry.grow
                                        yScale: entry.grow
                                    }
                                    Spring on grow { id: growSpring; to: 1; stiffness: 520; damping: 0.62 }
                                    NumberAnimation {
                                        id: fade
                                        target: entry
                                        property: "opacity"
                                        duration: 150
                                        easing.type: Easing.BezierSpline
                                    }
                                    Timer {
                                        id: gone
                                        interval: 140
                                        onTriggered: ed.update(ed.with_(block.modelData.key, Ui.without(block.items, entry.value)))
                                    }
                                    readonly property string geoKind: block.modelData.key === "geosite" ? "geosite"
                                        : block.modelData.key === "ips" && value.toLowerCase().startsWith("geoip:") ? "geoip" : ""
                                    onClicked: {
                                        if (geoKind !== "")
                                            Qt.openUrlExternally(Ui.geoPage(geoKind, value.replace(/^geo(site|ip):/i, "")))
                                        else
                                            remove()
                                    }
                                    function remove() {
                                        block.glide()
                                        going = true
                                        growSpring.stiffness = M.fastK
                                        growSpring.damping = 1
                                        growSpring.to = 0.6
                                        fade.to = 0
                                        fade.easing.bezierCurve = M.accelerate
                                        fade.restart()
                                        gone.start()
                                    }
                                    Component.onCompleted: {
                                        if (fresh) {
                                            opacity = 0
                                            growSpring.animateFrom(0.6)
                                            fade.to = 1
                                            fade.easing.bezierCurve = M.decelerate
                                            fade.restart()
                                        }
                                    }
                                    Row {
                                        id: chipRow
                                        x: 8
                                        anchors.verticalCenter: parent.verticalCenter
                                        spacing: 8
                                        Icon { name: block.modelData.icon; size: 18; anchors.verticalCenter: parent.verticalCenter }
                                        T { text: entry.value; type: "labelLarge"; color: Theme.onSurfaceVariant; width: Math.min(implicitWidth, 252); elide: Text.ElideMiddle; anchors.verticalCenter: parent.verticalCenter }
                                        Icon {
                                            name: "close"
                                            size: 18
                                            anchors.verticalCenter: parent.verticalCenter
                                            MouseArea {
                                                enabled: entry.geoKind !== "" && !entry.going
                                                anchors.fill: parent
                                                anchors.margins: -7
                                                cursorShape: Qt.PointingHandCursor
                                                Accessible.role: Accessible.Button
                                                Accessible.name: Ui.tr("Убрать", "Remove") + " " + entry.value
                                                onClicked: entry.remove()
                                            }
                                        }
                                    }
                                }
                            }
                            Chip {
                                visible: !block.all && block.items.length > 40
                                text: Ui.tr("Ещё %1", "%1 more").arg(block.items.length - 40)
                                selected: true
                                onClicked: block.all = true
                            }
                            Chip {
                                icon: "add"
                                text: block.modelData.add
                                color: Theme.secondaryContainer
                                borderWidth: 0
                                onClicked: entries.ask(block.modelData.key)
                            }
                        }
                    }
                }
            }
        }
    }

    EntriesDialog {
        id: entries
        onAdded: (list) => {
            const key = entries.kind
            ed.adding(key)
            ed.update(ed.with_(key, Ui.merged(ed.set[key] || [], list)))
        }
    }
    ShareSetDialog { id: share }
    ConfirmDialog {
        id: confirm
        title: Ui.tr("Удалить набор?", "Delete the set?")
        text: SetLook.name(ed.set)
        onAccepted: {
            App.deleteSet(ed.setId)
            ed.back()
        }
    }
}
