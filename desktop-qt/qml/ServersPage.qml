import QtQuick

Item {
    id: page

    property string focusKey
    property bool embedded: false
    readonly property bool picking: App.entryPick
    readonly property bool two: !embedded && !picking && Ui.widthClass === 2 && width >= 900
    property string menuFor
    property real side: 0
    property bool seen: false
    Spring on side {
        id: siding
        to: page.two ? 1 : 0
        stiffness: M.spatialK
        damping: 0.9
    }
    onTwoChanged: if (!seen) siding.snap(two ? 1 : 0)
    readonly property string menuSub: picking ? App.entrySub : App.currentSub
    readonly property var menuServer: (App.rev < 0 ? undefined : menuFor === "" ? ({}) : picking ? App.entryServer(menuFor) : App.server(menuFor))
    readonly property bool menuCurrent: menuSub === App.currentSub

    property var menuOf: ({})
    property int menuCount: -1
    readonly property bool menuLocal: menuOf.local === true

    function subIndex() {
        const all = App.subs
        for (let i = 0; i < all.length; ++i)
            if (all[i].id === menuSub)
                return i
        return -1
    }
    property bool allShown: false
    function showSub(id) {
        allShown = false
        if (id === menuSub)
            return
        if (picking)
            App.showEntrySub(id)
        else
            App.selectSub(id)
    }
    function flip(by) {
        const all = App.subs
        if (allShown) {
            if (by > 0 && all.length > 0)
                showSub(menuSub !== "" ? menuSub : all[0].id)
            return
        }
        const i = subIndex()
        if (i + by < 0 && !picking) {
            allShown = true
            return
        }
        const next = Math.max(0, Math.min(all.length - 1, i + by))
        if (all.length > 0 && next !== i)
            showSub(all[next].id)
    }
    onMenuSubChanged: {
        list.toTop()
        Qt.callLater(chips.reveal)
    }
    function openSubMenu(sub, chip) {
        menuOf = sub
        menuCount = sub.id === App.currentSub ? App.supportedCount : -1
        if (sub.id !== App.currentSub)
            App.loadServersOf(sub.id)
        subMenu.popup(chip)
    }
    Connections {
        target: App
        function onServersOfReady(id, servers) {
            if (id !== page.menuOf.id || id === App.currentSub)
                return
            page.menuCount = servers.filter(s => s.supported === true && s.placeholder !== true).length
        }
    }
    readonly property Item focusNow: Window.activeFocusItem
    readonly property bool arrowsFree: visible && App.nav === (embedded ? "home" : "servers") && !subMenu.open && (focusNow === null || focusNow === Ui.overlay
        || (Ui.within(focusNow, page) && !Ui.within(focusNow, searchIn) && !(focusNow.options !== undefined)))
    Shortcut {
        sequence: "Left"
        enabled: page.arrowsFree
        onActivated: page.flip(-1)
    }
    Shortcut {
        sequence: "Right"
        enabled: page.arrowsFree
        onActivated: page.flip(1)
    }

    function enter() {
        seen = true
        const d = parent && parent.dir !== undefined ? parent.dir : 1
        searchIn.play(d)
        chipsIn.play(d)
        sideIn.play(d)
        list.enter(d)
        fab.come()
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
    }

    ServerGrid {
        id: grid
        query: search.text
        entry: page.picking
        focused: page.two ? page.focusKey : ""
        scope: page.picking ? "entry:" + App.entrySub : App.currentSub
        pinSelected: !page.embedded
    }

    Panes {
        bare: page.embedded
        Pane {
            id: main
            width: parent.width - (360 + 24) * page.side
            height: parent.height
            pad: page.embedded ? 16 : 24
            radius: page.embedded ? Ui.outer : Ui.compact ? 0 : M.xl
            color: page.embedded ? Theme.surfaceContainerLow : Ui.compact ? "transparent" : Theme.surface
            title: page.picking ? Ui.tr("Вход цепочки", "Chain entry") : Ui.tr("Серверы", "Servers")

            actions: [
                IconBtn {
                    visible: !page.embedded && (page.picking || Ui.split)
                    icon: page.picking ? "close" : "arrow_back"
                    tip: page.picking ? Ui.tr("Закрыть", "Close") : Ui.tr("Назад", "Back")
                    onClicked: {
                        App.entryPick = false
                        App.nav = "home"
                    }
                },
                Choice {
                    visible: !page.picking && main.width > 520
                    compact: true
                    width: implicitWidth
                    options: ["default", "ping", "name"]
                    labels: [Ui.tr("Как есть", "Default"), Ui.tr("Пинг", "Ping"), Ui.tr("А–Я", "A–Z")]
                    selected: App.settings.sort
                    onPicked: (v) => App.set("sort", v)
                }
            ]

            Appear {
                id: searchIn
                width: parent.width
                height: search.height
                step: 30
                Search {
                    id: search
                    width: parent.width
                    hint: Ui.tr("Поиск: страна или протокол", "Search: country or protocol")
                }
            }
            Appear {
                id: chipsIn
                index: 1
                step: 30
                anchors.top: searchIn.bottom
                anchors.topMargin: 12
                width: parent.width
                height: 36
                Flickable {
                    id: chips
                    x: -6
                    y: -6
                    width: parent.width + 12
                    height: 48
                    contentWidth: chipRow.implicitWidth + 12
                    flickableDirection: Flickable.HorizontalFlick
                    boundsBehavior: Flickable.StopAtBounds
                    clip: true
                    function reveal() {
                        const i = page.subIndex()
                        const it = i >= 0 ? subChips.itemAt(i) : null
                        if (!it)
                            return
                        const left = chipRow.x + it.x - 6, right = chipRow.x + it.x + it.width + 6
                        if (left < contentX)
                            contentX = Math.max(0, left)
                        else if (right > contentX + width)
                            contentX = Math.min(contentWidth - width, right - width)
                    }
                    Row {
                        id: chipRow
                        x: 6
                        y: 6
                        spacing: 4
                        SubChip {
                            visible: !page.picking
                            text: Ui.tr("Все серверы", "All servers")
                            selected: page.allShown
                            onClicked: page.allShown = true
                        }
                        Repeater {
                            id: subChips
                            model: App.subs
                            SubChip {
                                id: chip
                                required property var modelData
                                text: modelData.name
                                selected: !page.allShown && modelData.id === page.menuSub
                                menu: subMenu
                                onClicked: {
                                    if (selected && !page.picking)
                                        page.openSubMenu(modelData, chip)
                                    else if (page.allShown && modelData.id === page.menuSub)
                                        page.allShown = false
                                    else
                                        page.showSub(modelData.id)
                                }
                                onMenuAsked: if (!page.picking) page.openSubMenu(modelData, chip)
                            }
                        }
                        SubChip {
                            visible: !page.picking
                            text: Ui.tr("Добавить", "Add")
                            icon: "add"
                            onClicked: adder.ask()
                        }
                    }
                }
            }
            Loader {
                active: page.allShown
                visible: active
                anchors.top: chipsIn.bottom
                anchors.topMargin: 12
                anchors.bottom: parent.bottom
                width: parent.width
                sourceComponent: AllServers { query: search.text }
            }
            ServerList {
                id: list
                visible: !page.allShown
                anchors.top: chipsIn.bottom
                anchors.topMargin: 12
                anchors.bottom: parent.bottom
                width: parent.width
                grid: grid
                bottomPad: page.picking ? 24 : 96
                onPicked: (remarks) => {
                    if (page.picking) {
                        App.pickEntry(remarks)
                        return
                    }
                    page.focusKey = remarks
                    App.selectServer(remarks)
                }
                onMenu: (remarks, at, x, y) => {
                    page.menuFor = remarks
                    const p = at.mapToItem(Ui.overlay, x, y)
                    menu.place(p.x, p.y)
                }
            }
            MouseArea {
                anchors.fill: list
                acceptedButtons: Qt.NoButton
                property real acc: 0
                property bool spent: false
                onWheel: (w) => {
                    const dx = w.pixelDelta.x !== 0 ? w.pixelDelta.x * 2 : w.angleDelta.x
                    if (Math.abs(dx) <= Math.abs(w.angleDelta.y) || dx === 0) {
                        w.accepted = false
                        return
                    }
                    idle.restart()
                    if (spent)
                        return
                    acc += dx
                    if (Math.abs(acc) >= 120) {
                        page.flip(acc < 0 ? 1 : -1)
                        spent = true
                        acc = 0
                    }
                }
                Timer {
                    id: idle
                    interval: 250
                    onTriggered: {
                        parent.spent = false
                        parent.acc = 0
                    }
                }
            }
            PingFab {
                id: fab
                property real pop: 1
                property int total: 0
                function come() {
                    popper.snap(0)
                    popLater.restart()
                }
                visible: !page.picking && !page.allShown
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                property real rise: 0
                anchors.bottomMargin: (Ui.compact ? 16 : 4) + rise
                Spring on rise {
                    to: Ui.snackHeight === 0 || !(Ui.compact || main.width < 720) ? 0 : Ui.snackHeight + (Ui.compact ? 20 : 28)
                    stiffness: M.spatialK
                    damping: 0.8
                }
                scale: pop
                running: App.pinging
                onRunningChanged: total = running ? App.pingWaiting.length : 0
                progress: {
                    const left = App.pingWaiting.length
                    const all = Math.max(total, left)
                    return all > 0 ? (all - left) / all : 0
                }
                label: App.pinging ? Ui.tr("Остановить проверку", "Stop checking") : Ui.tr("Проверить пинг", "Check ping")
                onClicked: {
                    if (App.pinging) {
                        App.stopPing()
                        return
                    }
                    if (search.text.trim() === "") App.ping()
                    else {
                        const names = grid.pingTargets()
                        if (names.length > 0) App.ping(names)
                    }
                }
                Spring on pop { id: popper; to: 1; stiffness: 500; damping: 0.55 }
                Timer {
                    id: popLater
                    interval: 160
                    onTriggered: popper.to = 1
                }
            }
            T {
                id: none
                readonly property bool on: grid.tiles === 0 && search.text !== ""
                property real shown: 0
                property real rise: 0
                visible: shown > 0.01
                opacity: shown
                anchors.centerIn: list
                anchors.verticalCenterOffset: rise
                onOnChanged: if (on) riser.animateFrom(14)
                Spring on shown { to: none.on ? 1 : 0; stiffness: none.on ? M.effectsK : M.effectsFastK; damping: 1 }
                Spring on rise { id: riser; to: 0; stiffness: M.spatialK; damping: 0.75 }
                text: Ui.tr("Ничего не нашлось", "Nothing found")
                type: "bodyLarge"
                color: Theme.onSurfaceVariant
            }
        }

        Appear {
            id: sideIn
            index: 1
            step: 30
            lift: 16
            from: 0.98
            visible: page.side > 0.004
            x: main.width + 24
            width: 360
            height: parent.height
            Pane {
                id: side
                anchors.fill: parent
                opacity: Math.min(1, page.side * 1.6)
                ServerDetails {
                    anchors.fill: parent
                    remarks: page.focusKey !== "" && App.server(page.focusKey).remarks !== undefined ? page.focusKey : (App.settings.profile || "")
                    onDns: (r) => dns.openFor(r)
                }
            }
        }
    }

    Menu {
        id: menu
        T {
            visible: page.menuServer.supported === false
            width: Math.min(280, implicitWidth)
            leftPadding: 16
            rightPadding: 16
            topPadding: 8
            bottomPadding: 8
            text: page.menuServer.reason || Ui.tr("Не поддерживается", "Not supported")
            type: "bodySmall"
            color: Theme.error
            wrapMode: Text.Wrap
        }
        MenuItem {
            visible: page.menuServer.supported === true && page.menuCurrent
            readonly property bool connected: App.phase === 2 && App.connectedServer.sub === page.menuSub && App.connectedServer.remarks === page.menuFor
            icon: "power_settings_new"
            text: connected ? Ui.tr("Отключиться", "Disconnect") : Ui.tr("Подключиться", "Connect")
            onTriggered: connected ? App.disconnectVpn() : App.connectTo(page.menuFor)
        }
        MenuItem {
            visible: page.menuServer.supported === true && page.menuCurrent
            icon: "bolt"
            text: Ui.tr("Проверить пинг", "Check ping")
            onTriggered: App.pingSome([page.menuFor])
        }
        MenuItem {
            visible: page.menuServer.supported === true && !page.picking
            icon: "speed"
            text: Ui.tr("Скорость", "Speed")
            onTriggered: App.openSpeed(page.menuFor)
        }
        MenuItem {
            visible: page.menuServer.supported === true && (isEntry || !(page.menuCurrent && page.menuFor === App.settings.profile))
            icon: "link"
            readonly property bool isEntry: App.settings.chainActive === true && App.settings.chainSub === page.menuSub
                && App.settings.chainProfile === page.menuFor
            text: isEntry ? Ui.tr("Убрать цепочку", "Remove chain") : Ui.tr("Сделать входом цепочки", "Make chain entry")
            onTriggered: App.setChainEntry(isEntry ? "" : page.menuFor, page.menuSub)
        }
        MenuItem {
            visible: page.menuServer.supported === true && !page.picking
            readonly property bool isPinned: (App.rev < 0 ? undefined : App.pinned(page.menuFor))
            icon: "push_pin"
            text: isPinned ? Ui.tr("Открепить", "Unpin") : Ui.tr("Закрепить", "Pin")
            onTriggered: App.pin(page.menuFor, !isPinned)
        }
        MenuItem {
            visible: page.menuServer.supported === true && !page.picking
            icon: "language"
            text: "DNS…"
            onTriggered: dns.openFor(page.menuFor)
        }
        MenuItem {
            visible: page.menuCurrent && !page.picking
            icon: "info"
            text: Ui.tr("Подробности", "Details")
            onTriggered: {
                configDialog.active = true
                configDialog.item.showFor(page.menuFor)
            }
        }
        MenuItem {
            visible: !page.picking && App.current.local === true
            icon: "edit"
            text: Ui.tr("Переименовать", "Rename")
            onTriggered: renaming.edit(Ui.tr("Название сервера", "Server name"), "serverName:" + page.menuFor)
        }
        MenuItem {
            visible: !page.picking && App.current.local === true
            icon: "delete"
            danger: true
            text: Ui.tr("Удалить", "Delete")
            onTriggered: App.removeLocal(page.menuFor)
        }
        T {
            readonly property string error: (App.rev < 0 ? undefined : page.menuFor !== "" ? App.ping(page.menuFor).error || "" : "")
            visible: error !== ""
            width: Math.min(280, implicitWidth)
            leftPadding: 16
            rightPadding: 16
            topPadding: 6
            bottomPadding: 6
            text: error
            type: "bodySmall"
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
        }
    }

    Menu {
        id: subMenu
        grouped: true
        minWidth: 240
        MenuGroup {
            Column {
                x: 16
                width: parent.width - 32
                topPadding: 10
                bottomPadding: 10
                spacing: 4
                readonly property var sub: page.menuOf
                readonly property double used: sub.used || 0
                readonly property double remain: sub.expire > 0 ? sub.expire - Date.now() / 1000 : NaN
                readonly property bool soon: !isNaN(remain) && remain < 3 * 86400
                T {
                    id: title
                    width: parent.width
                    text: parent.sub.name || ""
                    type: "titleMedium"
                    color: Theme.onSurface
                }
                T {
                    width: parent.width
                    text: Ui.joined([page.menuCount >= 0 ? App.plural(page.menuCount, "сервер", "сервера", "серверов", "server") : "",
                                     !page.menuLocal && parent.sub.updated > 0 ? Ui.tr("обновлена ", "updated ") + App.ago(Date.now() / 1000 - parent.sub.updated) : ""], " · ")
                    type: "bodySmall"
                    color: Theme.onSurfaceVariant
                }
                Row {
                    id: facts
                    visible: !page.menuLocal
                    topPadding: 4
                    spacing: 6
                    Icon { name: "data_usage"; size: 16; anchors.verticalCenter: parent.verticalCenter }
                    T {
                        anchors.verticalCenter: parent.verticalCenter
                        text: facts.parent.sub.total > 0 ? Ui.tr("%1 из %2", "%1 of %2").arg(App.bytes(facts.parent.used)).arg(App.bytes(facts.parent.sub.total))
                            : facts.parent.used > 0 ? App.bytes(facts.parent.used) : Ui.tr("без лимита", "unlimited")
                        type: "labelLarge"
                        color: Theme.onSurface
                    }
                    Item { width: 8; height: 1; visible: !isNaN(facts.parent.remain) }
                    Icon {
                        visible: !isNaN(facts.parent.remain)
                        name: "timer"
                        size: 16
                        color: facts.parent.soon ? Theme.error : Theme.onSurfaceVariant
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    T {
                        visible: !isNaN(facts.parent.remain)
                        anchors.verticalCenter: parent.verticalCenter
                        readonly property int days: Math.floor(facts.parent.remain / 86400)
                        text: facts.parent.remain <= 0 ? Ui.tr("истекла", "expired") : days === 0 ? Ui.tr("последний день", "last day")
                            : App.plural(days, "день", "дня", "дней", "day")
                        type: "labelLarge"
                        color: facts.parent.soon ? Theme.error : Theme.onSurface
                    }
                }
                Item {
                    visible: !page.menuLocal && parent.sub.total > 0
                    width: parent.width
                    height: 18
                    WavyProgress {
                        y: 6
                        width: parent.width
                        height: 10
                        amplitude: 2
                        moving: false
                        value: parent.parent.sub.total > 0 ? Math.min(1, parent.parent.used / parent.parent.sub.total) : 0
                        color: Theme.primary
                        trackColor: Theme.surfaceContainerHighest
                    }
                }
                T {
                    visible: text !== ""
                    width: parent.width
                    topPadding: 2
                    text: (parent.sub.announce || "").trim()
                    type: "bodySmall"
                    color: Theme.onSurfaceVariant
                    wrapMode: Text.Wrap
                    maximumLineCount: 8
                    elide: Text.ElideRight
                }
            }
        }
        MenuGroup {
            MenuItem {
                visible: page.menuLocal
                icon: "add"
                text: Ui.tr("Добавить сервер", "Add server")
                onTriggered: serverImport.show()
            }
            MenuItem {
                visible: page.menuLocal
                icon: "bolt"
                text: Ui.tr("Убрать нерабочие", "Remove dead ones")
                onTriggered: App.removeDead()
            }
            MenuItem {
                visible: !page.menuLocal
                icon: "refresh"
                text: App.busy ? Ui.tr("Обновляю…", "Updating…") : Ui.tr("Обновить", "Update")
                enabled: !App.busy
                onTriggered: App.refresh(page.menuOf.id)
            }
            MenuItem {
                visible: !page.menuLocal
                icon: "edit"
                text: Ui.tr("Изменить", "Edit")
                onTriggered: editing.edit(page.menuOf)
            }
            MenuItem {
                icon: "content_copy"
                text: Ui.tr("Скопировать конфиги", "Copy configs")
                onTriggered: App.copyLinks(page.menuOf.id)
            }
            MenuItem {
                visible: !page.menuLocal
                icon: "qr_code"
                text: Ui.tr("QR-код", "QR code")
                onTriggered: qrDialog.showLink(page.menuOf.name || "", page.menuOf.url || "")
            }
            MenuItem {
                visible: !page.menuLocal && (page.menuOf.supportUrl || "") !== ""
                icon: "support_agent"
                text: Ui.tr("Поддержка", "Support")
                onTriggered: App.open(page.menuOf.supportUrl)
            }
        }
        MenuGroup {
            MenuItem {
                icon: "delete"
                danger: true
                text: Ui.tr("Удалить", "Delete")
                onTriggered: {
                    removing.sub = page.menuOf
                    removing.show()
                }
            }
        }
    }
    ConfirmDialog {
        id: removing
        property var sub: ({})
        title: sub.local === true ? Ui.tr("Удалить свои серверы?", "Delete your servers?") : Ui.tr("Удалить подписку?", "Delete subscription?")
        text: sub.local === true ? Ui.tr("Все добавленные вручную серверы будут удалены.", "All manually added servers will be deleted.")
            : Ui.tr("«%1» и её серверы будут удалены.", "\"%1\" and its servers will be deleted.").arg(sub.name || "")
        onAccepted: App.remove(sub.id)
    }
    SubEditDialog { id: editing }
    QrDialog { id: qrDialog }
    ServerImportDialog { id: serverImport }
    AutoWhyDialog { id: why }
    DnsDialog { id: dns }
    EditDialog {
        id: renaming
        allowEmpty: false
        mono: false
    }
    SpeedDialog {}
    Loader {
        id: configDialog
        active: false
        sourceComponent: ConfigDialog {}
    }

    AddDialog { id: adder }
}
