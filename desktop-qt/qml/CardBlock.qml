import QtQuick

Surface {
    id: c

    property bool groupFirst: true
    property bool groupLast: true
    property real topEnd: 1
    property real bottomEnd: 1
    default property alias content: inner.data

    readonly property real topCorner: Ui.inner + (Ui.outer - Ui.inner) * topEnd
    readonly property real bottomCorner: Ui.inner + (Ui.outer - Ui.inner) * bottomEnd

    width: parent ? parent.width : 0
    height: inner.implicitHeight + 32
    color: Theme.surfaceContainerHigh
    topLeftRadius: Math.max(0, topCorner)
    topRightRadius: Math.max(0, topCorner)
    bottomLeftRadius: Math.max(0, bottomCorner)
    bottomRightRadius: Math.max(0, bottomCorner)
    onGroupFirstChanged: topMorph.to = groupFirst ? 1 : 0
    onGroupLastChanged: bottomMorph.to = groupLast ? 1 : 0
    Component.onCompleted: {
        topMorph.snap(groupFirst ? 1 : 0)
        bottomMorph.snap(groupLast ? 1 : 0)
    }

    Spring on topEnd { id: topMorph; stiffness: M.fastK; damping: 0.7 }
    Spring on bottomEnd { id: bottomMorph; stiffness: M.fastK; damping: 0.7 }

    Column {
        id: inner
        objectName: "cardInner"
        x: 16
        y: 16
        width: parent.width - 32
    }
}
