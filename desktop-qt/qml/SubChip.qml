import QtQuick

Item {
    id: c

    property string text
    property string icon
    property bool selected: false
    property bool loading: false
    property Item menu: null
    signal clicked()
    signal menuAsked()

    property real corner: 10
    property real squeeze: 1
    readonly property bool lead: icon !== "" || loading
    readonly property color fg: selected ? Theme.onPrimary : Theme.onSurfaceVariant

    implicitHeight: 36
    implicitWidth: (lead ? 12 + (loading ? 20 : 18) + 6 : 16) + label.implicitWidth + 16
    scale: squeeze
    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.checked: selected
    Accessible.onPressAction: c.clicked()

    Spring on corner { id: cornering; to: c.selected ? 18 : 10; stiffness: M.fastK; damping: M.fastZ }
    Spring on squeeze {
        to: area.pressed ? 0.94 : 1
        stiffness: area.pressed ? M.pressInK : M.pressOutK
        damping: area.pressed ? 1 : M.pressOutZ
    }
    Component.onCompleted: cornering.snap(selected ? 18 : 10)

    Keys.onPressed: (e) => {
        if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Space) {
            c.clicked()
            e.accepted = true
        } else if (e.key === Qt.Key_Menu) {
            c.menuAsked()
            e.accepted = true
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: c.corner
        ColorTo on color { to: c.selected ? Theme.primary : Theme.surfaceContainerHigh }
    }
    StateLayer {
        anchors.fill: parent
        anchors.margins: -inset
        radii: Qt.vector4d(c.corner, c.corner, c.corner, c.corner)
        tint: c.fg
        hovered: area.containsMouse
        pressed: area.pressed
        focusRing: true
        focused: c.activeFocus
    }
    Item {
        x: 12
        width: c.loading ? 20 : 18
        height: width
        visible: c.lead
        anchors.verticalCenter: parent.verticalCenter
        Icon {
            anchors.centerIn: parent
            visible: !c.loading
            name: c.icon
            size: 18
            ColorTo on color { to: c.fg }
        }
        LoadingIndicator {
            anchors.fill: parent
            visible: c.loading
            running: visible
            color: c.fg
        }
    }
    T {
        id: label
        x: c.lead ? 12 + (c.loading ? 20 : 18) + 6 : 16
        anchors.verticalCenter: parent.verticalCenter
        text: c.text
        type: "labelLarge"
        ColorTo on color { to: c.fg }
    }

    MouseArea {
        id: area
        property bool held: false
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onPressed: (e) => {
            held = false
            if (e.button === Qt.RightButton) {
                c.menuAsked()
                e.accepted = true
            }
        }
        onClicked: (e) => {
            if (e.button === Qt.LeftButton)
                c.clicked()
        }
        onPressAndHold: (e) => {
            if (e.button !== Qt.LeftButton)
                return
            held = true
            c.menuAsked()
        }
        onPositionChanged: (e) => {
            if (held && c.menu)
                c.menu.drag(mapToItem(Ui.overlay, e.x, e.y))
        }
        onReleased: {
            if (held && c.menu)
                c.menu.release()
            held = false
        }
        onCanceled: {
            if (held && c.menu)
                c.menu.drag(null)
            held = false
        }
    }
}
