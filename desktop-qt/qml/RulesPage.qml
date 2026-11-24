import QtQuick

Item {
    id: page

    readonly property var sets: (App.rev < 0 ? undefined : App.sets())
    readonly property var rules: (App.rev < 0 ? undefined : App.rules())
    readonly property var subscription: (App.rev < 0 ? undefined : App.subscriptionRules())
    readonly property string pinnedMode: App.settings.routingMode === "smart" ? "smart"
        : App.settings.routingMode === "subscription" && subscription.name !== undefined ? "subscription" : ""
    readonly property real pinnedHeight: pinnedMode !== "" ? headH + rowH : 0
    property string editing: "none"
    property string menuFor
    readonly property var menuSet: order.revision >= 0 ? order.set(menuFor) : ({})
    property int poppedRule: -1
    property double poppedAt: 0

    readonly property real rowH: 70
    readonly property real headH: 52

    Component.onCompleted: {
        unseen = parent !== null && parent.opacity < 0.01
        if (!unseen)
            appear()
    }
    property bool unseen: false
    Connections {
        target: page.parent
        function onOpacityChanged() {
            if (page.parent.opacity < 0.01) {
                page.unseen = true
            } else if (page.unseen) {
                page.unseen = false
                if (page.editing === "none")
                    page.appear()
            }
        }
    }

    KeyedRows {
        id: setRows
        items: page.sets
        ordered: false
    }
    SetOrder {
        id: order
        sets: page.sets
        top: page.pinnedHeight
        rowH: page.rowH
        headH: page.headH
        onReordered: (ids, on) => App.orderSets(ids, on)
    }

    property double enteredAt: 0
    function appear() {
        enteredAt = Date.now()
        Ui.cascadeDown(content, 0.8, enteredAt)
    }

    Panes {
        id: listPanes
        readonly property bool shown: page.editing === "none"
        property real dx: 0
        opacity: shown ? 1 : 0
        visible: opacity > 0.005
        transform: Translate { x: listPanes.dx }
        Behavior on opacity {
            NumberAnimation {
                duration: listPanes.shown ? 260 : 130
                easing.type: Easing.BezierSpline
                easing.bezierCurve: listPanes.shown ? M.decelerate : M.accelerate
            }
        }
        Spring on dx { to: listPanes.shown ? 0 : -40; stiffness: M.spatialK; damping: 0.86 }

        Pane {
            anchors.fill: parent
            title: Ui.tr("Правила", "Rules")
            actions: [
                IconBtn {
                    visible: Ui.compact
                    icon: "arrow_back"
                    tip: Ui.tr("Назад", "Back")
                    onClicked: App.nav = "settings"
                }
            ]

            Scroll {
                id: scroll
                anchors.fill: parent
                anchors.rightMargin: -14
                bottomPadding: 104
                Item {
                    id: content
                    width: Math.min(820, scroll.width - 14)
                    height: list.y + list.height

                    Column {
                        id: pinned
                        width: parent.width
                        visible: page.pinnedHeight > 0
                        Header { text: Ui.tr("Закреплённые", "Pinned") }
                        SettingRow {
                            readonly property bool smart: page.pinnedMode === "smart"
                            icon: "grid_view"
                            tailIcon: "push_pin"
                            title: smart ? Ui.tr("Режим Умный", "Smart mode") : page.subscription.name || ""
                            summary: smart ? Ui.tr("Российское и локальная сеть — напрямую", "Russian and local traffic goes direct")
                                : (page.subscription.entries || []).length === 0 ? Ui.tr("В подписке нет своих правил", "The subscription has no rules of its own")
                                : Ui.tr("Режим Подписка · ", "Subscription mode · ") + App.plural((page.subscription.entries || []).length, "запись", "записи", "записей", "entry", "entries")
                            faded: false
                            onActivated: modeDialog.view(page.pinnedMode)
                        }
                    }
                    Header {
                        y: page.pinnedHeight
                        visible: page.sets.length > 0
                        text: Ui.tr("Наборы", "Sets")
                    }
                    Item {
                        y: 0
                        width: parent.width
                        height: offHeader.height
                        opacity: order.offCount > 0 ? 1 : 0
                        visible: opacity > 0
                        Spring on y { to: order.offY; stiffness: M.spatialK; damping: 0.85 }
                        Behavior on opacity { NumberAnimation { duration: M.effectsMs; easing.type: Easing.OutCubic } }
                        Header {
                            id: offHeader
                            text: Ui.tr("Выключены", "Off")
                        }
                    }

                    Repeater {
                        model: setRows
                        Item {
                            id: setRow
                            required property string key
                            required property bool fresh
                            required property bool leaving
                            required property var item
                            readonly property var set: item
                            readonly property var spot: order.revision >= 0 ? order.spot(key) : ({})
                            readonly property bool on: spot.on
                            readonly property int pos: spot.pos
                            readonly property int count: spot.count
                            readonly property bool dragging: order.dragKey === key
                            property real lift: 0
                            property real grow: 1
                            property real leftAt: 0
                            property bool switching: false
                            property bool built: false
                            width: parent.width
                            height: page.rowH - 2
                            z: dragging ? 10 : switching ? 5 : leaving ? -1 : 0
                            onOnChanged: {
                                if (!built || dragging)
                                    return
                                switching = true
                                landed.restart()
                            }
                            y: 0
                            scale: (1 + 0.03 * lift) * grow
                            onLeavingChanged: {
                                if (!leaving) {
                                    growSpring.animateFrom(0.86)
                                    presence.to = 1
                                    presence.restart()
                                    return
                                }
                                leftAt = y
                                growSpring.to = 0.86
                                presence.to = 0
                                presence.restart()
                            }
                            Component.onCompleted: {
                                built = true
                                if (fresh) {
                                    opacity = 0
                                    growSpring.animateFrom(0.8)
                                    presence.restart()
                                } else if (Date.now() - page.enteredAt < 400) {
                                    Ui.cascadeAt(row, 0.8, spot.y, page.enteredAt)
                                }
                            }

                            Spring on y {
                                id: ySpring
                                onRunningChanged: if (!running && !landed.running) setRow.switching = false
                                to: setRow.leaving ? setRow.leftAt : setRow.dragging ? order.dragY : setRow.spot.y
                                stiffness: setRow.dragging ? 4000 : M.spatialK
                                damping: setRow.dragging ? 1 : 0.85
                            }
                            Spring on lift { to: setRow.dragging ? 1 : setRow.switching ? 0.5 : 0; stiffness: M.fastK; damping: 0.62 }
                            Timer {
                                id: landed
                                interval: 140
                                onTriggered: if (!ySpring.running) setRow.switching = false
                            }
                            Spring on grow { id: growSpring; to: 1; stiffness: 520; damping: 0.62 }
                            NumberAnimation {
                                id: presence
                                target: setRow
                                property: "opacity"
                                to: 1
                                duration: setRow.leaving ? 170 : 200
                                easing.type: Easing.BezierSpline
                                easing.bezierCurve: setRow.leaving ? M.accelerate : M.decelerate
                            }

                            Shadow {
                                anchors.fill: parent
                                radius: 24
                                level: 4
                                opacity: Math.max(0, Math.min(1, setRow.lift, row.opacity))
                                visible: opacity > 0.01
                            }
                            Item {
                                anchors.fill: parent
                                opacity: setRow.on || setRow.dragging || setRow.switching ? 1 : 0.55
                                Behavior on opacity { NumberAnimation { duration: M.effectsSlowMs; easing.type: Easing.OutCubic } }
                                SettingRow {
                                    id: row
                                    anchors.fill: parent
                                    iconStyle: "shape"
                                    icon: SetLook.icon(setRow.set.icon || 0)
                                    shape: SetLook.shape(setRow.set.icon || 0)
                                    title: SetLook.name(setRow.set)
                                    summary: SetLook.entries(setRow.set.size)
                                    groupFirst: setRow.pos === 0 || setRow.dragging
                                    groupLast: setRow.pos === setRow.count - 1 || setRow.dragging
                                    color: setRow.dragging ? Theme.mix(Theme.surfaceContainerHigh, Theme.onSurface, 0.08) : Theme.surfaceContainerHigh
                                    rightClicks: true
                                    onActivated: App.toggleSet(setRow.key)
                                    onRightClicked: (x, y) => {
                                        page.menuFor = setRow.key
                                        const p = mapToItem(Ui.overlay, x, y)
                                        setMenu.place(p.x, p.y)
                                    }
                                    onLongPressed: (x, y) => onRightClicked(x, y)
                                    onHoldMoved: (x, y) => {
                                        const p = mapToItem(Ui.overlay, x, y)
                                        setMenu.hoverAt(p.x, p.y)
                                    }
                                    onHoldReleased: (x, y) => {
                                        const p = mapToItem(Ui.overlay, x, y)
                                        setMenu.pickAt(p.x, p.y)
                                    }
                                    trailing: tags.width
                                    Row {
                                        id: tags
                                        anchors.right: parent.right
                                        anchors.rightMargin: 52
                                        anchors.verticalCenter: parent.verticalCenter
                                        spacing: 4
                                        ActionTag {
                                            action: setRow.set.action || "proxy"
                                            profile: setRow.set.profile || ""
                                            anchors.verticalCenter: parent.verticalCenter
                                        }
                                        Item {
                                            visible: setRow.on && order.onCount > 1
                                            width: 40
                                            height: 40
                                            anchors.verticalCenter: parent.verticalCenter
                                            Accessible.role: Accessible.Graphic
                                            Accessible.name: Ui.tr("Перетащить", "Drag")
                                            StateLayer {
                                                anchors.fill: parent
                                                radii: Qt.vector4d(20, 20, 20, 20)
                                                hovered: grip.hovered
                                                pressed: drag.active
                                            }
                                            Icon {
                                                anchors.centerIn: parent
                                                name: "drag_handle"
                                                ColorTo on color { to: setRow.dragging ? Theme.primary : Theme.onSurfaceVariant }
                                            }
                                            HoverHandler {
                                                id: grip
                                                cursorShape: Qt.SizeAllCursor
                                            }
                                            DragHandler {
                                                id: drag
                                                target: null
                                                xAxis.enabled: false
                                                property real startY
                                                onActiveChanged: {
                                                    if (active) {
                                                        startY = setRow.y
                                                        order.pick(setRow.key, setRow.y)
                                                    } else {
                                                        order.drop()
                                                    }
                                                }
                                                onTranslationChanged: if (active) order.move(startY + translation.y)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Column {
                        id: list
                        y: 0
                        width: parent.width
                        Spring on y { to: order.height; stiffness: M.spatialK; damping: 0.85 }
                        Header {
                            visible: page.rules.length > 0
                            text: Ui.tr("Отдельные правила", "Single rules")
                        }
                        Group {
                            Repeater {
                                model: page.rules
                                SettingRow {
                                    id: ruleRow
                                    required property var modelData
                                    required property int index
                                    property real grow: 1
                                    icon: modelData.kind === "domain" ? "link" : modelData.kind === "ip" ? "conversion_path" : "apps"
                                    shape: "Cookie6Sided"
                                    title: modelData.value
                                    middleElide: true
                                    summary: modelData.kind === "domain" ? Ui.tr("Домен", "Domain") : modelData.kind === "ip" ? "IP" : Ui.tr("Приложение", "App")
                                    onActivated: ruleDialog.edit(index, modelData)
                                    transform: Scale {
                                        origin.x: ruleRow.width / 2
                                        origin.y: ruleRow.height / 2
                                        xScale: ruleRow.grow
                                        yScale: ruleRow.grow
                                    }
                                    Spring on grow { id: ruleGrow; to: 1; stiffness: 520; damping: 0.62 }
                                    NumberAnimation { id: ruleFade; target: ruleRow; property: "opacity"; from: 0; to: 1; duration: 200; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate }
                                    Component.onCompleted: {
                                        if (index === page.poppedRule && Date.now() - page.poppedAt < 600) {
                                            ruleGrow.animateFrom(0.86)
                                            ruleFade.start()
                                        } else if (Date.now() - page.enteredAt < 400) {
                                            Ui.cascadeAt(ruleRow, 0.8, order.height + page.headH + index * page.rowH, page.enteredAt)
                                        }
                                    }
                                    trailing: ruleTag.width
                                    ActionTag {
                                        id: ruleTag
                                        anchors.right: parent.right
                                        anchors.rightMargin: 52
                                        anchors.verticalCenter: parent.verticalCenter
                                        action: modelData.action
                                        profile: modelData.profile
                                    }
                                }
                            }
                        }
                        Item { width: 1; height: page.rules.length > 0 || order.height > 0 ? 16 : 0 }
                        Group {
                            SettingRow {
                                readonly property int extra: (App.rev < 0 ? undefined : App.extraRuleLines())
                                icon: "data_object"
                                shape: "Pill"
                                title: Ui.tr("Дополнительно (mihomo)", "Advanced (mihomo)")
                                summary: extra > 0 ? App.plural(extra, "стр.", "стр.", "стр.", "line")
                                    : Ui.tr("Свои правила в формате mihomo", "Your own rules in mihomo format")
                                onActivated: raw.edit(Ui.tr("Дополнительно (mihomo)", "Advanced (mihomo)"))
                            }
                        }
                    }
                }
            }

            FabMenu {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 4
                z: 5
                items: [
                    { icon: "link", text: Ui.tr("Набор по ссылке", "Set from a link"), action: () => importDialog.ask() },
                    { icon: "grid_view", text: Ui.tr("Набор", "Set"), action: () => page.openEditor("") },
                    { icon: "conversion_path", text: "Tailscale", action: () => tailscale.show() },
                    { icon: "conversion_path", text: Ui.tr("Правило", "Rule"), action: () => ruleDialog.edit(-1, null) }
                ]
            }
        }
    }

    Dialog {
        id: tailscale
        title: "Tailscale"
        preferredWidth: 440
        Loader {
            width: parent.width
            active: tailscale.open
            sourceComponent: ServerPicker {
                onPicked: (s, p) => {
                    const set = App.newSet()
                    set.name = "Tailscale"
                    set.action = "server"
                    set.enabled = true
                    set.sub = s
                    set.profile = p
                    set.ips = ["100.64.0.0/10", "fd7a:115c:a1e0::/48"]
                    set.domains = ["ts.net"]
                    App.saveSet(set)
                    tailscale.close()
                }
            }
        }
        buttons: [Btn { kind: "text"; text: Ui.tr("Отмена", "Cancel"); onClicked: tailscale.close() }]
    }

    function openEditor(id) {
        editing = id
        editor.start(id)
    }

    Panes {
        id: editorPanes
        readonly property bool shown: page.editing !== "none"
        property real dx: 56
        opacity: shown ? 1 : 0
        visible: opacity > 0.005
        transform: Translate { x: editorPanes.dx }
        Behavior on opacity {
            NumberAnimation {
                duration: editorPanes.shown ? 260 : 130
                easing.type: Easing.BezierSpline
                easing.bezierCurve: editorPanes.shown ? M.decelerate : M.accelerate
            }
        }
        Spring on dx { to: editorPanes.shown ? 0 : 56; stiffness: M.spatialK; damping: 0.86 }
        SetEditor {
            id: editor
            anchors.fill: parent
            onBack: page.editing = "none"
        }
    }

    Menu {
        id: setMenu
        MenuItem {
            icon: "edit"
            text: Ui.tr("Изменить", "Edit")
            onTriggered: {
                setMenu.close()
                page.openEditor(page.menuFor)
            }
        }
        MenuItem {
            visible: (page.menuSet.size || 0) > 0
            icon: "share"
            text: Ui.tr("Поделиться", "Share")
            onTriggered: {
                setMenu.close()
                share.shareSet(page.menuSet)
            }
        }
        MenuGap {}
        MenuItem {
            icon: "delete"
            danger: true
            text: Ui.tr("Удалить", "Delete")
            onTriggered: {
                setMenu.close()
                deleting.show()
            }
        }
    }

    ModeRulesDialog { id: modeDialog }

    RuleDialog {
        id: ruleDialog
        onSaving: (i) => {
            page.poppedRule = i >= 0 ? i : page.rules.length
            page.poppedAt = Date.now()
        }
    }
    SetImportDialog { id: importDialog }
    ShareSetDialog { id: share }
    EditDialog {
        id: raw
        multiline: true
        happImport: true
        setting: "userRules"
        placeholder: "DOMAIN-SUFFIX,example.com,DIRECT\nGEOSITE,youtube,PROXY"
    }
    ConfirmDialog {
        id: deleting
        title: Ui.tr("Удалить набор?", "Delete the set?")
        text: SetLook.name(page.menuSet)
        onAccepted: App.deleteSet(page.menuFor)
    }
}
