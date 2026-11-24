import QtQuick

Dialog {
    id: dlg

    property string text
    property string confirm: Ui.tr("Удалить", "Delete")
    property bool danger: true
    signal accepted()

    preferredWidth: Math.min(480, question.implicitWidth + 48)

    T {
        id: question
        visible: dlg.text !== ""
        width: parent.width
        text: dlg.text
        type: "bodyMedium"
        color: Theme.onSurfaceVariant
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: dlg.danger ? "danger" : "text"
            text: dlg.confirm
            onClicked: {
                dlg.close()
                dlg.accepted()
            }
        }
    ]
}
