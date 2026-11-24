import QtQuick

Item {
    id: a

    property int index: 0
    property real lift: 24
    property int dir: 1
    property real from: 0.94
    property int step: 45
    property real p: 1
    property real shade: 1
    default property alias content: holder.data

    function play(d) {
        if (d !== undefined)
            dir = d
        rise.snap(0)
        fade.snap(0)
        wait.restart()
    }

    implicitWidth: holder.childrenRect.width
    implicitHeight: holder.childrenRect.height
    opacity: Math.max(0, Math.min(1, shade))
    transform: [
        Translate { y: (1 - a.p) * a.lift * a.dir },
        Scale {
            origin.x: a.width / 2
            origin.y: a.height / 2
            xScale: a.from + (1 - a.from) * a.p
            yScale: a.from + (1 - a.from) * a.p
        }
    ]

    Spring on p { id: rise; to: 1; stiffness: M.spatialK; damping: 0.7 }
    Spring on shade { id: fade; to: 1; stiffness: M.effectsK; damping: 1 }
    Timer {
        id: wait
        interval: Math.max(1, a.index * a.step)
        onTriggered: {
            rise.to = 1
            fade.to = 1
        }
    }

    Item {
        id: holder
        anchors.fill: parent
    }
}
