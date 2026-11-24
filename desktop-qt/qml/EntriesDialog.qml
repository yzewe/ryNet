import QtQuick

Dialog {
    id: dlg

    property string kind: "domains"
    property var suggestions: []
    signal added(var list)
    readonly property var result: (area.text, App.normalizeEntries(kind, area.text))

    function ask(k) {
        kind = k
        area.text = ""
        suggestions = k === "procs" ? App.processes() : []
        title = k === "domains" ? Ui.tr("Сайты", "Sites") : k === "procs" ? Ui.tr("Приложения", "Apps")
            : k === "geosite" ? Ui.tr("Категории geosite", "Geosite categories") : Ui.tr("Сети", "Networks")
        show()
        area.edit.forceActiveFocus()
    }

    preferredWidth: 440

    Column {
        width: parent.width
        spacing: 16
        Area {
            id: area
            width: parent.width
            height: 160
            placeholder: dlg.kind === "domains" ? "youtube.com\ndiscord.com" : dlg.kind === "procs" ? "firefox.exe\nTelegram.exe"
                : dlg.kind === "geosite" ? "github\ncategory-ads-all" : "10.0.0.0/8\n2001:db8::/32"
            error: dlg.result.total > 0 && !dlg.result.ok
        }
        Item {
            visible: dlg.suggestions.length > 0
            width: parent.width
            height: 32
            Flickable {
                x: -6
                y: -6
                width: parent.width + 12
                height: 44
                contentWidth: row.implicitWidth + 12
                flickableDirection: Flickable.HorizontalFlick
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                Row {
                    id: row
                    x: 6
                    y: 6
                    spacing: 8
                    Repeater {
                        model: Ui.suggest(dlg.suggestions, area.text, 30)
                        Chip {
                            required property string modelData
                            text: modelData
                            onClicked: {
                                const lines = area.text.split("\n")
                                lines[lines.length - 1] = modelData
                                area.text = lines.join("\n") + "\n"
                                area.edit.cursorPosition = area.text.length
                            }
                        }
                    }
                }
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: "text"
            text: dlg.result.good.length > 1 ? Ui.tr("Добавить · %1", "Add · %1").arg(dlg.result.good.length) : Ui.tr("Добавить", "Add")
            enabled: dlg.result.ok
            onClicked: {
                dlg.added(dlg.result.good)
                dlg.close()
            }
        }
    ]
}
