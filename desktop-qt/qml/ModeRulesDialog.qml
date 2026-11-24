import QtQuick

Dialog {
    id: dlg

    property var entries: []
    property string rest
    property var privateNets: []
    property var counts: ({})
    property var expanded: ({})
    readonly property var groups: build(entries)

    function view(which) {
        const r = App.modeRules(which)
        title = r.title || Ui.tr("Правила подписки", "Subscription rules")
        entries = r.entries || []
        rest = r.rest || ""
        privateNets = r["private"] || []
        expanded = ({})
        counts = ({})
        const geo = entries.filter(e => e.kind === "geosite" || e.kind === "geoip").map(e => ({ kind: e.kind, name: e.value }))
        if (geo.length > 0)
            App.geoCounts(geo, (n) => {
                const out = {}
                for (let i = 0; i < geo.length && i < n.length; ++i)
                    out[geo[i].kind + ":" + geo[i].name] = n[i]
                dlg.counts = out
            })
        show()
    }

    function build(list) {
        const sections = [
            { name: Ui.tr("Сайты", "Sites"), kinds: ["suffix", "full", "keyword", "regex"] },
            { name: Ui.tr("Категории сайтов", "Site categories"), kinds: ["geosite"] },
            { name: Ui.tr("Сети", "Networks"), kinds: ["cidr", "geoip"] },
            { name: Ui.tr("Приложения", "Apps"), kinds: ["app"] },
            { name: Ui.tr("Протоколы", "Protocols"), kinds: ["protocol"] }
        ]
        const out = []
        for (const action of ["block", "proxy", "direct"]) {
            const own = list.filter(e => e.action === action)
            if (own.length === 0)
                continue
            const secs = []
            for (const s of sections) {
                const items = own.filter(e => s.kinds.indexOf(e.kind) >= 0)
                if (items.length > 0)
                    secs.push({ key: action + ":" + s.kinds[0], name: s.name, items: items })
            }
            out.push({ action: action, sections: secs })
        }
        return out
    }

    function link(e) {
        return Ui.geoPage(e.kind, e.value)
    }

    preferredWidth: 520
    icon: "push_pin"

    Scroll {
        id: scroll
        width: parent.width
        height: Math.min(Math.max(200, Ui.overlay.height - 260), body.implicitHeight)
        Column {
            id: body
            width: scroll.width - 8
            spacing: 0
            T {
                width: parent.width
                text: Ui.tr("Встроенный набор, изменить нельзя", "Built-in set, can't be changed")
                type: "bodySmall"
                color: Theme.onSurfaceVariant
            }
            Repeater {
                model: dlg.groups
                Column {
                    id: group
                    required property var modelData
                    width: body.width
                    Header {
                        text: group.modelData.action === "block" ? Ui.tr("Блок", "Block")
                            : group.modelData.action === "direct" ? Ui.tr("Напрямую", "Direct") : Ui.tr("Прокси", "Proxy")
                    }
                    Repeater {
                        model: group.modelData.sections
                        Column {
                            id: section
                            required property var modelData
                            readonly property bool all: dlg.expanded[modelData.key] === true
                            readonly property var shown: all ? modelData.items : modelData.items.slice(0, 30)
                            readonly property bool more: !all && modelData.items.length > 30
                            readonly property int n: shown.length + (more ? 1 : 0)
                            width: group.width
                            spacing: Ui.gap
                            T {
                                leftPadding: 4
                                topPadding: 4
                                bottomPadding: 6
                                text: section.modelData.name + " · " + section.modelData.items.length
                                type: "labelLarge"
                                color: Theme.onSurfaceVariant
                            }
                            Repeater {
                                model: section.shown
                                Surface {
                                    id: entry
                                    required property var modelData
                                    required property int index
                                    readonly property bool geo: modelData.kind === "geosite" || modelData.kind === "geoip"
                                    readonly property var count: dlg.counts[modelData.kind + ":" + modelData.value]
                                    readonly property bool isPrivate: modelData.kind === "geoip" && modelData.value === "private" && dlg.privateNets.length > 0
                                    width: section.width
                                    height: Math.max(52, texts.implicitHeight + 24)
                                    color: Theme.surfaceContainerHighest
                                    topLeftRadius: index === 0 ? Ui.outer : Ui.inner
                                    topRightRadius: topLeftRadius
                                    bottomLeftRadius: index === section.n - 1 ? Ui.outer : Ui.inner
                                    bottomRightRadius: bottomLeftRadius
                                    interactive: geo
                                    Accessible.name: modelData.text
                                    onClicked: Qt.openUrlExternally(dlg.link(modelData))
                                    Column {
                                        id: texts
                                        x: 16
                                        width: parent.width - 32 - (countText.visible ? countText.width + 12 : 0)
                                        anchors.verticalCenter: parent.verticalCenter
                                        T {
                                            width: parent.width
                                            text: entry.modelData.text
                                            type: "bodyLarge"
                                            color: Theme.onSurface
                                            elide: Text.ElideMiddle
                                        }
                                        T {
                                            visible: entry.isPrivate
                                            width: parent.width
                                            text: dlg.privateNets.join(", ")
                                            type: "bodySmall"
                                            color: Theme.onSurfaceVariant
                                            wrapMode: Text.Wrap
                                        }
                                    }
                                    T {
                                        id: countText
                                        visible: entry.geo && entry.count !== undefined
                                        anchors.right: parent.right
                                        anchors.rightMargin: 16
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: entry.count === undefined ? "" : entry.count < 0 ? Ui.tr("не загружен", "not loaded")
                                            : App.plural(entry.count, "запись", "записи", "записей", "entry", "entries")
                                        type: "labelLarge"
                                        color: Theme.onSurfaceVariant
                                    }
                                }
                            }
                            Surface {
                                visible: section.more
                                width: section.width
                                height: 48
                                color: Theme.surfaceContainerHighest
                                topLeftRadius: Ui.inner
                                topRightRadius: Ui.inner
                                bottomLeftRadius: Ui.outer
                                bottomRightRadius: Ui.outer
                                interactive: true
                                Accessible.name: allText.text
                                onClicked: {
                                    const e = Object.assign({}, dlg.expanded)
                                    e[section.modelData.key] = true
                                    dlg.expanded = e
                                }
                                T {
                                    id: allText
                                    x: 16
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: Ui.tr("Показать все (%1)", "Show all (%1)").arg(section.modelData.items.length)
                                    type: "labelLarge"
                                    color: Theme.primary
                                }
                            }
                        }
                    }
                }
            }
            Header {
                visible: dlg.rest !== ""
                text: Ui.tr("Остальное", "Everything else")
            }
            ActionTag {
                visible: dlg.rest !== ""
                action: dlg.rest || "proxy"
            }
            Item { width: 1; height: 8 }
        }
    }

    buttons: [Btn { kind: "text"; text: Ui.tr("Закрыть", "Close"); onClicked: dlg.close() }]
}
