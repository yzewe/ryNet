import QtQuick

Dialog {
    id: dlg

    readonly property var run: App.speedRun
    readonly property bool has: run.profile !== undefined
    readonly property bool running: run.running === true
    readonly property int phase: run.phase || 0
    readonly property bool done: phase === 4
    readonly property var history: App.speedHistory
    readonly property bool showHistory: history.length > 0 && !running
    signal dealHistory()

    open: has
    onClosed: App.closeSpeed()
    icon: "speed"
    title: Ui.tr("Скорость", "Speed")
    maxWidth: 480
    Component.onCompleted: if ("preferredWidth" in dlg) dlg.preferredWidth = 480
    onPhaseChanged: {
        bigPop.animateFrom(0.88)
        counter.snap(done ? (run.download || 0) : 0)
    }
    onShowHistoryChanged: if (showHistory) dealHistory()
    onOpenChanged: if (open && showHistory) dealHistory()

    component Stat: Rectangle {
        id: tile

        property int index
        property bool rows
        property string title
        property string icon
        property string shape
        property string value
        property string unit
        property string sub
        property bool active
        property real pop: 1
        readonly property color fg: active ? Theme.onPrimaryContainer : Theme.onSurface
        readonly property real big: rows ? M.lgPlus : M.xl
        readonly property real small: rows ? M.xs : M.sm

        topLeftRadius: index === 0 ? big : small
        topRightRadius: (rows ? index === 0 : index === 2) ? big : small
        bottomLeftRadius: (rows ? index === 2 : index === 0) ? big : small
        bottomRightRadius: index === 2 ? big : small
        ColorTo on color { to: active ? Theme.primaryContainer : Theme.surfaceContainerHighest; duration: M.effectsSlowMs }

        onActiveChanged: if (active) glyph.play()
        onValueChanged: if (value !== "") popper.animateFrom(0.6)
        Spring on pop { id: popper; to: 1; stiffness: M.fastK; damping: 0.5 }

        ShapeIcon {
            id: glyph
            x: 14
            y: tile.rows ? (tile.height - height) / 2 : 14
            size: 32
            icon: tile.icon
            shape: tile.shape
            container: tile.active ? Theme.primary : Theme.primaryContainer
            color: tile.active ? Theme.onPrimary : Theme.onPrimaryContainer
        }
        Column {
            id: label
            x: tile.rows ? glyph.x + glyph.width + 14 : 14
            y: tile.rows ? (tile.height - height) / 2 : glyph.y + glyph.height + 8
            width: tile.rows ? reading.x - x - 8 : tile.width - 28
            T {
                width: parent.width
                text: tile.title
                type: tile.rows ? "labelLarge" : "labelMedium"
                color: Theme.alpha(tile.fg, 0.75)
            }
            T {
                visible: tile.rows && tile.sub !== ""
                width: parent.width
                text: tile.sub
                type: "labelSmall"
                color: Theme.alpha(tile.fg, 0.7)
            }
        }
        Item {
            id: reading
            x: tile.rows ? tile.width - width - 16 : 14
            y: tile.rows ? (tile.height - height) / 2 : label.y + label.height + 2
            width: tile.rows ? Math.max(32, figure.implicitWidth) : tile.width - 28
            height: 32
            Row {
                id: figure
                visible: tile.value !== ""
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: tile.rows ? parent.right : undefined
                spacing: 3
                scale: tile.pop
                transformOrigin: tile.rows ? Item.Right : Item.Left
                T {
                    id: amount
                    text: tile.value
                    type: "titleLargeE"
                    font.features: { "tnum": 1 }
                    color: tile.fg
                }
                T {
                    text: tile.unit
                    type: "labelSmall"
                    color: Theme.alpha(tile.fg, 0.7)
                    anchors.baseline: amount.baseline
                }
            }
            LoadingIndicator {
                visible: tile.value === "" && tile.active
                running: visible
                width: 30
                height: 30
                color: Theme.primary
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: tile.rows ? parent.right : undefined
            }
            T {
                visible: tile.value === "" && !tile.active
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: tile.rows ? parent.right : undefined
                text: "—"
                type: "titleLarge"
                color: Theme.outline
            }
        }
        T {
            visible: !tile.rows
            x: 14
            y: reading.y + reading.height + 2
            width: tile.width - 28
            text: tile.sub !== "" ? tile.sub : " "
            type: "labelSmall"
            color: Theme.alpha(tile.fg, 0.7)
        }
    }

    Column {
        width: parent.width
        spacing: 14

        Column {
            width: parent.width
            T {
                width: parent.width
                text: dlg.run.name || ""
                type: "titleMedium"
            }
            T {
                width: parent.width
                text: dlg.run.target || (dlg.running ? Ui.tr("Ищу сервер Ookla рядом с выходом VPN…", "Finding an Ookla server near the VPN exit…") : (dlg.run.kind || ""))
                type: "bodyMedium"
                color: Theme.onSurfaceVariant
            }
        }

        Column {
            width: parent.width
            T {
                id: phaseName
                property real dy: 0
                property real fade: 1
                anchors.horizontalCenter: parent.horizontalCenter
                text: dlg.done ? Ui.tr("Загрузка", "Download") : !dlg.running ? Ui.tr("Готов", "Ready")
                    : dlg.phase === 0 ? Ui.tr("Ищу сервер", "Finding server") : dlg.phase === 1 ? Ui.tr("Задержка", "Latency")
                    : dlg.phase === 2 ? Ui.tr("Загрузка", "Download") : Ui.tr("Отдача", "Upload")
                type: "labelLarge"
                color: Theme.onSurfaceVariant
                opacity: fade
                transform: Translate { y: phaseName.dy }
                onTextChanged: {
                    rolling.animateFrom(8)
                    fading.animateFrom(0)
                }
                Spring on dy { id: rolling; to: 0; stiffness: M.fastK; damping: 0.7 }
                Spring on fade { id: fading; to: 1; stiffness: M.effectsK; damping: 1 }
            }
            T {
                property real pop: 1
                property real shown: 0
                anchors.horizontalCenter: parent.horizontalCenter
                text: dlg.done ? (dlg.run.download !== undefined ? App.mbps(shown) : "—")
                    : dlg.running && dlg.phase === 1 ? String(Math.round(shown))
                    : dlg.running && dlg.phase >= 2 ? App.mbps(shown) : "—"
                type: "displayMedium"
                font.weight: 600
                font.features: { "tnum": 1 }
                scale: pop
                Spring on pop { id: bigPop; to: 1; stiffness: M.fastK; damping: 0.55 }
                Spring on shown {
                    id: counter
                    to: dlg.done ? (dlg.run.download || 0) : dlg.running && dlg.phase >= 1 ? (dlg.run.value || 0) : 0
                    stiffness: M.effectsSlowK
                    damping: 1
                }
            }
            T {
                anchors.horizontalCenter: parent.horizontalCenter
                text: dlg.running && dlg.phase === 1 ? Ui.tr("мс", "ms") : Ui.tr("Мбит/с", "Mbps")
                type: "titleSmall"
                color: Theme.onSurfaceVariant
            }
            Item { width: 1; height: 10 }
            WavyProgress {
                width: parent.width
                height: 14
                moving: dlg.running
                value: dlg.running ? Math.max(0, Math.min(1, dlg.run.fraction || 0)) : dlg.done ? 1 : 0
                color: Theme.primary
                trackColor: Theme.surfaceContainerHighest
            }
        }

        Item {
            id: results
            readonly property bool rows: width < 380
            width: parent.width
            height: rows ? 3 * 64 + 2 * 2 : 136

            Stat {
                index: 0
                rows: results.rows
                x: 0
                y: 0
                width: results.rows ? results.width : (results.width - 8) / 3
                height: results.rows ? 64 : results.height
                title: Ui.tr("Пинг", "Ping")
                icon: "show_chart"
                shape: "Cookie4Sided"
                value: dlg.run.ping !== undefined ? String(Math.round(dlg.run.ping)) : ""
                unit: Ui.tr("мс", "ms")
                sub: dlg.run.jitter !== undefined ? Ui.tr("джиттер %1", "jitter %1").arg(Math.round(dlg.run.jitter)) : ""
                active: dlg.running && dlg.phase === 1
            }
            Stat {
                index: 1
                rows: results.rows
                x: results.rows ? 0 : (results.width - 8) / 3 + 4
                y: results.rows ? 64 + 2 : 0
                width: results.rows ? results.width : (results.width - 8) / 3
                height: results.rows ? 64 : results.height
                title: Ui.tr("Загрузка", "Download")
                icon: "arrow_downward"
                shape: "Cookie9Sided"
                value: dlg.run.download !== undefined ? App.mbps(dlg.run.download) : ""
                unit: Ui.tr("Мбит/с", "Mbps")
                active: dlg.running && dlg.phase === 2
            }
            Stat {
                index: 2
                rows: results.rows
                x: results.rows ? 0 : ((results.width - 8) / 3 + 4) * 2
                y: results.rows ? (64 + 2) * 2 : 0
                width: results.rows ? results.width : (results.width - 8) / 3
                height: results.rows ? 64 : results.height
                title: Ui.tr("Отдача", "Upload")
                icon: "arrow_upward"
                shape: "Sunny"
                value: dlg.run.upload !== undefined ? App.mbps(dlg.run.upload) : ""
                unit: Ui.tr("Мбит/с", "Mbps")
                active: dlg.running && dlg.phase === 3
            }
        }

        Notice {
            visible: (dlg.run.error || "") !== ""
            width: parent.width
            error: true
            text: dlg.run.error || ""
        }

        Item {
            id: past
            property real open: 0
            width: parent.width
            height: pastColumn.implicitHeight * Math.max(0, Math.min(1, open))
            visible: open > 0.004
            opacity: Math.max(0, Math.min(1, open * 1.5 - 0.5))
            clip: open < 0.999
            Spring on open {
                to: dlg.showHistory ? 1 : 0
                stiffness: dlg.showHistory ? M.spatialK : M.effectsK
                damping: dlg.showHistory ? 0.85 : 1
            }

            Column {
                id: pastColumn
                width: parent.width
                spacing: 14
                T {
                    text: Ui.tr("История", "History")
                    type: "titleSmall"
                    color: Theme.primary
                }
                Column {
                    width: parent.width
                    spacing: 2
                    Repeater {
                        model: dlg.history.slice(0, 5)
                        Rectangle {
                            id: entry
                            required property var modelData
                            required property int index
                            readonly property int total: Math.min(dlg.history.length, 5)
                            property real rise: 0
                            width: parent.width
                            height: 44
                            color: Theme.surfaceContainerHighest
                            topLeftRadius: index === 0 ? M.lg : M.xs
                            topRightRadius: index === 0 ? M.lg : M.xs
                            bottomLeftRadius: index === total - 1 ? M.lg : M.xs
                            bottomRightRadius: index === total - 1 ? M.lg : M.xs
                            transform: Translate { y: entry.rise }
                            Spring on rise { id: rising; to: 0; stiffness: 420; damping: 0.78 }
                            NumberAnimation {
                                id: showing
                                target: entry
                                property: "opacity"
                                to: 1
                                duration: 220
                                easing.type: Easing.BezierSpline
                                easing.bezierCurve: M.decelerate
                            }
                            Timer {
                                id: turn
                                interval: 60 + entry.index * 35
                                onTriggered: {
                                    rising.to = 0
                                    showing.restart()
                                }
                            }
                            Connections {
                                target: dlg
                                function onDealHistory() {
                                    showing.stop()
                                    entry.opacity = 0
                                    rising.snap(14)
                                    turn.restart()
                                }
                            }
                            T {
                                x: 16
                                width: parent.width - stats.implicitWidth - 44
                                anchors.verticalCenter: parent.verticalCenter
                                text: modelData.name || ""
                                type: "bodyMedium"
                            }
                            T {
                                id: stats
                                anchors.right: parent.right
                                anchors.rightMargin: 16
                                anchors.verticalCenter: parent.verticalCenter
                                text: Ui.tr("%1 мс", "%1 ms").arg(Math.round(modelData.ping || 0)) + " · ↓ " + App.mbps(modelData.download || 0) + " · ↑ " + App.mbps(modelData.upload || 0)
                                type: "labelLarge"
                                color: Theme.onSurfaceVariant
                            }
                        }
                    }
                }
            }
        }
    }

    buttons: [
        Btn {
            visible: dlg.history.length > 0 && !dlg.running
            kind: "text"
            text: Ui.tr("Очистить историю", "Clear history")
            onClicked: {
                App.clearSpeedHistory()
                dlg.close()
            }
        },
        Btn {
            kind: "text"
            text: Ui.tr("Закрыть", "Close")
            onClicked: dlg.close()
        },
        Btn {
            kind: dlg.running ? "tonal" : "filled"
            text: dlg.running ? Ui.tr("Остановить", "Stop") : (dlg.done || dlg.run.error) ? Ui.tr("Ещё раз", "Again") : Ui.tr("Начать", "Start")
            onClicked: dlg.running ? App.stopSpeed() : App.startSpeed()
        }
    ]
}
