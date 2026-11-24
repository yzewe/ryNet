import QtQuick

Dialog {
    id: dlg

    property var parts: []
    property var chosen: []
    property string action
    signal picked(var keys)

    function ask(heading, verb, list) {
        title = heading
        action = verb
        parts = list
        chosen = Ui.pluck(list, "key")
        show()
    }

    icon: "restore"
    preferredWidth: 440

    Column {
        width: parent.width
        spacing: 2
        Repeater {
            model: dlg.parts
            Surface {
                id: row
                required property var modelData
                required property int index
                readonly property bool on: dlg.chosen.indexOf(modelData.key) >= 0
                width: parent.width
                height: Math.max(56, words.implicitHeight + 20)
                interactive: true
                radius: M.lg
                Accessible.role: Accessible.CheckBox
                Accessible.name: modelData.title
                Accessible.checked: on
                onClicked: dlg.chosen = Ui.toggled(dlg.chosen, modelData.key)
                Column {
                    id: words
                    anchors.left: parent.left
                    anchors.leftMargin: 16
                    anchors.right: toggle.left
                    anchors.rightMargin: 12
                    anchors.verticalCenter: parent.verticalCenter
                    T {
                        width: parent.width
                        text: row.modelData.title
                        type: "bodyLarge"
                        wrapMode: Text.Wrap
                    }
                    T {
                        visible: text !== ""
                        width: parent.width
                        text: row.modelData.summary || ""
                        type: "bodyMedium"
                        color: Theme.onSurfaceVariant
                    }
                }
                Switch {
                    id: toggle
                    anchors.right: parent.right
                    anchors.rightMargin: 16
                    anchors.verticalCenter: parent.verticalCenter
                    checked: row.on
                    interactive: false
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
            text: dlg.action
            enabled: dlg.chosen.length > 0
            onClicked: {
                const keys = dlg.chosen
                dlg.close()
                dlg.picked(keys)
            }
        }
    ]
}
