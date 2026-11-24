import QtQuick

Rectangle {
    id: n

    property string icon: "campaign"
    property string text
    property bool error: false
    property bool closable: false
    signal closed()

    implicitHeight: label.implicitHeight + 32
    radius: M.lgPlus
    color: error ? Theme.errorContainer : Theme.secondaryContainer

    Icon {
        id: glyph
        x: 20
        y: 16
        name: n.icon
        size: 20
        color: n.error ? Theme.onErrorContainer : Theme.onSecondaryContainer
    }
    T {
        id: label
        anchors.left: glyph.right
        anchors.leftMargin: 12
        anchors.right: parent.right
        anchors.rightMargin: n.closable ? 52 : 20
        y: 16
        text: n.text
        type: "bodyMedium"
        wrapMode: Text.Wrap
        elide: Text.ElideNone
        lineHeight: 1.15
        color: n.error ? Theme.onErrorContainer : Theme.onSecondaryContainer
    }
    IconBtn {
        visible: n.closable
        anchors.right: parent.right
        anchors.rightMargin: 4
        y: 4
        icon: "close"
        iconSize: 20
        tip: Ui.tr("Скрыть", "Hide")
        iconColor: n.error ? Theme.onErrorContainer : Theme.onSecondaryContainer
        onClicked: n.closed()
    }
}
