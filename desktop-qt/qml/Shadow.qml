import QtQuick

Item {
    id: sh
    property real radius: 16
    property real level: 2
    property color color: Theme.scrim
    readonly property real strength: Theme.dark ? 0.16 : 0.061

    Repeater {
        model: Math.max(0, Math.ceil(sh.level - 0.001)) * 2
        Rectangle {
            required property int index
            x: -index * 0.75
            width: sh.width + index * 1.5
            y: 1 + index * 0.6
            height: sh.height + index * 1.0
            radius: sh.radius + index * 0.75
            color: Qt.rgba(sh.color.r, sh.color.g, sh.color.b,
                           sh.strength * Math.max(0, Math.min(1, sh.level * 2 - index)) / (1 + index * 0.6))
        }
    }
}
