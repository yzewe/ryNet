import QtQuick

Item {
    id: page

    property bool follow: true
    property int seen: 0
    property int freshFrom: 0
    readonly property bool narrow: width <= 760

    onFollowChanged: if (follow) Qt.callLater(view.glideToEnd)

    function saveDiag() {
        App.pickFile(true, App.diagName(), Ui.tr("Журнал ryNet", "ryNet log"), "zip", (path) => App.exportDiag(path))
    }
    function copyAll() {
        App.copy(log.text())
        App.notice = Ui.tr("Скопировано", "Copied")
    }

    LogModel {
        id: log
        active: page.visible
        query: filter.text
    }

    property bool unseen: false
    Connections {
        target: page.parent
        function onOpacityChanged() {
            if (page.parent.opacity < 0.01) {
                page.unseen = true
            } else if (page.unseen) {
                page.unseen = false
                empty.play()
            }
        }
    }
    Component.onCompleted: {
        unseen = parent !== null && parent.opacity < 0.01
        if (!unseen)
            empty.play()
    }

    Panes {
        Pane {
            id: pane
            anchors.fill: parent
            title: Ui.tr("Журнал", "Log")
            actions: [
                IconBtn {
                    visible: Ui.compact
                    anchors.verticalCenter: parent ? parent.verticalCenter : undefined
                    icon: "arrow_back"
                    tip: Ui.tr("Назад", "Back")
                    onClicked: App.nav = "settings"
                },
                Item {
                    id: filterSlot
                    visible: !Ui.compact
                    width: Math.min(320, page.width - 240)
                    height: 48
                },
                IconBtn {
                    id: more
                    visible: page.narrow
                    anchors.verticalCenter: parent ? parent.verticalCenter : undefined
                    icon: "more_vert"
                    tip: tools.open ? "" : Ui.tr("Ещё", "More")
                    onClicked: tools.popupEnd(more)
                }
            ]

            Item {
                id: compactSlot
                visible: Ui.compact
                width: parent.width
                height: visible ? 48 : 0
            }
            Item {
                id: bar
                anchors.top: compactSlot.bottom
                anchors.topMargin: compactSlot.visible ? 12 : 0
                width: parent.width
                height: 40
                MouseArea {
                    anchors.fill: levels
                    enabled: levels.interactive
                    acceptedButtons: Qt.NoButton
                    onWheel: (w) => {
                        if (w.angleDelta.y === 0) {
                            w.accepted = false
                            return
                        }
                        levels.contentX = Math.max(0, Math.min(levels.contentWidth - levels.width, levels.contentX - w.angleDelta.y / 2))
                    }
                }
                Flickable {
                    id: levels
                    x: -pane.side
                    width: page.narrow ? parent.width + pane.side * 2 : toolbar.x - 8 + pane.side
                    height: parent.height
                    contentWidth: chips.width + pane.side * (page.narrow ? 2 : 1)
                    contentHeight: height
                    interactive: contentWidth > width
                    flickableDirection: Flickable.HorizontalFlick
                    boundsBehavior: Flickable.StopAtBounds
                    clip: true
                    function reveal(chip) {
                        if (chip.x < contentX)
                            contentX = chip.x
                        else if (chip.x + chip.width + chips.x * 2 > contentX + width)
                            contentX = Math.min(contentWidth - width, chip.x + chip.width + chips.x * 2 - width)
                    }
                    Row {
                        id: chips
                        x: pane.side
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 8
                        Repeater {
                            model: [
                                { key: "all", title: Ui.tr("Все", "All") },
                                { key: "info", title: Ui.tr("Инфо", "Info") },
                                { key: "warn", title: Ui.tr("Предупреждения", "Warnings") },
                                { key: "error", title: Ui.tr("Ошибки", "Errors") }
                            ]
                            LevelChip {
                                id: chip
                                required property var modelData
                                readonly property int n: modelData.key === "warn" ? log.warnings : modelData.key === "error" ? log.errors : 0
                                text: n > 0 ? modelData.title + " · " + n : modelData.title
                                on: log.level === modelData.key
                                onClicked: {
                                    if (log.level === modelData.key)
                                        return
                                    log.level = modelData.key
                                    view.reveal()
                                }
                                onActiveFocusChanged: if (activeFocus) levels.reveal(chip)
                            }
                        }
                    }
                }
                Row {
                    id: toolbar
                    anchors.right: parent.right
                    spacing: 4
                    visible: !page.narrow
                    IconBtn {
                        kind: "filled"
                        toggle: true
                        checked: page.follow
                        icon: "arrow_downward"
                        tip: Ui.tr("Следить", "Follow")
                        onClicked: page.follow = !page.follow
                    }
                    IconBtn {
                        kind: "filled"
                        toggle: true
                        checked: log.paused
                        icon: log.paused ? "play_arrow" : "pause"
                        tip: log.paused ? Ui.tr("Продолжить", "Resume") : Ui.tr("Пауза", "Pause")
                        onClicked: log.paused = !log.paused
                    }
                    IconBtn {
                        kind: "filled"
                        toggle: true
                        checked: App.settings.debugLog === true
                        icon: "bug_report"
                        tip: Ui.tr("Подробный журнал", "Verbose log")
                        onClicked: App.setDebugLog(!App.settings.debugLog)
                    }
                    IconBtn {
                        icon: "content_copy"
                        tip: Ui.tr("Скопировать", "Copy")
                        enabled: log.count > 0
                        onClicked: page.copyAll()
                    }
                    IconBtn {
                        icon: "download"
                        tip: Ui.tr("Сохранить журнал", "Save log")
                        onClicked: page.saveDiag()
                    }
                    IconBtn {
                        icon: "folder"
                        tip: Ui.tr("Открыть папку", "Open folder")
                        onClicked: App.open(App.dataDir())
                    }
                    IconBtn {
                        icon: "delete"
                        tip: Ui.tr("Очистить", "Clear")
                        onClicked: log.clear()
                    }
                }
            }

            Rectangle {
                anchors.top: bar.bottom
                anchors.topMargin: 12
                anchors.bottom: parent.bottom
                width: parent.width
                radius: 20
                color: Theme.surfaceContainerLow
                clip: true

                Item {
                    id: empty
                    readonly property bool on: log.count === 0
                    property real k: 0
                    property bool filtered: false
                    Binding on filtered {
                        when: empty.on
                        value: log.total > 0
                        restoreMode: Binding.RestoreNone
                    }
                    anchors.centerIn: parent
                    width: emptyColumn.width
                    height: emptyColumn.height
                    visible: k > 0.01
                    opacity: Math.max(0, Math.min(1, k * 1.4))
                    scale: 0.7 + 0.3 * k
                    Spring on k {
                        id: emptySpring
                        to: empty.on ? 1 : 0
                        stiffness: empty.on ? M.spatialK : M.effectsK
                        damping: empty.on ? 0.68 : 1
                    }
                    onOnChanged: if (on) glyph.play()
                    function play() {
                        if (!on)
                            return
                        emptySpring.animateFrom(0)
                        glyph.play()
                    }
                    Column {
                        id: emptyColumn
                        spacing: 16
                        ShapeIcon {
                            id: glyph
                            anchors.horizontalCenter: parent.horizontalCenter
                            size: 72
                            icon: empty.filtered ? "search" : "terminal"
                            shape: "Cookie9Sided"
                            container: Theme.surfaceContainerHighest
                            color: Theme.onSurfaceVariant
                        }
                        T {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: empty.filtered ? Ui.tr("Ничего не нашлось", "Nothing found") : Ui.tr("Пусто", "Empty")
                            type: "titleMedium"
                            color: Theme.onSurfaceVariant
                        }
                    }
                }

                ListView {
                    id: view
                    anchors.fill: parent
                    anchors.margins: 8
                    anchors.rightMargin: 16
                    model: log
                    boundsBehavior: Flickable.StopAtBounds
                    property int lastCount: 0
                    property real drop: 0
                    function reveal() {
                        listDrop.animateFrom(16)
                        revealFade.restart()
                    }
                    transform: Translate { y: view.drop }
                    Spring on drop { id: listDrop; to: 0; stiffness: M.spatialK; damping: 0.8 }
                    NumberAnimation {
                        id: revealFade
                        target: view
                        property: "opacity"
                        from: 0
                        to: 1
                        duration: 220
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: M.decelerate
                    }
                    onMovingChanged: {
                        if (!moving)
                            return
                        glide.stop()
                        page.follow = atYEnd
                    }
                    onContentYChanged: if (moving && !atYEnd) page.follow = false
                    onCountChanged: {
                        const grew = count - lastCount
                        page.freshFrom = Math.min(lastCount, count)
                        lastCount = count
                        if (page.follow && count > 0) {
                            if (grew > 0 && grew <= 20 && page.freshFrom > 0) {
                                smoothPending = true
                                Qt.callLater(view.glideToEnd)
                            } else {
                                Qt.callLater(view.settle)
                            }
                        }
                    }
                    onContentHeightChanged: if (page.follow && count > 0) Qt.callLater(view.settle)
                    onHeightChanged: if (page.follow && count > 0) Qt.callLater(view.settle)
                    property bool smoothPending: false
                    function glideToEnd() {
                        smoothPending = false
                        pin(true)
                    }
                    function settle() {
                        if (!smoothPending && !glide.running)
                            pin(false)
                    }
                    function pin(smooth) {
                        if (!page.follow || count === 0 || moving)
                            return
                        const from = contentY
                        positionViewAtEnd()
                        const to = contentY
                        if (smooth && to - from > 0.5) {
                            contentY = Math.max(from, to - height)
                            glide.to = to
                            glide.restart()
                        } else if (!smooth) {
                            glide.stop()
                        }
                    }
                    NumberAnimation {
                        id: glide
                        target: view
                        property: "contentY"
                        duration: 260
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: M.decelerate
                        onFinished: view.pin(false)
                    }
                    delegate: Rectangle {
                        id: line
                        required property int index
                        required property string time
                        required property string level
                        required property string tag
                        required property string text
                        required property bool fresh
                        property real rise: 0
                        readonly property bool stacked: view.width < 560 && time !== ""
                        width: view.width
                        height: Math.max(26, body.y + body.implicitHeight + 5)
                        radius: 12
                        color: level === "ERROR" ? Theme.alpha(Theme.errorContainer, 0.45) : "transparent"
                        transform: Translate { y: line.rise }
                        Spring on rise { id: riseSpring; to: 0; stiffness: M.spatialK; damping: 0.86 }
                        NumberAnimation {
                            id: fadeIn
                            target: line
                            property: "opacity"
                            from: 0
                            to: 1
                            duration: 220
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: M.decelerate
                        }
                        Component.onCompleted: {
                            if (fresh && index >= page.freshFrom) {
                                riseSpring.animateFrom(14)
                                fadeIn.start()
                            }
                        }

                        T {
                            id: timeText
                            visible: line.time !== ""
                            x: 12
                            y: 6
                            width: 72
                            text: line.time
                            font.family: "monospace"
                            font.pixelSize: 12
                            color: Theme.onSurfaceVariant
                        }
                        Rectangle {
                            id: badge
                            visible: line.time !== ""
                            x: 84
                            y: 5
                            width: 60
                            height: 18
                            radius: 8
                            color: line.level === "ERROR" ? Theme.errorContainer : line.level === "WARN" ? Theme.warningContainer : Theme.surfaceContainerHighest
                            T {
                                anchors.centerIn: parent
                                text: line.level
                                type: "labelSmall"
                                font.weight: 700
                                color: line.level === "ERROR" ? Theme.onErrorContainer : line.level === "WARN" ? Theme.onWarningContainer : Theme.onSurfaceVariant
                            }
                        }
                        T {
                            id: tagText
                            visible: line.time !== ""
                            x: 158
                            y: 6
                            width: 80
                            text: line.tag
                            font.family: "monospace"
                            font.pixelSize: 12
                            color: Theme.primary
                        }
                        TextEdit {
                            id: body
                            x: line.time !== "" && !line.stacked ? 246 : 12
                            y: line.stacked ? 27 : 5
                            width: line.width - x - 12
                            readOnly: true
                            selectByMouse: true
                            text: line.text
                            wrapMode: TextEdit.WrapAtWordBoundaryOrAnywhere
                            font.family: "monospace"
                            font.pixelSize: 12
                            color: line.level === "ERROR" ? Theme.onErrorContainer : Theme.onSurface
                            selectionColor: Theme.alpha(Theme.primary, 0.35)
                            selectedTextColor: Theme.onSurface
                            textFormat: TextEdit.PlainText
                        }
                    }
                }
                Bar {
                    flick: view
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    anchors.margins: 4
                }
                Item {
                    id: toLatest
                    readonly property bool on: !page.follow && log.count > 0
                    property real k: 0
                    property real drop: 0
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.margins: 16
                    width: 40
                    height: 40
                    visible: k > 0.01
                    opacity: Math.max(0, Math.min(1, k * 1.6))
                    scale: 0.4 + 0.6 * k
                    transformOrigin: Item.BottomRight
                    Spring on k {
                        to: toLatest.on ? 1 : 0
                        stiffness: toLatest.on ? M.bouncyK : M.effectsK
                        damping: toLatest.on ? M.bouncyZ : 1
                    }
                    Spring on drop { id: dropSpring; to: 0; stiffness: M.fastK; damping: 0.5 }
                    onOnChanged: if (on) dropSpring.animateFrom(-14)

                    Shadow {
                        anchors.fill: parent
                        radius: fab.radius
                        level: fab.hovered ? 4 : 3
                        scale: fab.scale
                    }
                    Surface {
                        id: fab
                        anchors.fill: parent
                        radius: M.md
                        interactive: true
                        squish: true
                        squishScale: 0.9
                        color: Theme.primaryContainer
                        tint: Theme.onPrimaryContainer
                        Accessible.name: Ui.tr("К последним", "To latest")
                        onClicked: {
                            dropSpring.kick(160)
                            page.follow = true
                        }
                        onHoveredChanged: if (!hovered) Ui.hideTip(fab)
                        onPressedChanged: if (pressed) Ui.hideTip(fab)
                        Component.onDestruction: Ui.hideTip(fab)
                        Timer {
                            interval: 600
                            running: fab.hovered && !fab.pressed
                            onTriggered: Ui.showTip(fab, Ui.tr("К последним", "To latest"))
                        }
                        Icon {
                            anchors.centerIn: parent
                            anchors.verticalCenterOffset: toLatest.drop
                            name: "arrow_downward"
                            color: Theme.onPrimaryContainer
                        }
                    }
                }
            }
        }
    }

    Search {
        id: filter
        parent: Ui.compact ? compactSlot : filterSlot
        anchors.fill: parent
        hint: Ui.tr("Фильтр", "Filter")
        icon: "filter_list"
    }

    Menu {
        id: tools
        MenuItem {
            icon: "arrow_downward"
            checked: page.follow
            text: Ui.tr("Следить", "Follow")
            onTriggered: page.follow = !page.follow
        }
        MenuItem {
            icon: log.paused ? "play_arrow" : "pause"
            text: log.paused ? Ui.tr("Продолжить", "Resume") : Ui.tr("Пауза", "Pause")
            onTriggered: log.paused = !log.paused
        }
        MenuItem {
            icon: "bug_report"
            checked: App.settings.debugLog === true
            text: Ui.tr("Подробный журнал", "Verbose log")
            onTriggered: App.setDebugLog(!App.settings.debugLog)
        }
        MenuItem {
            icon: "content_copy"
            text: Ui.tr("Скопировать", "Copy")
            enabled: log.count > 0
            onTriggered: page.copyAll()
        }
        MenuItem {
            icon: "download"
            text: Ui.tr("Сохранить журнал", "Save log")
            onTriggered: page.saveDiag()
        }
        MenuItem {
            icon: "folder"
            text: Ui.tr("Открыть папку", "Open folder")
            onTriggered: App.open(App.dataDir())
        }
        MenuItem {
            icon: "delete"
            text: Ui.tr("Очистить", "Clear")
            onTriggered: log.clear()
        }
    }
}
