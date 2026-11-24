import QtQuick

Column {
    id: b

    property string title
    property var steps: []
    property var selected
    property var label: (v) => String(v)
    property var tick: label
    property bool live: false
    property bool first: false
    signal picked(var value)

    readonly property bool cascadeUnit: !carded
    readonly property bool carded: parent !== null && parent.objectName === "cardInner"

    readonly property int count: steps.length
    readonly property int savedIndex: Ui.nearest(steps, selected)
    property int at: savedIndex
    property bool dragging: false
    property real pointer: 0

    onSavedIndexChanged: if (!dragging) at = savedIndex
    onAtChanged: if (live && dragging && count > 0 && steps[at] !== selected) picked(steps[at])

    width: parent ? parent.width : 0
    bottomPadding: carded ? 0 : 4
    opacity: enabled ? 1 : 0.38
    activeFocusOnTab: enabled && count > 1
    Accessible.role: Accessible.Slider
    Accessible.name: title
    Accessible.description: count > 0 ? label(steps[at]) : ""
    Keys.onPressed: (e) => {
        const step = e.key === Qt.Key_Left || e.key === Qt.Key_Down ? -1 : e.key === Qt.Key_Right || e.key === Qt.Key_Up ? 1 : 0
        if (step === 0 || count < 2)
            return
        const i = Math.max(0, Math.min(count - 1, savedIndex + (LayoutMirroring.enabled ? -step : step)))
        if (steps[i] !== selected)
            picked(steps[i])
        e.accepted = true
    }

    Item {
        visible: b.title !== "" || !b.carded
        width: parent.width
        height: heading.implicitHeight + (b.first || b.carded ? 0 : 20) + 12
        T {
            id: heading
            y: b.first || b.carded ? 0 : 20
            anchors.left: parent.left
            anchors.right: value.left
            anchors.rightMargin: 16
            text: b.title
            type: b.carded ? "titleMediumE" : "titleSmall"
            font.pixelSize: b.carded ? 15 : spec[0]
            color: b.carded ? Theme.onSurface : Theme.primary
        }
        T {
            id: value
            y: heading.y
            anchors.right: parent.right
            text: b.count > 0 ? b.label(b.steps[b.at]) : ""
            type: "titleSmall"
            color: Theme.onSurfaceVariant
        }
    }

    Item {
        id: slider
        readonly property real inset: 8
        readonly property real span: Math.max(1, width - 2 * inset)
        readonly property real mid: 18
        function xOf(i) { return inset + (b.count > 1 ? i / (b.count - 1) : 0) * span }
        function indexAt(x) { return b.count > 1 ? Math.max(0, Math.min(b.count - 1, Math.round((x - inset) / span * (b.count - 1)))) : 0 }
        property real pos: 8
        property real grip: 4

        width: parent.width
        height: 56
        Spring on pos {
            id: travel
            to: b.dragging ? Math.max(slider.inset, Math.min(slider.inset + slider.span, b.pointer)) : slider.xOf(b.at)
            stiffness: b.dragging ? 4000 : M.fastK
            damping: b.dragging ? 1 : 0.8
        }
        Spring on grip { to: b.dragging ? 2 : 4; stiffness: M.fastK; damping: 1 }
        Component.onCompleted: travel.snap(xOf(b.at))
        onWidthChanged: if (!b.dragging) travel.snap(xOf(b.at))

        Rectangle {
            visible: width > 0.5
            y: slider.mid - 8
            width: Math.max(0, slider.pos - slider.grip / 2 - 6)
            height: 16
            color: Theme.primary
            topLeftRadius: 8
            bottomLeftRadius: 8
            topRightRadius: Math.min(2, width / 2)
            bottomRightRadius: Math.min(2, width / 2)
        }
        Rectangle {
            readonly property real from: slider.pos + slider.grip / 2 + 6
            visible: width > 0.5
            x: from
            y: slider.mid - 8
            width: Math.max(0, slider.width - from)
            height: 16
            color: Theme.secondaryContainer
            topRightRadius: 8
            bottomRightRadius: 8
            topLeftRadius: Math.min(2, width / 2)
            bottomLeftRadius: Math.min(2, width / 2)
        }
        Repeater {
            model: b.count
            Item {
                required property int index
                readonly property real cx: slider.xOf(index)
                readonly property real off: cx - slider.pos
                Rectangle {
                    visible: Math.abs(parent.off) > slider.grip / 2 + 6
                    x: parent.cx - 2
                    y: slider.mid - 2
                    width: 4
                    height: 4
                    radius: 2
                    color: parent.off < 0 ? Theme.onPrimary : Theme.alpha(Theme.onSecondaryContainer, 0.6)
                }
                T {
                    x: Math.max(0, Math.min(slider.width - width, parent.cx - width / 2))
                    y: slider.mid + 8 + 8
                    text: b.tick(b.steps[index])
                    type: "labelMedium"
                    ColorTo on color { to: index === b.at ? Theme.primary : Theme.onSurfaceVariant }
                }
            }
        }
        Rectangle {
            x: slider.pos - width / 2
            width: slider.grip
            height: 36
            radius: width / 2
            color: Theme.primary
        }
        StateLayer {
            x: slider.pos - 8 - inset
            y: -inset
            width: 16 + 2 * inset
            height: 36 + 2 * inset
            radii: Qt.vector4d(8, 8, 8, 8)
            tint: "transparent"
            focusRing: true
            focused: b.activeFocus
        }
        MouseArea {
            anchors.fill: parent
            enabled: b.enabled && b.count > 1
            preventStealing: true
            cursorShape: Qt.PointingHandCursor
            onPressed: (m) => {
                b.pointer = m.x
                b.dragging = true
                b.at = slider.indexAt(m.x)
            }
            onPositionChanged: (m) => {
                b.pointer = m.x
                b.at = slider.indexAt(m.x)
            }
            onReleased: {
                b.dragging = false
                if (b.steps[b.at] !== b.selected)
                    b.picked(b.steps[b.at])
                b.at = b.savedIndex
            }
            onCanceled: {
                b.dragging = false
                b.at = b.savedIndex
            }
        }
    }
}
