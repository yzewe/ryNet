import QtQuick

Column {
    id: g
    width: parent ? parent.width : 0
    spacing: Ui.gap

    function relayout() {
        Ui.groupEdges(g, true, true, true)
    }
    onVisibleChildrenChanged: Qt.callLater(relayout)
    Component.onCompleted: relayout()
}
