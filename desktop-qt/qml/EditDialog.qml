import QtQuick

Dialog {
    id: dlg

    property string setting
    property string placeholder
    property bool multiline: false
    property bool allowEmpty: true
    property bool mono: true
    property bool happImport: false
    property string note
    property string hint
    property bool resettable: false
    readonly property string value: multiline ? area.text : field.text
    readonly property bool ok: (allowEmpty && value.trim() === "") || (value.trim() !== "" && App.fieldValid(setting, value.trim()))

    function edit(title, name) {
        if (name !== undefined)
            setting = name
        dlg.title = title
        field.text = App.fieldText(setting)
        area.text = field.text
        note = ""
        show()
        if (multiline)
            area.edit.forceActiveFocus()
        else
            field.input.forceActiveFocus()
    }

    maxWidth: 520
    preferredWidth: 440

    Column {
        id: importer
        width: parent.width
        spacing: 6
        visible: dlg.happImport
        Btn {
            id: happ
            width: parent.width
            kind: "outlined"
            text: Ui.tr("Импорт из Happ / v2RayTun (буфер)", "Import from Happ / v2RayTun (clipboard)")
            onClicked: {
                const input = App.clipboard().trim()
                if (input === "") {
                    dlg.note = Ui.tr("Буфер пуст: скопируйте ссылку профиля Happ или v2RayTun", "The clipboard is empty: copy a Happ or v2RayTun profile link")
                    return
                }
                happ.busy = true
                App.importRules(input, (r) => {
                    happ.busy = false
                    if (r.error !== undefined) {
                        dlg.note = r.error || Ui.tr("Не удалось разобрать профиль", "Couldn't parse the profile")
                        return
                    }
                    const had = area.text.replace(/\s+$/, "")
                    area.text = had === "" ? r.rules : had + "\n" + r.rules
                    dlg.note = r.skipped === 0 ? Ui.tr("Правила импортированы", "Rules imported")
                        : Ui.tr("Правила импортированы, пропущено записей: %1", "Rules imported, skipped entries: %1").arg(r.skipped)
                })
            }
        }
        T {
            visible: dlg.note !== ""
            width: parent.width
            text: dlg.note
            type: "bodySmall"
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
        }
        Item { width: 1; height: 6 }
    }
    Item {
        y: importer.visible ? importer.height : 0
        width: parent.width
        height: dlg.multiline ? area.height : field.implicitHeight
        Field {
            id: field
            visible: !dlg.multiline
            width: parent.width
            mono: dlg.mono
            bg: Theme.surfaceContainerHigh
            placeholder: dlg.placeholder
            error: !dlg.ok ? Ui.tr("Неверное значение", "Invalid value") : ""
            supporting: dlg.hint
            onAccepted: if (dlg.ok) { App.saveField(dlg.setting, dlg.value.trim()); dlg.close() }
        }
        Area {
            id: area
            visible: dlg.multiline
            width: parent.width
            height: 180
            placeholder: dlg.placeholder
            error: !dlg.ok
        }
    }

    buttons: [
        Btn {
            visible: dlg.resettable
            kind: "text"
            text: Ui.tr("По умолчанию", "Default")
            onClicked: { field.text = ""; area.text = "" }
        },
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Сохранить", "Save")
            enabled: dlg.ok
            onClicked: {
                App.saveField(dlg.setting, dlg.value.trim())
                dlg.close()
            }
        }
    ]
}
