import QtQuick

Item {
    id: all

    property string query
    property var bySub: ({})
    property int rev: 0
    readonly property var s: App.settings
    readonly property bool wide: s.grid === "1x1"
    readonly property int cols: Math.max(1, Math.floor((width + 2 * 6 + 8) / ((wide ? 420 : 240) + 8)))

    function load() {
        bySub = ({})
        for (const sub of App.subs)
            App.loadServersOf(sub.id)
    }
    Component.onCompleted: load()
    Connections {
        target: App
        function onServersOfReady(id, servers) {
            const m = all.bySub
            m[id] = servers
            all.bySub = m
            ++all.rev
        }
        function onSubsChanged() { all.load() }
    }

    function line(p) {
        if (s.serverInfo === "none")
            return ""
        if (s.serverInfo === "description" && (p.description || "") !== "")
            return p.description
        return p.kind || ""
    }
    readonly property var entries: {
        void rev
        void App.rev
        const out = []
        const q = query.trim().toLowerCase()
        for (const sub of App.subs) {
            const list = bySub[sub.id] || []
            for (const p of list) {
                if (p.supported !== true || p.placeholder === true)
                    continue
                if (q !== "" && ![p.name, p.kind, p.description].some(t => (t || "").toLowerCase().indexOf(q) >= 0))
                    continue
                const ping = App.pingIn(sub.id, p.remarks)
                out.push({ sub: sub.id, remarks: p.remarks, name: p.name, flag: p.flag, line: line(p), supported: true,
                           ping: ping.ms !== undefined ? ping.ms : -1, pingFail: ping.error !== undefined,
                           pinned: (s.pins && s.pins[sub.id] || []).indexOf(p.remarks) >= 0,
                           selected: !s.autoSelect && sub.id === App.currentSub && p.remarks === (s.profile || "") })
            }
        }
        if (s.sort === "ping")
            out.sort((a, b) => (a.ping >= 0 ? a.ping : 1e9) - (b.ping >= 0 ? b.ping : 1e9))
        else if (s.sort === "name")
            out.sort((a, b) => a.name.localeCompare(b.name))
        return out
    }
    readonly property var pinned: query.trim() === "" ? entries.filter(e => e.pinned) : []
    readonly property var rest: entries.filter(e => pinned.indexOf(e) < 0)

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + 96
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: column
            width: flick.width
            spacing: 8

            AutoTile {
                visible: all.s.autoFeature === true && all.query.trim() === ""
                width: parent.width
            }
            T {
                visible: all.pinned.length > 0
                topPadding: 4
                text: Ui.tr("Закреплённые · ", "Pinned · ") + all.pinned.length
                type: "labelLarge"
                color: Theme.primary
            }
            Flow {
                visible: all.pinned.length > 0
                width: parent.width
                spacing: 8
                Repeater {
                    model: all.pinned
                    Tile {
                        required property var modelData
                        width: Math.floor((column.width - 8 * (all.cols - 1)) / all.cols)
                        height: all.wide ? 60 : 52
                        info: Object.assign({ wide: all.wide }, modelData)
                        onPicked: App.autoChoose(modelData.sub, modelData.remarks)
                    }
                }
            }
            T {
                visible: all.pinned.length > 0 && all.rest.length > 0
                topPadding: 4
                text: Ui.tr("Все серверы · ", "All servers · ") + all.rest.length
                type: "labelLarge"
                color: Theme.primary
            }
            Flow {
                width: parent.width
                spacing: 8
                Repeater {
                    model: all.rest
                    Tile {
                        required property var modelData
                        width: Math.floor((column.width - 8 * (all.cols - 1)) / all.cols)
                        height: all.wide ? 60 : 52
                        info: Object.assign({ wide: all.wide }, modelData)
                        onPicked: App.autoChoose(modelData.sub, modelData.remarks)
                    }
                }
            }
            T {
                visible: all.query.trim() !== "" && all.entries.length === 0
                width: parent.width
                topPadding: 32
                horizontalAlignment: Text.AlignHCenter
                text: Ui.tr("Ничего не найдено", "Nothing found")
                type: "bodyLarge"
                color: Theme.onSurfaceVariant
            }
        }
    }

    component AutoTile: Surface {
        id: at
        readonly property bool active: all.s.autoSelect === true
        readonly property var pick: active ? App.server(all.s.profile || "") : ({})
        height: 60
        radius: M.lg
        interactive: true
        squish: true
        color: active ? Theme.secondaryContainer : Theme.surfaceContainerHigh
        onClicked: {
            if (active)
                App.nav = "home"
            else
                App.set("autoSelect", true)
        }
        Rectangle {
            id: pad
            x: 8
            anchors.verticalCenter: parent.verticalCenter
            width: 58
            height: 44
            radius: M.md
            color: at.active ? Theme.primary : Theme.primaryContainer
            Icon {
                anchors.centerIn: parent
                name: "autorenew"
                size: 24
                color: at.active ? Theme.onPrimary : Theme.onPrimaryContainer
            }
        }
        Column {
            anchors.left: pad.right
            anchors.leftMargin: 14
            anchors.right: parent.right
            anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            T {
                text: Ui.tr("Автовыбор", "Auto select")
                type: "titleMediumE"
                color: at.active ? Theme.onSecondaryContainer : Theme.onSurface
            }
            T {
                visible: text !== ""
                width: parent.width
                text: at.active && at.pick.name ? ((at.pick.flag ? at.pick.flag + " " : "") + at.pick.name) : ""
                type: "labelLarge"
                elide: Text.ElideRight
                color: at.active ? Theme.alpha(Theme.onSecondaryContainer, 0.72) : Theme.onSurfaceVariant
            }
        }
    }
}
