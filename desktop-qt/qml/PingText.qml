import QtQuick

T {
    id: t

    property var ping
    readonly property int ms: ping && ping.ms !== undefined ? ping.ms : -1
    property color shade: Ui.pingColor(ms)
    property color tone
    property real pop: 1
    property bool rolling: false

    function bump() {
        popper.animateFrom(0.86)
        rolling = false
        rolling = true
        done.restart()
    }

    text: ms >= 0 ? String(ms) : "—"
    color: rolling ? "transparent" : tone
    type: "labelLargeE"
    font.features: { "tnum": 1 }
    scale: pop

    ColorTo on tone { to: t.shade }
    Spring on pop { id: popper; to: 1; stiffness: M.fastK; damping: 0.5 }
    Timer {
        id: done
        interval: 420 + 40 * t.text.length
        onTriggered: t.rolling = false
    }

    Row {
        x: t.leftPadding
        y: t.topPadding
        Repeater {
            model: t.rolling ? t.text.split("") : []
            Text {
                id: digit
                required property string modelData
                required property int index
                property real rise: 0
                text: modelData
                font: t.font
                color: t.tone
                opacity: 0
                transform: Translate { y: digit.rise }
                Spring on rise { id: riser; stiffness: M.fastK; damping: 0.62 }
                NumberAnimation {
                    id: fade
                    target: digit
                    property: "opacity"
                    to: 1
                    duration: 160
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: M.decelerate
                }
                Timer {
                    interval: 1 + digit.index * 40
                    running: true
                    onTriggered: {
                        riser.to = 0
                        fade.start()
                    }
                }
                Component.onCompleted: riser.snap(t.font.pixelSize * 0.55)
            }
        }
    }
}
