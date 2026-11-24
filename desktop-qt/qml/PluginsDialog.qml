import QtQuick

Dialog {
    id: dlg

    property var plugins: []
    property var pending: null
    property string pendingFile: ""
    property var granted: ({})
    property string opened: ""
    property var logs: []
    property var values: ({})
    property string note: ""
    property bool noteError: false

    readonly property var current: plugins.find(p => p.manifest.id === opened) || null

    function say(text, error) { note = text || ""; noteError = !!error }
    function reload() {
        App.plugins("list", {}, "", (r) => {
            if (r.error) say(r.error, true)
            else plugins = r.result || []
        })
    }
    function openFor() {
        pending = null
        opened = ""
        say("")
        reload()
        show()
    }
    function kindLabel(m) {
        if (m.kind === "tunnel") return Ui.tr("Туннель", "Tunnel")
        if (m.kind === "transport") return m.carries === "datagram" ? Ui.tr("Транспорт датаграмм", "Datagram transport") : Ui.tr("Транспорт", "Transport")
        return Ui.tr("Протокол", "Protocol")
    }
    function text(t) { return !t ? "" : typeof t === "string" ? t : Ui.tr(t.ru || t.en || "", t.en || t.ru || "") }
    function dangerTitle(k, m) {
        return k === "net_any" ? Ui.tr("Любые адреса в интернете", "Any internet address")
            : k === "net_lan" ? Ui.tr("Локальная сеть", "Local network")
            : k === "memory" ? Ui.tr("Больше памяти: %1 МБ", "More memory: %1 MB").arg(m.permissions.memory_mb)
            : k === "keepalive" ? Ui.tr("Работа в простое", "Idle timers") : k
    }
    function dangerText(k) {
        return k === "net_any" ? Ui.tr("Плагин сможет отправлять данные на любой сервер, не только на свой", "The plugin can send data to any server, not only its own")
            : k === "net_lan" ? Ui.tr("Доступ к роутеру и устройствам в домашней сети", "Access to the router and devices on the home network")
            : k === "memory" ? Ui.tr("Может вытеснять другие программы из памяти", "May push other programs out of memory")
            : k === "keepalive" ? Ui.tr("Будит компьютер по таймеру", "Wakes the computer on timers") : ""
    }
    function apply(op, arg) {
        App.plugins(op, arg, "", (r) => {
            if (r.error) {
                say(r.error, true)
                return
            }
            say(Ui.tr("Применится при следующем подключении", "Takes effect on the next connection"))
            reload()
        })
    }
    function openPlugin(id) {
        opened = opened === id ? "" : id
        say("")
        const p = plugins.find(x => x.manifest.id === id)
        values = p ? Object.assign({}, p.global) : {}
        logs = []
        if (opened)
            App.plugins("logs", {id: id}, "", (r) => { logs = r.result || [] })
    }
    function installFrom(path) {
        openFor()
        inspect(path)
    }
    function inspect(path) {
        if (!path)
            return
        App.plugins("inspect", {}, path, (r) => {
            if (r.error) {
                say(Ui.tr("Это не плагин ryNet: ", "Not a ryNet plugin: ") + r.error, true)
                return
            }
            granted = ({})
            pendingFile = path
            pending = r.result
            say("")
        })
    }
    function install() {
        const asked = pending.installed ? pending.new_dangerous : pending.dangerous
        const old = plugins.find(x => x.manifest.id === pending.manifest.id)
        const denied = (old ? old.denied : []).concat(asked).filter((d, i, a) => !granted[d] && a.indexOf(d) === i)
        App.plugins("install", {denied: denied}, pendingFile, (r) => {
            if (r.error) {
                say(Ui.tr("Не удалось установить: ", "Couldn't install: ") + r.error, true)
                return
            }
            pending = null
            reload()
            openPlugin(r.result.manifest.id)
        })
    }

    icon: "terminal"
    title: pending ? (pending.installed ? Ui.tr("Обновить плагин", "Update plugin") : Ui.tr("Установить плагин", "Install plugin")) : Ui.tr("Плагины", "Plugins")
    Component.onCompleted: if ("preferredWidth" in dlg) dlg.preferredWidth = 520

    Column {
        width: parent.width
        spacing: 8

        T {
            visible: !dlg.pending
            width: parent.width
            text: Ui.tr("Протоколы и транспорты, которых нет в приложении. Плагин работает в песочнице: только сеть к своему серверу через правила и цепочки, остальное — по разрешениям.",
                        "Protocols and transports the app doesn't ship. A plugin runs in a sandbox: only network to its own server through rules and chains, anything else by permission.")
            type: "bodyMedium"
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
        }
        Group {
            visible: !dlg.pending && dlg.plugins.length > 0
            Repeater {
                model: dlg.pending ? [] : dlg.plugins
                SettingRow {
                    required property var modelData
                    icon: "terminal"
                    title: modelData.manifest.name + " " + modelData.manifest.version
                    summary: dlg.kindLabel(modelData.manifest) + " · "
                        + (modelData.diag.resting ? Ui.tr("отдыхает после сбоев", "resting after crashes")
                           : modelData.enabled ? modelData.manifest.protocols.join(", ") : Ui.tr("выключен", "off"))
                    tailIcon: "expand_more"
                    tailTurn: dlg.opened === modelData.manifest.id ? 180 : 0
                    onActivated: dlg.openPlugin(modelData.manifest.id)
                }
            }
        }

        Column {
            visible: !dlg.pending && dlg.current !== null
            width: parent.width
            spacing: 8
            Group {
                SettingRow {
                    icon: "power_settings_new"
                    title: Ui.tr("Включён", "Enabled")
                    summary: dlg.current ? dlg.text(dlg.current.manifest.description) : ""
                    checked: dlg.current ? dlg.current.enabled : false
                    onActivated: dlg.apply("enable", {id: dlg.opened, on: !dlg.current.enabled})
                }
                Repeater {
                    model: dlg.current ? dlg.current.dangerous : []
                    SettingRow {
                        required property var modelData
                        readonly property bool on: dlg.current.denied.indexOf(modelData) < 0
                        icon: "shield"
                        title: dlg.dangerTitle(modelData, dlg.current.manifest)
                        summary: dlg.dangerText(modelData)
                        checked: on
                        onActivated: dlg.apply("deny", {id: dlg.opened, denied: on ? dlg.current.denied.concat([modelData]) : dlg.current.denied.filter(d => d !== modelData)})
                    }
                }
            }
            Repeater {
                model: dlg.current ? dlg.current.manifest.global : []
                Item {
                    required property var modelData
                    width: parent.width
                    height: modelData.type === "bool" ? flag.height : field.height
                    Group {
                        id: flag
                        visible: modelData.type === "bool"
                        SettingRow {
                            icon: "tune"
                            title: dlg.text(modelData.label) || modelData.key
                            summary: dlg.text(modelData.help)
                            checked: !!dlg.values[modelData.key]
                            onActivated: { const v = Object.assign({}, dlg.values); v[modelData.key] = !v[modelData.key]; dlg.values = v }
                        }
                    }
                    Field {
                        id: field
                        visible: modelData.type !== "bool"
                        width: parent.width
                        label: (dlg.text(modelData.label) || modelData.key) + (modelData.required ? " *" : "")
                        placeholder: dlg.text(modelData.placeholder)
                        supporting: dlg.text(modelData.help) + (modelData.type === "list" ? Ui.tr(" (через запятую)", " (comma separated)") : "")
                        password: modelData.type === "secret"
                        mono: modelData.type === "secret" || modelData.type === "host"
                        bg: Theme.surfaceContainerHigh
                        text: { const v = dlg.values[modelData.key]; return v === undefined || v === null ? "" : Array.isArray(v) ? v.join(", ") : String(v) }
                        onTextChanged: {
                            if (!visible) return
                            const t = text.trim()
                            const v = Object.assign({}, dlg.values)
                            if (modelData.type === "int" || modelData.type === "port") v[modelData.key] = t === "" ? undefined : Number(t)
                            else if (modelData.type === "list") v[modelData.key] = t.split(",").map(s => s.trim()).filter(s => s !== "")
                            else v[modelData.key] = t === "" ? undefined : t
                            dlg.values = v
                        }
                    }
                }
            }
            Btn {
                visible: dlg.current !== null && dlg.current.manifest.global.length > 0
                kind: "tonal"
                text: Ui.tr("Сохранить настройки", "Save settings")
                onClicked: {
                    const v = {}
                    for (const k in dlg.values) if (dlg.values[k] !== undefined) v[k] = dlg.values[k]
                    dlg.apply("global", {id: dlg.opened, values: v})
                }
            }
            T {
                width: parent.width
                text: dlg.current ? [
                    Ui.tr("Соединений: %1", "Connections: %1").arg(dlg.current.diag.connections),
                    dlg.current.diag.traps > 0 ? Ui.tr("Сбоев: %1", "Crashes: %1").arg(dlg.current.diag.traps) : "",
                    dlg.current.diag.last_error ? Ui.tr("Последняя ошибка: ", "Last error: ") + dlg.current.diag.last_error : "",
                    "id: " + dlg.current.manifest.id + " · SHA-256 " + dlg.current.sha256.substring(0, 16) + "…",
                ].filter(s => s !== "").join("\n") : ""
                type: "bodySmall"
                color: Theme.onSurfaceVariant
                wrapMode: Text.Wrap
            }
            T {
                width: parent.width
                text: dlg.logs.length === 0 ? Ui.tr("Журнал пуст", "The log is empty")
                    : dlg.logs.slice(-60).reverse().map(l => new Date(l.time).toLocaleTimeString(Qt.locale(), "HH:mm:ss") + " " + ["", "E", "W", "I", "D"][l.level] + " " + l.message).join("\n")
                type: "bodySmall"
                font.family: "monospace"
                color: Theme.onSurfaceVariant
                wrapMode: Text.WrapAnywhere
            }
            Row {
                spacing: 8
                Btn {
                    kind: "text"
                    text: Ui.tr("Очистить журнал", "Clear the log")
                    onClicked: App.plugins("clear_logs", {id: dlg.opened}, "", () => { dlg.logs = [] })
                }
                Btn {
                    kind: "danger"
                    text: Ui.tr("Удалить", "Remove")
                    onClicked: removeAsk.show()
                }
            }
        }

        Column {
            visible: dlg.pending !== null
            width: parent.width
            spacing: 6
            T {
                width: parent.width
                text: dlg.pending ? [
                    dlg.pending.manifest.name + " " + dlg.pending.manifest.version + (dlg.pending.installed ? Ui.tr(" (сейчас %1)", " (installed: %1)").arg(dlg.pending.installed) : ""),
                    dlg.pending.manifest.author,
                    dlg.text(dlg.pending.manifest.description),
                    dlg.kindLabel(dlg.pending.manifest) + ": " + dlg.pending.manifest.protocols.join(", "),
                    dlg.pending.manifest.schemes.length ? Ui.tr("Ссылки: ", "Links: ") + dlg.pending.manifest.schemes.map(s => s + "://").join(", ") : "",
                    dlg.pending.manifest.permissions.hosts.length ? Ui.tr("Ходит также на: ", "Also connects to: ") + dlg.pending.manifest.permissions.hosts.join(", ") : "",
                    Math.round(dlg.pending.size / 1024) + " KB · SHA-256 " + dlg.pending.sha256.substring(0, 16) + "…",
                ].filter(s => s !== "").join("\n") : ""
                type: "bodyMedium"
                wrapMode: Text.Wrap
            }
            T {
                visible: dlg.pending !== null && dlg.pending.conflicts.length > 0
                width: parent.width
                text: dlg.pending ? Ui.tr("Те же протоколы уже у: %1. Новый плагин будет выключен", "The same protocols are already handled by: %1. The new plugin stays off").arg(dlg.pending.conflicts.join(", ")) : ""
                type: "bodyMedium"
                color: Theme.error
                wrapMode: Text.Wrap
            }
            Header {
                visible: dlg.pending !== null && (dlg.pending.installed ? dlg.pending.new_dangerous : dlg.pending.dangerous).length > 0
                text: Ui.tr("Опасные разрешения", "Dangerous permissions")
            }
            Group {
                Repeater {
                    model: dlg.pending ? (dlg.pending.installed ? dlg.pending.new_dangerous : dlg.pending.dangerous) : []
                    SettingRow {
                        required property var modelData
                        icon: "shield"
                        title: dlg.dangerTitle(modelData, dlg.pending.manifest)
                        summary: dlg.dangerText(modelData)
                        checked: !!dlg.granted[modelData]
                        onActivated: { const g = Object.assign({}, dlg.granted); g[modelData] = !g[modelData]; dlg.granted = g }
                    }
                }
            }
        }

        T {
            visible: dlg.note !== ""
            width: parent.width
            text: dlg.note
            type: "bodyMedium"
            color: dlg.noteError ? Theme.error : Theme.primary
            wrapMode: Text.Wrap
        }
    }

    ConfirmDialog {
        id: removeAsk
        title: dlg.current ? Ui.tr("Удалить %1?", "Remove %1?").arg(dlg.current.manifest.name) : ""
        text: Ui.tr("Серверы с этим протоколом перестанут подключаться", "Servers with this protocol will stop connecting")
        onAccepted: App.plugins("remove", {id: dlg.opened}, "", (r) => {
            if (r.error) {
                dlg.say(r.error, true)
                return
            }
            dlg.opened = ""
            dlg.reload()
        })
    }

    buttons: [
        Btn {
            kind: "text"
            text: dlg.pending ? Ui.tr("Отмена", "Cancel") : Ui.tr("Закрыть", "Close")
            onClicked: dlg.pending ? (dlg.pending = null) : dlg.close()
        },
        Btn {
            kind: "text"
            text: dlg.pending ? (dlg.pending.installed ? Ui.tr("Обновить", "Update") : Ui.tr("Установить", "Install")) : Ui.tr("Установить из файла", "Install from a file")
            onClicked: dlg.pending ? dlg.install() : App.pickFile(false, "", Ui.tr("Плагин ryNet", "ryNet plugin"), "rnp", (path) => dlg.inspect(path))
        }
    ]
}
