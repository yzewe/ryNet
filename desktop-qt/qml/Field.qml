import QtQuick

Item {
    id: f

    property alias text: input.text
    property alias input: input
    property string label
    property string placeholder
    property string error
    property string supporting
    property bool mono: false
    property bool password: false
    property color bg: Theme.surface
    property alias trailing: trail.data
    signal accepted()

    readonly property bool raised: input.activeFocus || input.text.length > 0
    readonly property bool focused: input.activeFocus
    readonly property bool hovered: hover.hovered && enabled
    readonly property bool failed: error !== ""
    readonly property color accent: !enabled ? Theme.alpha(Theme.onSurface, 0.38)
        : failed ? (hovered && !focused ? Theme.onErrorContainer : Theme.error)
        : focused ? Theme.primary : hovered ? Theme.onSurface : Theme.onSurfaceVariant
    readonly property color outline: !enabled ? Theme.alpha(Theme.onSurface, 0.12)
        : failed ? (hovered && !focused ? Theme.onErrorContainer : Theme.error)
        : focused ? Theme.primary : hovered ? Theme.onSurface : Theme.outline
    readonly property string message: error !== "" ? error : supporting
    property string note: ""
    property bool noteError: false

    property real lift: 0
    property real stroke: 1
    property real line: 0
    property real shake: 0
    property double editedAt: 0
    function nudge() { shaking.kick(260) }
    function shakeIfStale() {
        if (failed && Date.now() - editedAt > 400)
            nudge()
    }
    onFailedChanged: if (failed) Qt.callLater(shakeIfStale)

    implicitWidth: 280
    implicitHeight: 56 + 22 * Math.max(0, line)

    onMessageChanged: {
        if (message !== "") {
            note = message
            noteError = error !== ""
        }
    }
    Spring on lift {
        id: lifting
        to: f.raised ? 1 : 0
        stiffness: M.fastK
        damping: 0.75
    }
    Spring on stroke {
        to: f.focused ? 2 : 1
        stiffness: M.effectsK
        damping: 1
    }
    Spring on line {
        id: lining
        to: f.message !== "" ? 1 : 0
        stiffness: M.fastK
        damping: 1
    }
    Spring on shake { id: shaking; to: 0; stiffness: 1400; damping: 0.25 }
    Component.onCompleted: {
        note = message
        noteError = error !== ""
        lifting.snap(raised ? 1 : 0)
        lining.snap(message !== "" ? 1 : 0)
    }

    Rectangle {
        id: box
        x: f.shake
        width: parent.width
        height: 56
        radius: 16
        color: "transparent"
        border.width: f.stroke
        ColorTo on border.color { to: f.outline }
        HoverHandler { id: hover; cursorShape: Qt.IBeamCursor }
        TapHandler { onTapped: input.forceActiveFocus() }

        T {
            anchors.left: input.left
            anchors.right: input.right
            anchors.verticalCenter: input.verticalCenter
            text: f.placeholder
            type: "bodyLarge"
            font.family: f.mono ? "monospace" : M.font
            color: Theme.onSurfaceVariant
            opacity: 0.8 * (f.label === "" ? 1 : Math.max(0, Math.min(1, (f.lift - 0.5) * 2)))
            visible: input.text.length === 0 && opacity > 0
        }
        TextInput {
            id: input
            anchors.left: parent.left
            anchors.right: trail.left
            anchors.leftMargin: 16
            anchors.rightMargin: trail.width > 0 ? 4 : 16
            anchors.verticalCenter: parent.verticalCenter
            anchors.verticalCenterOffset: f.label !== "" ? 1 : 0
            color: f.enabled ? Theme.onSurface : Theme.alpha(Theme.onSurface, 0.38)
            font.family: f.mono ? "monospace" : M.font
            font.pixelSize: 16
            selectionColor: Theme.alpha(Theme.primary, 0.35)
            selectedTextColor: Theme.onSurface
            selectByMouse: true
            activeFocusOnTab: true
            clip: true
            echoMode: f.password ? TextInput.Password : TextInput.Normal
            Accessible.name: f.label !== "" ? f.label : f.placeholder
            Accessible.description: f.error !== "" ? f.error : f.supporting
            onTextEdited: f.editedAt = Date.now()
            onAccepted: {
                if (f.failed)
                    f.nudge()
                f.accepted()
            }
            cursorDelegate: Rectangle {
                id: caret
                property bool blink: true
                width: 2
                radius: 1
                color: f.failed ? Theme.error : Theme.primary
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
            id: trail
            anchors.right: parent.right
            anchors.rightMargin: childrenRect.width > 0 ? 4 : 0
            anchors.verticalCenter: parent.verticalCenter
            width: childrenRect.width
            height: childrenRect.height
        }

        TextMetrics {
            id: raisedLabel
            text: f.label
            font.family: label.font.family
            font.weight: label.font.weight
            font.letterSpacing: label.font.letterSpacing
            font.pixelSize: 12
        }
        Rectangle {
            readonly property real full: Math.ceil(raisedLabel.advanceWidth) + 8
            visible: f.label !== "" && width > 0.5
            x: 12 + (full - width) / 2
            y: -1
            width: full * Math.max(0, Math.min(1, f.lift))
            height: 4
            color: f.bg
        }
        T {
            id: label
            visible: f.label !== ""
            text: f.label
            type: "bodyLarge"
            font.pixelSize: Math.round(16 - 4 * f.lift)
            x: 16
            y: (box.height - height) / 2 * (1 - f.lift) + (-height / 2) * f.lift
            ColorTo on color { to: f.accent }
        }
    }
    T {
        x: 16 + f.shake
        y: box.height + 4 - 6 * (1 - Math.min(1, f.line))
        width: parent.width - 32
        text: f.note
        type: "bodySmall"
        color: f.noteError ? Theme.error : Theme.onSurfaceVariant
        opacity: Math.max(0, Math.min(1, f.line * 1.4 - 0.4))
        visible: text !== "" && opacity > 0
    }
}
