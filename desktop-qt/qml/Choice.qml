import QtQuick

Item {
    id: c

    property var options: []
    property var labels: []
    property var selected
    property bool compact: false
    signal picked(var value)

    readonly property int count: options.length
    readonly property int selectedIndex: Math.max(0, options.indexOf(selected))
    property int active: selectedIndex
    property bool holding: false
    property int built: 0
    readonly property real gap: 4

    readonly property bool keyboardFocus: activeFocus
    activeFocusOnTab: enabled && count > 1
    opacity: enabled ? 1 : 0.38
    Accessible.role: Accessible.Grouping
    Keys.onPressed: (e) => {
        const step = e.key === Qt.Key_Left || e.key === Qt.Key_Up ? -1 : e.key === Qt.Key_Right || e.key === Qt.Key_Down ? 1 : 0
        if (step === 0 || count < 2)
            return
        const i = Math.max(0, Math.min(count - 1, selectedIndex + (LayoutMirroring.enabled ? -step : step)))
        if (options[i] !== selected) {
            const s = segments.itemAt(i)
            if (s)
                s.nudge()
            picked(options[i])
        }
        e.accepted = true
    }

    implicitHeight: compact ? 40 : 48
    implicitWidth: row.implicitWidth

    onSelectedIndexChanged: if (!holding) active = selectedIndex

    FontMetrics {
        id: metrics
        font.family: M.font
        font.pixelSize: 14
        font.weight: 500
    }

    function indexAt(x) {
        return row.indexAt(x)
    }

    SegmentRow {
        id: row
        anchors.fill: parent
        gap: c.gap
        Repeater {
            id: segments
            model: c.count
            onItemAdded: c.built++
            onItemRemoved: c.built++
            Rectangle {
                id: seg
                required property int index
                readonly property bool on: index === c.active
                readonly property bool hovered: hover.hovered && c.enabled
                readonly property real outer: height / 2
                readonly property string label: c.labels[index] !== undefined ? String(c.labels[index]) : String(c.options[index])
                readonly property real weight: metrics.advanceWidth(label) + 24
                property real swell: 1
                property real round: 0
                property real lit: 0
                property real tone: c.holding && on ? 0.10 : c.keyboardFocus && on ? 0.10 : hovered ? 0.08 : 0
                readonly property real inner: Math.max(0, 6 + (outer - 6) * round)
                topLeftRadius: index === 0 ? outer : inner
                bottomLeftRadius: index === 0 ? outer : inner
                topRightRadius: index === c.count - 1 ? outer : inner
                bottomRightRadius: index === c.count - 1 ? outer : inner
                color: Theme.mix(Theme.mix(Theme.surfaceContainerHighest, Theme.onSurface, tone), Theme.mix(Theme.primary, Theme.onPrimary, tone), lit)
                Accessible.role: Accessible.RadioButton
                Accessible.name: label
                Accessible.checkable: true
                Accessible.checked: index === c.selectedIndex
                Accessible.onPressAction: if (c.enabled && c.options[index] !== c.selected) c.picked(c.options[index])
                Accessible.onToggleAction: if (c.enabled && c.options[index] !== c.selected) c.picked(c.options[index])
                Behavior on tone { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
                function nudge() { swelling.kick(4) }
                Spring on swell {
                    id: swelling
                    to: c.holding && seg.on ? 1.35 : 1
                    stiffness: M.fastK
                    damping: 0.7
                }
                Spring on round {
                    id: rounding
                    to: seg.on ? 1 : 0
                    stiffness: M.fastK
                    damping: M.fastZ
                }
                Spring on lit {
                    id: lighting
                    to: seg.on ? 1 : 0
                    stiffness: M.effectsFastK
                    damping: 1
                }
                onOnChanged: if (on) lighting.snap(1)
                Component.onCompleted: {
                    rounding.snap(on ? 1 : 0)
                    lighting.snap(on ? 1 : 0)
                }
                HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                T {
                    anchors.centerIn: parent
                    width: parent.width - 12
                    horizontalAlignment: Text.AlignHCenter
                    text: seg.label
                    type: "labelLarge"
                    fontSizeMode: Text.HorizontalFit
                    minimumPixelSize: 10
                    color: Theme.mix(Theme.onSurfaceVariant, c.enabled ? Theme.onPrimary : Theme.onSurface, seg.lit)
                }
            }
        }
    }

    StateLayer {
        readonly property Item seg: (c.built, c.selectedIndex >= 0 ? segments.itemAt(c.active) : null)
        x: seg ? seg.x - inset : 0
        width: seg ? seg.width + 2 * inset : 0
        y: -inset
        height: c.height + 2 * inset
        radii: seg ? Qt.vector4d(seg.topLeftRadius, seg.topRightRadius, seg.bottomRightRadius, seg.bottomLeftRadius) : Qt.vector4d(0, 0, 0, 0)
        focusRing: true
        focused: c.keyboardFocus
        stateAlpha: 0
    }

    MouseArea {
        anchors.fill: parent
        enabled: c.enabled
        preventStealing: true
        onPressed: (m) => {
            c.holding = true
            c.active = c.indexAt(m.x)
        }
        onPositionChanged: (m) => {
            const i = c.indexAt(m.x)
            if (i !== c.active)
                c.active = i
        }
        onReleased: {
            c.holding = false
            if (c.active >= 0 && c.active < c.count && c.options[c.active] !== c.selected) {
                const s = segments.itemAt(c.active)
                if (s)
                    s.nudge()
                c.picked(c.options[c.active])
            }
        }
        onCanceled: {
            c.holding = false
            c.active = c.selectedIndex
        }
    }
}
