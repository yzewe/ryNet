import QtQuick

Item {
    id: g

    default property alias items: col.data
    property bool first: true
    property bool last: true
    property var menu: null
    property real enter: 1

    width: parent ? parent.width : implicitWidth
    implicitWidth: {
        let w = 0
        for (const c of col.children)
            if (c.visible)
                w = Math.max(w, c.implicitWidth + (c.x || 0) * 2)
        return w
    }
    implicitHeight: col.implicitHeight + 8
    opacity: enter
    onMenuChanged: {
        for (const c of col.children)
            if (c.menu !== undefined)
                c.menu = g.menu
    }

    Shadow {
        anchors.fill: parent
        radius: M.lg
        level: 2
    }
    Rectangle {
        anchors.fill: parent
        topLeftRadius: g.first ? M.lg : M.xs
        topRightRadius: topLeftRadius
        bottomLeftRadius: g.last ? M.lg : M.xs
        bottomRightRadius: bottomLeftRadius
        color: Theme.surfaceContainer
    }
    Column {
        id: col
        y: 4
        width: parent.width
    }
}
