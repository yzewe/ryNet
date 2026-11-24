import QtQuick

Item {
    id: r

    property string text
    property bool checked: false
    signal clicked(real x, real y)

    readonly property bool hovered: area.containsMouse
    readonly property bool pressed: area.pressed || held.running
    readonly property bool keyboardFocus: activeFocus
    readonly property color ink: !enabled ? Theme.alpha(Theme.onSurface, 0.38)
        : checked ? Theme.primary : hovered || pressed || keyboardFocus ? Theme.onSurface : Theme.onSurfaceVariant
    property real dot: 0
    property real squeeze: 1

    implicitHeight: 48
    implicitWidth: label.x + label.implicitWidth + 12
    activeFocusOnTab: enabled
    Accessible.role: Accessible.RadioButton
    Accessible.name: text
    Accessible.checkable: true
    Accessible.checked: checked
    Accessible.onPressAction: if (enabled) r.clicked(width / 2, height / 2)
    Accessible.onToggleAction: if (enabled) r.clicked(width / 2, height / 2)
    Keys.onPressed: (e) => {
        if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Space) {
            held.restart()
            r.clicked(width / 2, height / 2)
            e.accepted = true
        }
    }

    Spring on dot {
        id: dotSpring
        to: r.checked ? 1 : 0
        stiffness: r.checked ? M.fastK : M.effectsK
        damping: r.checked ? M.fastZ : 1
    }
    Spring on squeeze {
        to: r.pressed ? 0.84 : 1
        stiffness: r.pressed ? M.pressInK : M.pressOutK
        damping: r.pressed ? 1 : M.pressOutZ
    }
    Component.onCompleted: dotSpring.snap(checked ? 1 : 0)

    Item {
        x: 4
        width: 40
        height: 40
        anchors.verticalCenter: parent.verticalCenter

        StateLayer {
            anchors.fill: parent
            anchors.margins: -inset
            focusRing: true
            origin: Qt.point(20, 20)
            radii: Qt.vector4d(20, 20, 20, 20)
            tint: r.checked ? Theme.primary : Theme.onSurface
            hovered: r.hovered && r.enabled
            pressed: r.pressed
            focused: r.keyboardFocus
        }
        Rectangle {
            anchors.centerIn: parent
            width: 20
            height: 20
            radius: 10
            scale: r.squeeze
            color: "transparent"
            border.width: 2
            ColorTo on border.color { to: r.ink }
            Rectangle {
                anchors.centerIn: parent
                width: 10 * Math.max(0, r.dot)
                height: width
                radius: width / 2
                visible: width > 0.2
                color: r.ink
            }
        }
    }
    T {
        id: label
        x: 56
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(0, parent.width - x - 12)
        text: r.text
        type: "bodyLarge"
        color: r.enabled ? Theme.onSurface : Theme.alpha(Theme.onSurface, 0.38)
    }
    MouseArea {
        id: area
        anchors.fill: parent
        enabled: r.enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onPressed: held.restart()
        onClicked: (m) => r.clicked(m.x, m.y)
    }
    Timer {
        id: held
        interval: M.minPressMs
    }
}
