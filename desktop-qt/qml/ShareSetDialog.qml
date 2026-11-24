import QtQuick

Dialog {
    id: dlg

    property var set: ({})
    property string link
    property var parts: []
    property int frame: 0

    function shareSet(s) {
        set = s
        link = App.setLink(s)
        parts = link.length > 900 ? App.setParts(link) : []
        frame = 0
        title = s.name || Ui.tr("Набор", "Set")
        show()
    }

    maxWidth: 420
    preferredWidth: 388

    Timer {
        interval: 450
        repeat: true
        running: dlg.open && dlg.parts.length > 1
        onTriggered: dlg.frame = (dlg.frame + 1) % dlg.parts.length
    }

    Column {
        width: parent.width
        spacing: 14
        Rectangle {
            width: Math.min(parent.width, 340)
            height: width
            radius: 24
            color: Theme.primaryFixed
            anchors.horizontalCenter: parent.horizontalCenter
            QrCode {
                anchors.fill: parent
                anchors.margins: 16
                text: dlg.parts.length > 1 ? dlg.parts[dlg.frame] : dlg.link
                stream: dlg.parts.length > 1
                ink: Theme.onPrimaryFixed
                background: Theme.primaryFixed
            }
        }
        WavyProgress {
            visible: dlg.parts.length > 1
            width: parent.width
            height: 12
            moving: true
            value: dlg.parts.length > 0 ? (dlg.frame + 1) / dlg.parts.length : 0
            color: Theme.primary
            trackColor: Theme.surfaceContainerHighest
        }
        T {
            anchors.horizontalCenter: parent.horizontalCenter
            text: App.plural(dlg.set.size || 0, "запись", "записи", "записей", "entry", "entries")
            type: "bodyMedium"
            color: Theme.onSurfaceVariant
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Сохранить файлом", "Save as file")
            onClicked: {
                const name = (dlg.set.name || "set").replace(/[^\p{L}\p{N} _-]/gu, "") || "set"
                const link = dlg.link
                App.pickFile(true, name + ".rynet.txt", Ui.tr("Набор правил", "Rule set"), "txt", (path) => {
                    if (path)
                        App.writeText(path, link, Ui.tr("Набор сохранён: %1", "Set saved: %1").arg(path.replace(/^.*[\\/]/, "")))
                })
                dlg.close()
            }
        },
        Btn {
            kind: "text"
            text: Ui.tr("Скопировать", "Copy")
            onClicked: {
                App.copy(dlg.link)
                App.notice = Ui.tr("Ссылка на набор скопирована", "Set link copied")
                dlg.close()
            }
        }
    ]
}
