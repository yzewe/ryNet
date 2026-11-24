import QtQuick

Item {
    id: s

    property alias text: input.text
    property string hint
    property string icon: "search"

    readonly property bool focused: input.activeFocus
    readonly property bool hovered: hover.hovered
    property real tilt: 0
    property real clearIn: 0

    implicitWidth: 360
    implicitHeight: 56

    onFocusedChanged: if (focused) tipping.kick(-520)
    Spring on tilt {
        id: tipping
        to: 0
        stiffness: M.spatialK
        damping: 0.42
    }
    Spring on clearIn {
        id: popping
        to: input.text.length > 0 ? 1 : 0
        stiffness: input.text.length > 0 ? M.fastK : M.effectsK
        damping: input.text.length > 0 ? M.fastZ : 1
    }
    Component.onCompleted: popping.snap(input.text.length > 0 ? 1 : 0)

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceContainerHigh
    }
    StateLayer {
        anchors.fill: parent
        radii: Qt.vector4d(height / 2, height / 2, height / 2, height / 2)
        tint: Theme.onSurface
        hovered: s.hovered
        focused: s.focused
    }
    HoverHandler { id: hover; cursorShape: Qt.IBeamCursor }
    TapHandler { onTapped: input.forceActiveFocus() }

    Icon {
        id: lead
        x: 16
        anchors.verticalCenter: parent.verticalCenter
        name: s.icon
        color: Theme.onSurface
        rotation: s.tilt
    }
    T {
        anchors.left: input.left
        anchors.right: input.right
        anchors.verticalCenter: input.verticalCenter
        visible: input.text.length === 0
        text: s.hint
        type: "bodyLarge"
        color: Theme.onSurfaceVariant
    }
    TextInput {
        id: input
        anchors.left: lead.right
        anchors.leftMargin: 12
        anchors.right: clear.left
        anchors.rightMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        color: Theme.onSurface
        font.family: M.font
        font.pixelSize: 16
        selectByMouse: true
        activeFocusOnTab: true
        selectionColor: Theme.alpha(Theme.primary, 0.35)
        selectedTextColor: Theme.onSurface
        clip: true
        Accessible.name: s.hint
        Accessible.searchEdit: true
        Keys.onEscapePressed: text = ""
        cursorDelegate: Rectangle {
            id: caret
            property bool blink: true
            width: 2
            radius: 1
            color: Theme.primary
            visible: input.cursorVisible
            opacity: blink ? 1 : 0
            Timer {
                id: blinker
                interval: Math.max(250, Qt.styleHints.cursorFlashTime / 2)
                repeat: true
                running: caret.visible && Qt.styleHints.cursorFlashTime > 0
                onRunningChanged: caret.blink = true
                onTriggered: caret.blink = !caret.blink
            }
            Connections {
                target: input
                function onCursorPositionChanged() {
                    caret.blink = true
                    blinker.restart()
                }
            }
        }
    }
    Item {
        id: clear
        anchors.right: parent.right
        anchors.rightMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        width: button.width
        height: button.height
        visible: s.clearIn > 0.01
        opacity: Math.min(1, s.clearIn * 1.5)
        scale: Math.max(0, s.clearIn)
        rotation: -90 * (1 - s.clearIn)
        IconBtn {
            id: button
            icon: "close"
            iconSize: 20
            tip: Ui.tr("Очистить", "Clear")
            onClicked: input.text = ""
        }
    }
}
