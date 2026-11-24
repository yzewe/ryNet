import QtQuick

Item {
    id: f

    property bool running: false
    property real progress: 0
    property string label
    signal clicked()

    property real lift: 3
    property real round: 0
    property real ring: 0
    property real shown: 0

    implicitWidth: 56
    implicitHeight: 56

    Spring on lift { to: surface.hovered && !surface.down ? 4 : 3; stiffness: M.effectsK; damping: 1 }
    Spring on round { to: f.running ? 1 : 0; stiffness: M.spatialK; damping: M.spatialZ }
    Spring on ring { to: f.running ? 1 : 0; stiffness: M.fastK; damping: M.fastZ }
    Spring on shown { id: showing; to: f.running ? f.progress : 0; stiffness: M.slowK; damping: 1 }
    onRunningChanged: if (running) showing.snap(0)

    Shadow {
        anchors.fill: surface
        radius: surface.radius
        level: f.lift
        scale: surface.scale
    }
    Surface {
        id: surface
        anchors.fill: parent
        radius: 16 + (28 - 16) * Math.max(0, Math.min(1, f.round))
        interactive: true
        squish: true
        squishScale: 0.94
        color: Theme.primaryContainer
        tint: Theme.onPrimaryContainer
        Accessible.name: f.label
        onClicked: f.clicked()

        Canvas {
            id: wave
            readonly property real r: 46 / 2 - 2
            anchors.centerIn: parent
            width: 46
            height: 46
            visible: f.ring > 0.01
            opacity: Math.min(1, f.ring)
            scale: Math.max(0, f.ring)
            property real value: Math.max(0, Math.min(1, f.shown))
            property color ink: Theme.onPrimaryContainer
            onValueChanged: requestPaint()
            onInkChanged: requestPaint()
            onVisibleChanged: if (visible) requestPaint()
            onPaint: {
                const g = getContext("2d")
                g.reset()
                const cx = width / 2, cy = height / 2
                const start = -Math.PI / 2
                const gap = value > 0 && value < 1 ? 0.32 : 0
                const end = start + 2 * Math.PI * value
                g.lineWidth = 4
                g.lineCap = "round"
                if (value < 1) {
                    g.strokeStyle = Qt.rgba(ink.r, ink.g, ink.b, 0.18)
                    g.beginPath()
                    g.arc(cx, cy, r - 1.5, end + gap, start + 2 * Math.PI - gap, false)
                    g.stroke()
                }
                if (value <= 0)
                    return
                const amp = 1.5 * Math.min(1, value / 0.1)
                const waves = 8
                g.strokeStyle = ink
                g.beginPath()
                const steps = Math.max(2, Math.ceil(120 * value))
                for (let i = 0; i <= steps; ++i) {
                    const a = start + (end - start) * i / steps
                    const rr = r - 1.5 + amp * Math.sin((a - start) * waves)
                    const x = cx + rr * Math.cos(a), y = cy + rr * Math.sin(a)
                    if (i === 0)
                        g.moveTo(x, y)
                    else
                        g.lineTo(x, y)
                }
                g.stroke()
            }
        }
        Icon {
            anchors.centerIn: parent
            name: "bolt"
            size: 24
            color: Theme.onPrimaryContainer
            visible: opacity > 0.01
            opacity: 1 - Math.max(0, Math.min(1, f.ring))
            scale: 0.4 + 0.6 * (1 - Math.max(0, Math.min(1, f.ring)))
        }
        Icon {
            anchors.centerIn: parent
            name: "stop"
            size: 18
            fill: 1
            color: Theme.onPrimaryContainer
            visible: opacity > 0.01
            opacity: Math.max(0, Math.min(1, f.ring))
            scale: 0.4 + 0.6 * Math.max(0, f.ring)
        }
    }
}
