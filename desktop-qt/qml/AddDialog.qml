import QtQuick

Dialog {
    id: dlg

    readonly property string value: field.text.trim()

    function ask() {
        const clip = App.clipboard().trim()
        field.text = /^[a-z][a-z0-9+.-]*:\/\//i.test(clip) && clip.indexOf("\n") < 0 ? clip : ""
        show()
        field.input.forceActiveFocus()
        field.input.selectAll()
    }
    function go() {
        if (value !== "" && !App.busy)
            App.add(value)
    }

    title: Ui.tr("Добавить", "Add")
    icon: "add_link"
    maxWidth: 520
    preferredWidth: 440

    Connections {
        target: App
        function onAdded(link) {
            if (dlg.open && link === dlg.value)
                dlg.close()
        }
    }

    Field {
        id: field
        width: parent.width
        mono: true
        bg: Theme.surfaceContainerHigh
        placeholder: Ui.tr("Ссылка на подписку или сервер", "Subscription or server link")
        onAccepted: dlg.go()
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
            busy: App.busy
            enabled: dlg.value !== "" && !App.busy
            onClicked: dlg.go()
        }
    ]
}
