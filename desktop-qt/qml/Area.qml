import QtQuick

Rectangle {
    id: a

    property alias text: edit.text
    property alias edit: edit
    property string placeholder
    property bool mono: true
    property bool error: false

    readonly property bool focused: edit.activeFocus
    readonly property bool hovered: hover.hovered && enabled
    property real stroke: 1

    implicitHeight: 160
    radius: 16
    color: "transparent"
    border.width: stroke
    ColorTo on border.color {
        to: !enabled ? Theme.alpha(Theme.onSurface, 0.12)
            : error ? (hovered && !focused ? Theme.onErrorContainer : Theme.error)
            : focused ? Theme.primary : hovered ? Theme.onSurface : Theme.outline
    }
    Spring on stroke {
        to: a.focused ? 2 : 1
        stiffness: M.effectsK
        damping: 1
    }
    HoverHandler { id: hover; cursorShape: Qt.IBeamCursor }

    Flickable {
        id: flick
        anchors.fill: parent
        anchors.margins: 14
        contentWidth: width
        contentHeight: edit.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        function ensureVisible(r) {
            if (contentY >= r.y)
                contentY = r.y
            else if (contentY + height <= r.y + r.height)
                contentY = r.y + r.height - height
        }
        T {
            visible: edit.text.length === 0
            width: flick.width
            text: a.placeholder
            font.family: a.mono ? "monospace" : M.font
            font.pixelSize: a.mono ? 13 : 15
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
            elide: Text.ElideNone
        }
        TextEdit {
            id: edit
            width: flick.width
            wrapMode: TextEdit.Wrap
            color: a.enabled ? Theme.onSurface : Theme.alpha(Theme.onSurface, 0.38)
            font.family: a.mono ? "monospace" : M.font
            font.pixelSize: a.mono ? 13 : 15
            selectByMouse: true
            selectionColor: Theme.alpha(Theme.primary, 0.35)
            selectedTextColor: Theme.onSurface
            textFormat: TextEdit.PlainText
            activeFocusOnTab: true
            Keys.onTabPressed: (e) => {
                const next = edit.nextItemInFocusChain(true)
                if (next && next !== edit)
                    next.forceActiveFocus(Qt.TabFocusReason)
            }
            Keys.onBacktabPressed: (e) => {
                const prev = edit.nextItemInFocusChain(false)
                if (prev && prev !== edit)
                    prev.forceActiveFocus(Qt.BacktabFocusReason)
            }
            onCursorRectangleChanged: flick.ensureVisible(cursorRectangle)
            cursorDelegate: Rectangle {
                id: caret
                property bool blink: true
                width: 2
                radius: 1
                color: a.error ? Theme.error : Theme.primary
                visible: edit.cursorVisible
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
                    target: edit
                    function onCursorPositionChanged() {
                        caret.blink = true
                        blinker.restart()
                    }
                }
            }
        }
    }
    Bar {
        flick: flick
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: 4
    }
}
