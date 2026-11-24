import QtQuick

Rectangle {
    id: p

    property string title
    property real pad: 24
    property alias actions: actionRow.data
    default property alias content: inner.data
    readonly property real side: Ui.compact ? 16 : pad
    property real roll: 0
    property bool ready: false

    onTitleChanged: if (ready && title !== "") rolling.animateFrom(1)
    Component.onCompleted: ready = true
    Spring on roll {
        id: rolling
        to: 0
        stiffness: M.fastK
        damping: 0.75
    }

    radius: Ui.compact ? 0 : M.xl
    color: Ui.compact ? "transparent" : Theme.surface

    Item {
        id: header
        visible: p.title !== ""
        x: p.side
        y: Ui.compact ? 8 : p.pad
        width: p.width - p.side * 2
        height: visible ? 48 : 0
        T {
            anchors.left: parent.left
            anchors.right: actionRow.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            text: p.title
            type: "headlineSmallE"
            opacity: 1 - Math.max(0, p.roll)
            transform: Translate { y: 12 * p.roll }
        }
        Row {
            id: actionRow
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
        }
    }
    Item {
        id: inner
        anchors.top: header.visible ? header.bottom : parent.top
        anchors.topMargin: header.visible ? 16 : (Ui.compact ? 12 : p.pad)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: p.side
        anchors.rightMargin: p.side
        anchors.bottomMargin: Ui.compact ? 0 : p.pad
    }
}
