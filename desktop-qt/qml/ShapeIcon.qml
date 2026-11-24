import QtQuick

Item {
    id: s

    property string icon
    property string shape: "Circle"
    property color container: Theme.primaryContainer
    property color color: Theme.onPrimaryContainer
    property real size: 40
    property bool pressed: false
    property real turn: 0
    property real pop: 1
    property real press: 1

    readonly property real step: ({
        Circle: 90, Square: 90, Pill: 180, Oval: 180, Diamond: 180, Sunny: 45, VerySunny: 45,
        Cookie4Sided: 90, Cookie6Sided: 60, Cookie7Sided: 360 / 7, Cookie9Sided: 40, Cookie12Sided: 30,
        Clover4Leaf: 90, Clover8Leaf: 45, Burst: 30, SoftBurst: 30, Flower: 45, Puffy: 180
    })[shape] || 360

    function play() {
        turnStep()
        popper.kick(2.4)
    }
    function turnStep() {
        spin.to = spin.to + step * Math.ceil(80 / step)
    }

    implicitWidth: size
    implicitHeight: size
    scale: pop

    Spring on turn {
        id: spin
        to: 0
        stiffness: 240
        damping: 0.62
        onRunningChanged: if (!running) snap(0)
    }
    Spring on pop { id: popper; to: 1; stiffness: 420; damping: 0.45 }
    Spring on press {
        to: s.pressed ? 0.86 : 1
        stiffness: s.pressed ? M.pressInK : M.pressOutK
        damping: s.pressed ? 1 : M.pressOutZ
    }

    MShape {
        anchors.fill: parent
        shape: s.shape
        rotation: s.turn
        scale: s.press
        ColorTo on color { to: s.container; duration: 220 }
    }
    Icon {
        anchors.centerIn: parent
        name: s.icon
        size: Math.round(s.size * 0.55)
        scale: 0.5 + 0.5 * s.press
        ColorTo on color { to: s.color; duration: 220 }
    }
}
