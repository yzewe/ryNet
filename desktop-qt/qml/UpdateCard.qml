import QtQuick

Item {
    id: card

    readonly property var info: App.update
    readonly property bool cascadeUnit: true

    width: parent ? parent.width : 0
    visible: info.state === "found"
    height: visible ? panel.height + 8 : 0

    Rectangle {
        id: panel
        width: parent.width
        height: body.height + 32
        radius: M.lgPlus
        color: Theme.secondaryContainer

        Icon {
            id: glyph
            x: 20
            y: 16
            name: "campaign"
            size: 20
            color: Theme.onSecondaryContainer
        }
        Column {
            id: body
            anchors.left: glyph.right
            anchors.leftMargin: 12
            anchors.right: parent.right
            anchors.rightMargin: 20
            y: 16
            spacing: 12

            T {
                width: parent.width
                text: Ui.tr("Доступна версия ", "Version ") + (card.info.version || "") + Ui.tr("", " is available")
                type: "titleMedium"
                color: Theme.onSecondaryContainer
            }
            Column {
                width: parent.width
                spacing: 4
                visible: (card.info.changelog || "") !== ""
                T {
                    width: parent.width
                    text: Ui.tr("Что нового", "What's new")
                    type: "labelLarge"
                    color: Theme.onSecondaryContainer
                }
                Scroll {
                    id: notes
                    width: parent.width
                    height: Math.min(168, notesText.implicitHeight)
                    T {
                        id: notesText
                        width: notes.width - 14
                        text: card.info.changelog || ""
                        type: "bodyMedium"
                        wrapMode: Text.Wrap
                        elide: Text.ElideNone
                        lineHeight: 1.15
                        color: Theme.onSecondaryContainer
                    }
                }
            }
            Flow {
                width: parent.width
                spacing: 8
                Btn {
                    kind: "filled"
                    icon: "download"
                    text: Ui.tr("Скачать", "Download")
                    onClicked: App.openUpdate()
                }
                Btn {
                    visible: (card.info.post || "") !== ""
                    kind: "text"
                    icon: "open_in_new"
                    text: Ui.tr("Открыть пост", "Open post")
                    onClicked: App.openUpdatePost()
                }
            }
            T {
                width: parent.width
                visible: (card.info.sha256 || "") !== ""
                text: "SHA-256: " + (card.info.sha256 || "")
                type: "bodySmall"
                wrapMode: Text.WrapAnywhere
                elide: Text.ElideNone
                color: Theme.alpha(Theme.onSecondaryContainer, 0.72)
                Accessible.role: Accessible.Button
                Accessible.name: Ui.tr("Скопировать SHA-256", "Copy SHA-256")
                TapHandler { onTapped: App.copy(card.info.sha256) }
            }
        }
    }
}
