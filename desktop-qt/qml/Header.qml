import QtQuick

T {
    property bool first: false
    width: parent ? parent.width : implicitWidth
    topPadding: first ? 0 : 20
    bottomPadding: 12
    type: "titleSmall"
    color: Theme.primary
    readonly property bool cascadeUnit: true
}
