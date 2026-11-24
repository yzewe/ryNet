import QtQuick

Item {
    id: bar

    property Flickable flick
    readonly property real ratio: flick ? flick.visibleArea.heightRatio : 1
    readonly property bool needed: ratio < 0.999
    property bool active: drag.active || hover.hovered || shown.running
    property real thick: 4
    property real strength: 0

    width: 12
    visible: needed

    Timer {
        id: shown
        interval: 900
    }
    Connections {
        target: bar.flick
        function onContentYChanged() { shown.restart() }
    }

    Spring on thick {
        to: hover.hovered || drag.active ? 8 : 4
        stiffness: M.fastK
        damping: M.fastZ
    }
    Spring on strength {
        to: drag.active ? 0.7 : hover.hovered ? 0.55 : bar.active ? 0.4 : 0
        stiffness: bar.active ? M.effectsFastK : M.effectsSlowK
        damping: 1
    }

    Rectangle {
        id: handle
        x: bar.width - width - 2
        width: bar.thick
        radius: width / 2
        y: bar.flick ? bar.flick.visibleArea.yPosition * bar.height : 0
        height: Math.max(32, bar.ratio * bar.height)
        color: Theme.onSurfaceVariant
        opacity: bar.strength
        visible: opacity > 0.005
    }
    HoverHandler { id: hover }
    DragHandler {
        id: drag
        target: null
        xAxis.enabled: false
        property real startY
        onActiveChanged: if (active) startY = bar.flick.contentY
        onTranslationChanged: {
            const range = bar.flick.contentHeight - bar.flick.height
            const track = bar.height - handle.height
            if (track > 0)
                bar.flick.contentY = Math.max(0, Math.min(range, startY + translation.y * range / track))
        }
    }
}
