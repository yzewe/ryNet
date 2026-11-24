import QtQuick

Item {
    id: s

    default property alias content: flick.flickableData
    property alias flick: flick
    property alias contentY: flick.contentY
    property real bottomPadding: 0

    Item {
        anchors.fill: parent
        anchors.leftMargin: -6
        anchors.rightMargin: -6
        clip: true
        Flickable {
            id: flick
            x: 6
            width: s.width
            height: parent.height
            contentWidth: width
            contentHeight: contentItem.childrenRect.height + s.bottomPadding
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            interactive: contentHeight > height
        }
    }
    Connections {
        target: s.Window.window
        enabled: flick.interactive
        function onActiveFocusItemChanged() {
            const f = s.Window.window.activeFocusItem
            if (!Ui.within(f, flick.contentItem))
                return
            const r = f.mapToItem(flick.contentItem, 0, 0, f.width, f.height)
            let to = flick.contentY
            if (r.y < flick.contentY + 8)
                to = r.y - 8
            else if (r.y + r.height > flick.contentY + flick.height - 8)
                to = r.y + r.height - flick.height + 8
            to = Math.max(0, Math.min(flick.contentHeight - flick.height, to))
            if (Math.abs(to - flick.contentY) > 0.5) {
                revealing.to = to
                revealing.restart()
            }
        }
    }
    NumberAnimation {
        id: revealing
        target: flick
        property: "contentY"
        duration: 260
        easing.type: Easing.BezierSpline
        easing.bezierCurve: M.decelerate
    }
    Bar {
        flick: flick
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
    }
}
