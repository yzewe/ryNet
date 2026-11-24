import QtQuick

Item {
    id: list

    property ServerGrid grid
    property real bottomPad: 104
    signal picked(string remarks)
    signal menu(string remarks, Item at, real x, real y)

    function enter(dir) {
        grid.enter(dir)
    }
    function toTop() {
        scrolling.stop()
        flick.contentY = 0
    }
    function reveal(key) {
        const to = grid.revealY(key, flick.contentHeight - flick.height)
        if (to >= 0) {
            scrolling.to = to
            scrolling.restart()
        }
    }

    Binding { target: list.grid; property: "viewWidth"; value: flick.width }
    Binding { target: list.grid; property: "viewHeight"; value: flick.height }
    Binding { target: list.grid; property: "contentY"; value: flick.contentY }

    Flickable {
        id: flick
        anchors.fill: parent
        anchors.topMargin: -list.grid.inset
        anchors.leftMargin: -list.grid.inset
        anchors.rightMargin: -list.grid.inset
        contentWidth: width
        contentHeight: list.grid.contentHeight + list.bottomPad
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        NumberAnimation {
            id: scrolling
            target: flick
            property: "contentY"
            duration: 260
            easing.type: Easing.BezierSpline
            easing.bezierCurve: M.decelerate
        }

        Repeater {
            model: list.grid.visibleItems

            GridCell {
                id: cell
                required kind
                required key
                required property string title
                required property string note
                required property int count
                required current
                required property var model

                grid: list.grid
                ready: body.status === Loader.Ready

                Loader {
                    id: body
                    anchors.fill: parent
                    active: cell.live
                    sourceComponent: cell.kind === "tile" ? tile : cell.kind === "header" ? header : notice
                }
                Component {
                    id: tile
                    Tile {
                        id: t
                        info: cell.model
                        onActiveFocusChanged: if (activeFocus) list.reveal(cell.key)
                        onPicked: list.picked(cell.model.remarks)
                        onMenu: (x, y) => list.menu(cell.model.remarks, t, x, y)
                    }
                }
                Component {
                    id: header
                    Item {
                        Column {
                            y: 12
                            width: parent.width
                            T {
                                width: parent.width
                                text: cell.title + "<font color=\"" + Theme.onSurfaceVariant + "\"> · " + cell.count + "</font>"
                                textFormat: Text.StyledText
                                type: "titleSmall"
                                color: Theme.primary
                            }
                            T {
                                visible: cell.note !== ""
                                width: parent.width
                                text: cell.note
                                type: "bodySmall"
                                color: Theme.onSurfaceVariant
                            }
                        }
                    }
                }
                Component {
                    id: notice
                    Item {
                        Notice {
                            width: parent.width
                            text: cell.model.text
                            onImplicitHeightChanged: list.grid.setNoticeHeight(cell.key, implicitHeight)
                            Component.onCompleted: list.grid.setNoticeHeight(cell.key, implicitHeight)
                        }
                    }
                }
            }
        }
    }
    Bar {
        flick: flick
        anchors.right: flick.right
        anchors.top: flick.top
        anchors.bottom: flick.bottom
    }
}
