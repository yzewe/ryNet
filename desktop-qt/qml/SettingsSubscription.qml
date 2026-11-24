import QtQuick

Column {
    id: sec
    readonly property var s: App.settings

    Header {
        first: true
        text: Ui.tr("Обновление", "Updates")
    }
    Group {
        SettingRow {
            icon: "refresh"
            title: Ui.tr("Автообновление", "Auto-update")
            summary: Ui.tr("Подписка обновляется сама в фоне", "The subscription refreshes itself in the background")
            checked: sec.s.autoUpdate
            onActivated: App.set("autoUpdate", !sec.s.autoUpdate)
        }
        SettingRow {
            icon: "notifications"
            title: Ui.tr("Сообщения об обновлении", "Update notices")
            summary: Ui.tr("Сколько серверов пришло после обновления", "How many servers came after an update")
            checked: sec.s.refreshNotices === true
            onActivated: App.set("refreshNotices", !sec.s.refreshNotices)
        }
    }

    ChoiceBlock {
        title: Ui.tr("Представляться как", "Present as")
        options: ["rynet", "happ", "custom"]
        labels: ["ryNet", "Happ", Ui.tr("Своё", "Custom")]
        selected: sec.s.subClient
        onPicked: (v) => {
            App.set("subClient", v)
            if (v === "custom" && (sec.s.subHeaders || "").trim() === "")
                headers.edit(Ui.tr("Заголовки", "Headers"))
        }
    }
    Group {
        Fold {
            open: sec.s.subClient === "custom"
            SettingRow {
                icon: "code"
                title: Ui.tr("Заголовки", "Headers")
                summary: (sec.s.subHeaders || "").trim() !== "" ? sec.s.subHeaders.trim() : Ui.tr("Не заданы", "Not set")
                onActivated: headers.edit(Ui.tr("Заголовки", "Headers"))
            }
        }
        SettingRow {
            icon: "upload"
            title: Ui.tr("Отправлять HWID", "Send HWID")
            summary: Ui.tr("Провайдер подписки увидит устройство и учтёт в лимите", "The subscription provider sees the device and counts it in the limit")
            checked: sec.s.sendHwid
            onActivated: App.set("sendHwid", !sec.s.sendHwid)
        }
        SettingRow {
            icon: "fingerprint"
            title: "HWID"
            summary: (sec.s.hwid || "") !== "" ? sec.s.hwid : Ui.tr(App.hwidDefault + "\u00a0· по\u00a0умолчанию", App.hwidDefault + "\u00a0· default")
            onActivated: hwid.edit("HWID")
        }
    }

    EditDialog {
        id: headers
        multiline: true
        placeholder: "User-Agent: Happ/4.3.0\nX-Client: ryNet"
        setting: "subHeaders"
    }
    EditDialog {
        id: hwid
        placeholder: App.hwidDefault
        setting: "hwid"
    }
}
