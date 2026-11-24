import QtQuick

Column {
    id: b

    property string title
    property string current
    property var options: []
    property var labels: []
    property var selected
    property var summaries: []
    property bool first: false
    default property alias extra: extraHolder.data
    signal picked(var value)

    readonly property bool cascadeUnit: !carded
    readonly property bool carded: parent !== null && parent.objectName === "cardInner"

    width: parent ? parent.width : 0
    bottomPadding: carded ? 0 : 12

    Item {
        visible: b.title !== "" || !b.carded
        width: parent.width
        height: heading.implicitHeight + (b.first || b.carded ? 0 : 20) + 12
        T {
            id: heading
            y: b.first || b.carded ? 0 : 20
            anchors.left: parent.left
            anchors.right: shownValue.left
            anchors.rightMargin: shownValue.text !== "" ? 16 : 0
            text: b.title
            type: b.carded ? "titleMediumE" : "titleSmall"
            font.pixelSize: b.carded ? 15 : spec[0]
            color: b.carded ? Theme.onSurface : Theme.primary
        }
        T {
            id: shownValue
            y: heading.y
            anchors.right: parent.right
            width: Math.min(implicitWidth, parent.width * 0.6)
            text: b.current
            type: "titleSmall"
            color: Theme.onSurfaceVariant
            elide: Text.ElideMiddle
        }
    }
    Choice {
        visible: b.options.length > 0
        width: parent.width
        options: b.options
        labels: b.labels
        selected: b.selected
        onPicked: (v) => b.picked(v)
    }
    T {
        readonly property int at: b.options.indexOf(b.selected)
        visible: text !== ""
        width: parent.width
        topPadding: 8
        text: at >= 0 && at < b.summaries.length ? b.summaries[at] : ""
        type: "bodyMedium"
        font.pixelSize: 13
        color: Theme.onSurfaceVariant
        wrapMode: Text.Wrap
    }
    Column {
        id: extraHolder
        width: parent.width
        visible: children.length > 0
    }
}
