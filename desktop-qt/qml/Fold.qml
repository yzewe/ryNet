import QtQuick

Item {
    id: f

    property bool open: false
    readonly property bool fold: true
    property bool groupFirst: false
    property bool groupLast: false
    default property alias content: inner.data
    property real k: 0

    width: parent ? parent.width : 0
    height: inner.childrenRect.height * Math.max(0, k)
    visible: k > 0.005
    clip: opening.running

    function relayout() {
        Ui.groupEdges(inner, f.groupFirst, f.groupLast, false)
    }
    onGroupFirstChanged: relayout()
    onGroupLastChanged: relayout()
    onOpenChanged: if (parent && parent.relayout) Qt.callLater(parent.relayout)
    Component.onCompleted: {
        opening.snap(open ? 1 : 0)
        relayout()
    }

    Spring on k {
        id: opening
        to: f.open ? 1 : 0
        stiffness: M.spatialK
        damping: f.open ? 0.82 : 1
    }

    Column {
        id: inner
        y: -10 * (1 - Math.min(1, f.k))
        width: parent.width
        spacing: Ui.gap
        opacity: f.open ? Math.max(0, Math.min(1, (f.k - 0.25) / 0.6)) : Math.max(0, Math.min(1, (f.k - 0.45) / 0.55))
        onVisibleChildrenChanged: Qt.callLater(f.relayout)
    }
}
