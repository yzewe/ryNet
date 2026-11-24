import QtQuick

Dialog {
    id: dlg
    title: Ui.tr("Добавить сервер", "Add server")
    preferredWidth: 480
    function show() {
        open = true
        input.edit.forceActiveFocus()
    }
    Area {
        id: input
        width: parent.width
        placeholder: Ui.tr("Ссылка на сервер или конфиг", "Server link or configuration")
    }
    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Добавить", "Add")
            enabled: input.text.trim() !== ""
            onClicked: if (App.addServer(input.text)) { input.text = ""; dlg.close() }
        }
    ]
}
