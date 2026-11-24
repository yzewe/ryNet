import QtQuick

Column {
    id: sec
    readonly property var s: App.settings
    readonly property bool on: s.autoFeature === true
    property var groups: []
    property var transports: []
    readonly property int skipped: Ui.countEntries(s.autoSkip || {})
    readonly property var transportChoice: Ui.merged(transports, s.autoTransports || [])

    function toggled(list, item) {
        return Ui.toggled(list || [], item)
    }
    function marked() {
        const subs = (s.autoWlSubs || []).length
        const servers = Ui.countEntries(s.autoWl || {})
        return Ui.joined([subs > 0 ? App.plural(subs, "подписка", "подписки", "подписок", "subscription") : "",
                          servers > 0 ? App.plural(servers, "сервер", "сервера", "серверов", "server") : ""], ", ")
    }
    function hostList(text) {
        return Ui.hostList(text)
    }

    Component.onCompleted: App.loadAutoServers()
    Connections {
        target: App
        function onAutoServersReady(groups, transports) {
            sec.groups = groups
            sec.transports = transports
        }
    }

    Group {
        SettingRow {
            icon: "swap_horiz"
            title: Ui.tr("Умный автовыбор", "Smart auto select")
            summary: Ui.tr("Сам выбирает рабочий быстрый сервер", "Picks a working, fast server for you")
            checked: sec.on
            onActivated: {
                const next = !sec.on
                App.set("autoFeature", next)
                App.set("autoSelect", next)
            }
        }
    }

    Header { text: Ui.tr("Где искать", "Where to look") }
    Flow {
        visible: App.subs.length > 1
        width: parent.width
        spacing: 6
        bottomPadding: 12
        Repeater {
            model: App.subs
            Chip {
                required property var modelData
                readonly property bool use: (sec.s.autoSkipSubs || []).indexOf(modelData.id) < 0
                text: modelData.name || modelData.host || ""
                selected: use
                enabled: sec.on
                onClicked: App.set("autoSkipSubs", sec.toggled(sec.s.autoSkipSubs, modelData.id))
            }
        }
    }
    Group {
        SettingRow {
            icon: "block"
            title: Ui.tr("Не брать", "Never use")
            summary: sec.skipped > 0 ? App.plural(sec.skipped, "сервер", "сервера", "серверов", "server")
                : Ui.tr("Серверы, которые автовыбор пропускает", "Servers auto select skips")
            enabled: sec.on
            onActivated: lists.ask(false)
        }
        SettingRow {
            icon: "push_pin"
            title: Ui.tr("Закреплённые первыми", "Pinned first")
            summary: Ui.tr("Закреплённые серверы выбираются чаще", "Pinned servers are picked more often")
            checked: sec.s.autoPins
            enabled: sec.on
            onActivated: App.set("autoPins", !sec.s.autoPins)
        }
    }
    Column {
        visible: sec.transportChoice.length > 1 || (sec.s.autoTransports || []).length > 0
        width: parent.width
        Header { text: Ui.tr("Предпочитать транспорт", "Prefer transport") }
        Flow {
            width: parent.width
            spacing: 6
            Repeater {
                model: sec.transportChoice
                Chip {
                    required property string modelData
                    text: modelData
                    selected: (sec.s.autoTransports || []).indexOf(modelData) >= 0
                    enabled: sec.on
                    onClicked: App.set("autoTransports", sec.toggled(sec.s.autoTransports, modelData))
                }
            }
        }
    }

    ChoiceBlock {
        title: Ui.tr("Белые списки", "Whitelists")
        options: ["auto", "always", "never"]
        labels: [Ui.tr("Авто", "Auto"), Ui.tr("Всегда", "Always"), Ui.tr("Никогда", "Never")]
        summaries: [Ui.tr("Определять по тому, какие сайты открываются\u00A0напрямую", "Detect by which sites open directly"),
                    Ui.tr("Считать, что белые списки включены всегда", "Always assume whitelists are on"),
                    Ui.tr("Считать интернет всегда обычным", "Always assume open internet")]
        selected: sec.s.autoWhitelist
        opacity: sec.on ? 1 : 0.38
        enabled: sec.on
        onPicked: (v) => App.set("autoWhitelist", v)
    }
    Group {
        SettingRow {
            icon: "flag"
            title: Ui.tr("Для белых списков", "For whitelists")
            summary: Ui.joined([Ui.tr("Проверяются первыми", "Tried first"), sec.marked()], "\n")
            enabled: sec.on
            onActivated: lists.ask(true)
        }
        SettingRow {
            visible: sec.s.autoWhitelist === "auto"
            icon: "home"
            title: Ui.tr("Свои сайты", "Home sites")
            summary: Ui.tr("Открыты и при белых списках", "Open even under whitelists") + "\n" + sec.hostList(sec.s.autoHome)
            enabled: sec.on
            onActivated: hosts.open(true)
        }
        SettingRow {
            visible: sec.s.autoWhitelist === "auto"
            icon: "public"
            title: Ui.tr("Зарубежные сайты", "Foreign sites")
            summary: Ui.tr("Закрыты при белых списках", "Blocked under whitelists") + "\n" + sec.hostList(sec.s.autoFree)
            enabled: sec.on
            onActivated: hosts.open(false)
        }
    }

    ChoiceBlock {
        title: Ui.tr("Проверять", "Check")
        enabled: sec.on
        opacity: sec.on ? 1 : 0.38
        options: [5, 15, 30, 0]
        labels: [Ui.tr("5 мин", "5 min"), Ui.tr("15 мин", "15 min"), Ui.tr("30 мин", "30 min"), Ui.tr("При сбое", "On failure")]
        selected: options.indexOf(sec.s.autoInterval) >= 0 ? sec.s.autoInterval : 15
        onPicked: (v) => App.set("autoInterval", v)
    }
    ChoiceBlock {
        title: Ui.tr("Менять, если быстрее на", "Switch if faster by")
        enabled: sec.on
        opacity: sec.on ? 1 : 0.38
        options: [30, 50, 100]
        labels: [Ui.tr("30 мс", "30 ms"), Ui.tr("50 мс", "50 ms"), Ui.tr("100 мс", "100 ms")]
        selected: options.indexOf(sec.s.autoMargin) >= 0 ? sec.s.autoMargin : 50
        onPicked: (v) => App.set("autoMargin", v)
    }
    Group {
        SettingRow {
            icon: "show_chart"
            title: Ui.tr("Учитывать скорость", "Count speed")
            summary: Ui.tr("При равном пинге берёт более быстрый", "With equal ping, takes the faster one")
            checked: sec.s.autoSpeed
            enabled: sec.on
            onActivated: App.set("autoSpeed", !sec.s.autoSpeed)
        }
        SettingRow {
            icon: "data_usage"
            title: Ui.tr("Беречь мобильный трафик", "Save mobile data")
            summary: Ui.tr("В мобильной сети проверок меньше", "Fewer checks on mobile data")
            checked: sec.s.autoSaveData
            enabled: sec.on
            onActivated: App.set("autoSaveData", !sec.s.autoSaveData)
        }
        SettingRow {
            icon: "notifications"
            title: Ui.tr("Сообщать о смене", "Notify on switch")
            summary: Ui.tr("Уведомление, когда автовыбор переключил сервер", "A notification when auto select switches the server")
            checked: sec.s.autoNotify
            enabled: sec.on
            onActivated: App.set("autoNotify", !sec.s.autoNotify)
        }
    }

    AutoListDialog {
        id: lists
        groups: sec.groups
    }

    EditDialog {
        id: hosts
        function open(h) {
            edit(h ? Ui.tr("Свои сайты", "Home sites") : Ui.tr("Зарубежные сайты", "Foreign sites"), h ? "autoHome" : "autoFree")
        }
        multiline: true
        allowEmpty: false
        placeholder: "yandex.ru\nvk.ru"
    }
}
