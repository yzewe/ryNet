import QtQuick

Dialog {
    id: dlg

    property string remarks
    property string choice
    property bool custom: false
    readonly property bool valid: App.validDns(field.text)

    function openFor(r) {
        remarks = r
        const current = App.serverDns(r)
        const preset = Ui.serverDnsPreset(current)
        custom = current !== "" && !preset
        choice = custom ? "" : current
        field.text = custom ? current : ""
        opening.snap(custom ? 1 : 0)
        show()
    }

    icon: "language"
    title: Ui.tr("DNS для «%1»", "DNS for \"%1\"").arg((App.rev < 0 ? undefined : App.server(remarks).name || ""))
    Component.onCompleted: if ("preferredWidth" in dlg) dlg.preferredWidth = 400

    Column {
        width: parent.width
        Radio {
            width: parent.width
            text: Ui.tr("Как в настройках", "As in settings")
            checked: !dlg.custom && dlg.choice === ""
            onClicked: { dlg.custom = false; dlg.choice = "" }
        }
        Repeater {
            model: Ui.serverDnsPresets
            Radio {
                required property var modelData
                width: parent.width
                text: modelData[0]
                checked: !dlg.custom && dlg.choice === modelData[1]
                onClicked: { dlg.custom = false; dlg.choice = modelData[1] }
            }
        }
        Radio {
            width: parent.width
            text: Ui.tr("Свой", "Custom")
            checked: dlg.custom
            onClicked: {
                dlg.custom = true
                field.input.forceActiveFocus()
            }
        }
        Item {
            id: drawer
            property real open: 0
            width: parent.width
            height: (field.implicitHeight + 8) * Math.max(0, Math.min(1, open))
            clip: true
            Spring on open {
                id: opening
                to: dlg.custom ? 1 : 0
                stiffness: dlg.custom ? M.spatialK : M.effectsK
                damping: dlg.custom ? 0.8 : 1
            }
            Field {
                id: field
                y: 8 - 12 * (1 - drawer.open)
                width: parent.width
                opacity: Math.max(0, Math.min(1, drawer.open * 1.6 - 0.4))
                mono: true
                bg: Theme.surfaceContainerHigh
                placeholder: "https://dns.example/dns-query"
                error: text.trim() !== "" && !dlg.valid ? Ui.tr("Неверный адрес", "Invalid address") : ""
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
            text: Ui.tr("Готово", "Done")
            enabled: !dlg.custom || dlg.valid
            onClicked: {
                App.setServerDns(dlg.remarks, dlg.custom ? field.text.trim() : dlg.choice)
                dlg.close()
            }
        }
    ]
}
