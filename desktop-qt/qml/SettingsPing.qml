import QtQuick

Column {
    id: sec
    readonly property var s: App.settings

    Header {
        first: true
        text: Ui.tr("Замер", "Measuring")
    }
    Group {
        DropdownRow {
            icon: "speed"
            title: Ui.tr("Способ", "Method")
            options: App.windows ? ["tcp", "proxy", "proxy2"] : ["tcp", "icmp", "proxy", "proxy2"]
            labels: App.windows ? ["TCP", Ui.tr("Прокси", "Proxy"), Ui.tr("Прокси ×2", "Proxy ×2")] : ["TCP", "ICMP", Ui.tr("Прокси", "Proxy"), Ui.tr("Прокси ×2", "Proxy ×2")]
            selected: sec.s.pingMode
            onPicked: (v) => App.set("pingMode", v)
        }
        SettingRow {
            icon: "bolt"
            title: Ui.tr("Пинг при запуске", "Ping on startup")
            summary: Ui.tr("Проверять серверы при открытии приложения", "Check servers when the app opens")
            checked: sec.s.pingOnStart
            onActivated: App.set("pingOnStart", !sec.s.pingOnStart)
        }
        SettingRow {
            icon: "travel_explore"
            title: Ui.tr("Адрес проверки", "Test URL")
            summary: sec.s.pingUrl !== "" ? sec.s.pingUrl : Ui.tr("generate_204\u00a0· по\u00a0умолчанию", "generate_204\u00a0· default")
            onActivated: url.edit(Ui.tr("Адрес проверки", "Test URL"), "pingUrl")
        }
        SettingRow {
            icon: "public"
            title: Ui.tr("Адрес проверки IP", "IP check URL")
            summary: (sec.s.ipUrl || "") !== "" ? sec.s.ipUrl : Ui.tr("ipwho.is\u00a0· по\u00a0умолчанию", "ipwho.is\u00a0· default")
            onActivated: ip.edit(Ui.tr("Адрес проверки IP", "IP check URL"))
        }
    }
    StepBlock {
        title: Ui.tr("Одновременно", "Parallel")
        steps: [2, 4, 6, 8, 12, 16, 24, 32]
        selected: sec.s.pingParallel
        onPicked: (v) => App.set("pingParallel", v)
    }
    StepBlock {
        title: Ui.tr("Таймаут", "Timeout")
        steps: [2, 3, 4, 5, 6, 8, 10, 12]
        selected: sec.s.pingTimeout
        label: (n) => Ui.tr(n + "\u00a0с", n + "\u00a0s")
        tick: (n) => String(n)
        onPicked: (v) => App.set("pingTimeout", v)
    }

    Header { text: Ui.tr("Проверка соединения", "Connection check") }
    Group {
        SettingRow {
            icon: "favorite"
            title: Ui.tr("Проверять сервер", "Check server")
            summary: Ui.tr("Следить, что подключённый сервер отвечает", "Watch that the connected server responds")
            checked: sec.s.healthCheck
            onActivated: App.set("healthCheck", !sec.s.healthCheck)
        }
    }
    Fold {
        open: sec.s.healthCheck === true
        StepBlock {
            title: Ui.tr("Раз в", "Every")
            steps: [3, 5, 10, 15, 30, 60, 120, 300, 600, 3600]
            selected: sec.s.healthSec
            label: (n) => n < 60 ? Ui.tr(n + "\u00a0с", n + "\u00a0s") : Ui.tr(n / 60 + "\u00a0мин", n / 60 + "\u00a0min")
            tick: (n) => n < 60 ? Ui.tr(n + "с", n + "s") : Ui.tr(n / 60 + "м", n / 60 + "m")
            onPicked: (v) => App.set("healthSec", v)
        }
        ChoiceBlock {
            title: Ui.tr("Если не отвечает", "If not responding")
            options: [false, true]
            labels: [Ui.tr("Переподключиться", "Reconnect"), Ui.tr("Сменить сервер", "Switch server")]
            selected: sec.s.autoSwitch === true
            onPicked: (v) => App.set("autoSwitch", v)
        }
        Group {
            SettingRow {
                icon: "notifications"
                title: Ui.tr("Уведомлять об отключении", "Notify on disconnect")
                summary: Ui.tr("Уведомление, если сервер перестал отвечать", "A notification if the server stops responding")
                checked: sec.s.healthNotify
                onActivated: App.set("healthNotify", !sec.s.healthNotify)
            }
        }
    }

    EditDialog {
        id: url
        placeholder: "http://www.gstatic.com/generate_204"
        setting: "pingUrl"
    }
    EditDialog {
        id: ip
        placeholder: "https://api.ipify.org"
        setting: "ipUrl"
        resettable: true
        hint: Ui.tr("Ответ: JSON с полем ip или просто IP-адрес текстом", "Reply: JSON with an ip field, or a plain IP address")
    }
}
