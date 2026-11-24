import QtQuick

Surface {
    id: it

    property string text
    property string icon
    property bool danger: false
    property bool checked: false
    property var menu: null
    property bool hot: false
    property real enter: 1
    readonly property real lead: icon !== "" || checked ? 36 : 0
    signal triggered()

    x: 4
    width: parent ? parent.width - 8 : implicitWidth
    implicitWidth: lead + label.implicitWidth + 32
    implicitHeight: 48
    radius: M.md
    interactive: true
    focusRing: false
    tint: danger ? Theme.error : Theme.onSurface
    opacity: enter
    Accessible.role: Accessible.MenuItem
    Accessible.name: text
    Accessible.checked: checked
    onClicked: {
        if (menu)
            menu.close()
        it.triggered()
    }

    Rectangle {
        anchors.fill: parent
        radius: it.radius
        color: Theme.alpha(it.tint, 0.10)
        visible: it.hot || (it.menu !== null && it.menu.held === it)
    }
    Row {
        id: row
        x: 12
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: (1 - it.enter) * (it.menu && it.menu.upward ? 8 : -8)
        spacing: 12
        opacity: it.enabled ? 1 : 0.38
        Icon {
            visible: it.icon !== "" || it.checked
            name: it.checked ? "check" : it.icon
            size: 24
            color: it.danger ? Theme.error : Theme.onSurfaceVariant
            anchors.verticalCenter: parent.verticalCenter
        }
        T {
            id: label
            width: Math.min(implicitWidth, it.width - 24 - it.lead)
            text: it.text
            type: "labelLarge"
            color: it.danger ? Theme.error : Theme.onSurface
            anchors.verticalCenter: parent.verticalCenter
        }
    }
}
