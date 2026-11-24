import QtQuick

Column {
    id: sec
    readonly property var s: App.settings
    readonly property var seeds: ["", "#0061a4", "#6750a4", "#b4236b", "#c4501f", "#5e7c16"]
    readonly property var seedNames: [["Бирюзовый", "Teal"], ["Синий", "Blue"], ["Фиолетовый", "Violet"], ["Розовый", "Rose"],
                                      ["Оранжевый", "Orange"], ["Зелёный", "Green"]]
    readonly property bool own: !Theme.systemColors
    readonly property bool custom: s.seedColor !== "" && seeds.indexOf(s.seedColor) < 0
    readonly property bool dark: Theme.dark

    ChoiceBlock {
        first: true
        title: Ui.tr("Тема", "Theme")
        options: ["system", "light", "dark", "amoled"]
        labels: [Ui.tr("Авто", "Auto"), Ui.tr("Светлая", "Light"), Ui.tr("Тёмная", "Dark"), "AMOLED"]
        selected: sec.s.theme
        onPicked: (v) => App.set("theme", v)
    }
    Group {
        SettingRow {
            icon: "wallpaper"
            title: App.windows ? Ui.tr("Цвет акцента Windows", "Windows accent color") : Ui.tr("Цвета обоев", "Wallpaper colors")
            summary: App.windows ? Ui.tr("Цвета берутся из акцента Windows", "Colors come from the Windows accent") : Ui.tr("Цвета берутся из обоев", "Colors come from the wallpaper")
            checked: sec.s.wallpaperColors
            onActivated: App.set("wallpaperColors", !sec.s.wallpaperColors)
        }
    }
    ChoiceBlock {
        title: Ui.tr("Цвет", "Color")
        Flow {
            width: parent.width
            spacing: 2
            Repeater {
                model: sec.seeds
                Swatch {
                    required property string modelData
                    required property int index
                    colors: Theme.swatch(modelData, sec.dark)
                    selected: sec.own && sec.s.seedColor === modelData
                    Accessible.name: Ui.tr(sec.seedNames[index][0], sec.seedNames[index][1])
                    onClicked: {
                        if (selected)
                            return
                        App.setSeedColor(modelData)
                    }
                }
            }
            Swatch {
                id: customSwatch
                colors: sec.custom ? Theme.swatch(sec.s.seedColor, sec.dark) : []
                icon: "add"
                selected: sec.own && sec.custom
                Accessible.name: Ui.tr("Свой", "Custom")
                onClicked: {
                    play()
                    picker.edit(sec.custom ? sec.s.seedColor : sec.s.seedColor !== "" ? sec.s.seedColor
                                : Theme.systemColors ? Theme.hct(Theme.hueOf(Theme.primary), 48, 50).toString() : "#0b6b57")
                }
            }
        }
    }
    ChoiceBlock {
        title: Ui.tr("Шрифт", "Font")
        options: ["roboto", "system"]
        labels: ["Roboto", Ui.tr("Системный", "System")]
        selected: sec.s.font
        onPicked: (v) => {
            App.set("font", v)
        }
    }
    ChoiceBlock {
        title: Ui.tr("Масштаб", "Scale")
        options: [0, 90, 110, 125, 150]
        labels: [Ui.tr("Авто", "Auto"), "90%", "110%", "125%", "150%"]
        selected: [0, 90, 110, 125, 150].indexOf(sec.s.uiScale) >= 0 ? sec.s.uiScale : 0
        onPicked: (v) => {
            App.set("uiScale", v)
        }
    }
    StepBlock {
        title: Ui.tr("Скругление секций", "Section corners")
        steps: [0, 4, 8, 12, 16, 20, 24, 28]
        selected: sec.s.sectionRadius
        label: (n) => n + "\u00a0px"
        tick: (n) => String(n)
        live: true
        onPicked: (v) => App.set("sectionRadius", v)
    }
    ChoiceBlock {
        title: Ui.tr("Разделитель", "Divider")
        options: ["none", "gap"]
        labels: [Ui.tr("Без разделителя", "None"), Ui.tr("Зазор", "Gap")]
        selected: sec.s.sectionDivider
        onPicked: (v) => App.set("sectionDivider", v)
    }

    Header { text: Ui.tr("Работа", "Behavior") }
    Group {
        SettingRow {
            icon: "power_settings_new"
            title: Ui.tr("Запуск при входе в систему", "Start at sign-in")
            summary: Ui.tr("Подключаться сразу после входа в систему", "Connect right after signing in")
            checked: sec.s.autostart
            onActivated: App.setAutostart(!sec.s.autostart)
        }
        SettingRow {
            icon: "notifications"
            title: Ui.tr("Уведомления", "Notifications")
            summary: Ui.tr("Обрыв связи, смена сервера и конец подписки", "A dropped connection, a server switch and the subscription's end")
            checked: sec.s.notifications
            onActivated: App.set("notifications", !sec.s.notifications)
        }
    }

    Header { text: Ui.tr("Обновления", "Updates") }
    UpdateCard {}
    Group {
        SettingRow {
            icon: "refresh"
            title: Ui.tr("Проверить обновления", "Check for updates")
            summary: App.update.state === "checking" ? Ui.tr("Проверка…", "Checking…") : Ui.tr("Установлена версия ", "Installed version ") + App.version
            enabled: App.update.state !== "checking"
            onActivated: App.checkUpdate()
        }
        SettingRow {
            icon: "sync"
            title: Ui.tr("Проверять автоматически", "Check automatically")
            summary: Ui.tr("Раз в день", "Once a day")
            checked: sec.s.updateCheck
            onActivated: App.set("updateCheck", !sec.s.updateCheck)
        }
    }

    Header { text: Ui.tr("Резервная копия", "Backup") }
    Group {
        SettingRow {
            icon: "upload"
            title: Ui.tr("Экспорт настроек", "Export settings")
            summary: Ui.tr("Сохранить настройки в файл", "Save settings to a file")
            onActivated: {
                parts.file = ""
                parts.ask(Ui.tr("Экспорт настроек", "Export settings"), Ui.tr("Сохранить", "Save"), App.backupParts())
            }
        }
        SettingRow {
            icon: "download"
            title: Ui.tr("Импорт настроек", "Import settings")
            summary: Ui.tr("Восстановить настройки из файла", "Restore settings from a file")
            onActivated: {
                if (App.canImport())
                    App.pickFile(false, "", Ui.tr("Импорт настроек", "Import settings"), "rynet", (path) => {
                        const found = path ? App.backupFileParts(path) : []
                        if (found.length > 0) {
                            parts.file = path
                            parts.ask(Ui.tr("Импорт настроек", "Import settings"), Ui.tr("Применить", "Apply"), found)
                        }
                    })
            }
        }
    }

    Header { text: Ui.tr("Диагностика", "Diagnostics") }
    Group {
        SettingRow {
            icon: "bug_report"
            title: Ui.tr("Подробный журнал", "Verbose log")
            summary: Ui.tr("Больше подробностей в журнале, чтобы найти ошибку", "More detail in the log to track down a problem")
            checked: sec.s.debugLog
            onActivated: App.setDebugLog(!sec.s.debugLog)
        }
        SettingRow {
            icon: "upload"
            title: Ui.tr("Отправлять логи разработчику", "Send logs to the developer")
            summary: Ui.tr("Отчёты о сбоях помогут быстрее их исправить", "Crash reports help get them fixed sooner")
            checked: sec.s.sendLogs
            onActivated: sec.s.sendLogs ? App.setSendLogs(false) : ask.show()
        }
    }

    T {
        width: parent.width
        topPadding: 28
        horizontalAlignment: Text.AlignHCenter
        text: "ryNet " + App.version
        type: "bodySmall"
        color: Theme.onSurfaceVariant
    }

    ColorPicker {
        id: picker
        save: (c) => App.setSeedColor(c)
    }
    ConfirmDialog {
        id: ask
        icon: "upload"
        danger: false
        title: Ui.tr("Отправлять логи разработчику?", "Send logs to the developer?")
        text: Ui.tr("Включится подробный журнал, и каждые 2 часа работы VPN он будет уходить на api.rynet.app. В нём адреса сайтов и серверов, к которым шли подключения, и версия системы. Ключей и ссылок подписок там нет.",
                    "The verbose log turns on and is sent to api.rynet.app every 2 hours of VPN use. It holds the addresses of sites and servers that were connected to and the system version. No keys or subscription links.")
        confirm: Ui.tr("Включить", "Turn on")
        onAccepted: App.setSendLogs(true)
    }
    BackupDialog {
        id: parts
        property string file
        onPicked: (keys) => {
            if (file === "")
                App.pickFile(true, App.backupName(), Ui.tr("Экспорт настроек", "Export settings"), "rynet", (path) => App.exportBackup(path, keys))
            else
                App.importBackup(file, keys)
        }
    }
}
