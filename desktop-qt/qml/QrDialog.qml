import QtQuick

Dialog {
    id: dlg

    property string link

    function showLink(name, text) {
        title = name
        link = text
        show()
    }

    maxWidth: 420
    preferredWidth: 388

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
                text: dlg.link
                ink: Theme.onPrimaryFixed
                background: Theme.primaryFixed
            }
        }
        T {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: Ui.shortLink(dlg.link)
            type: "bodyMedium"
            color: Theme.onSurfaceVariant
            elide: Text.ElideMiddle
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Скопировать", "Copy")
            onClicked: {
                App.copy(dlg.link)
                App.notice = Ui.tr("Ссылка скопирована", "Link copied")
                dlg.close()
            }
        },
        Btn {
            kind: "text"
            text: Ui.tr("Закрыть", "Close")
            onClicked: dlg.close()
        }
    ]
}
