import QtQuick

Rectangle {
    id: tag

    property string action: "proxy"
    property string profile
    readonly property var split: Ui.splitFlag(profile || "")
    readonly property string label: action === "server" && profile ? split[1]
        : action === "direct" ? Ui.tr("Напрямую", "Direct") : action === "block" ? Ui.tr("Блок", "Block")
        : action === "server" ? Ui.tr("Сервер", "Server") : Ui.tr("Прокси", "Proxy")
    readonly property color fg: action === "direct" ? Theme.onSecondaryContainer : action === "block" ? Theme.onErrorContainer
        : action === "server" ? Theme.onTertiaryContainer : Theme.onPrimaryContainer

    implicitWidth: Math.min(220, row.implicitWidth + 20)
    implicitHeight: 28
    radius: M.sm
    ColorTo on color {
        to: action === "direct" ? Theme.secondaryContainer : action === "block" ? Theme.errorContainer
            : action === "server" ? Theme.tertiaryContainer : Theme.primaryContainer
    }

    Row {
        id: row
        x: 10
        anchors.verticalCenter: parent.verticalCenter
        spacing: 6
        Flag {
            visible: tag.split[0] !== ""
            width: 18
            height: 13
            radius: 3
            outline: false
            flag: tag.split[0]
            anchors.verticalCenter: parent.verticalCenter
        }
        T {
            width: Math.min(implicitWidth, 200 - (tag.split[0] !== "" ? 24 : 0))
            text: tag.label
            type: "labelLarge"
            ColorTo on color { to: tag.fg }
            anchors.verticalCenter: parent.verticalCenter
        }
    }
}
