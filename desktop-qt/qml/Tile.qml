import QtQuick

Surface {
    id: t

    property var info: ({})
    readonly property bool wide: info.wide === true
    readonly property bool selected: info.selected === true
    readonly property bool waiting: info.waiting === true
    readonly property color content: selected ? Theme.onSecondaryContainer : Theme.onSurface
    readonly property color dim: selected ? Theme.alpha(Theme.onSecondaryContainer, 0.72) : Theme.onSurfaceVariant
    property real shape: M.lg
    signal picked()
    signal menu(real x, real y)

    radius: shape
    interactive: true
    rightClicks: true
    squish: true
    color: selected ? Theme.secondaryContainer : info.focused ? Theme.surfaceContainerHighest : Theme.surfaceContainerHigh
    tint: content
    opacity: info.supported === false ? 0.38 : 1
    Accessible.name: info.name || ""
    Accessible.description: info.line || ""
    Accessible.selected: selected

    onClicked: (x, y) => info.supported ? picked() : menu(x, y)
    onRightClicked: (x, y) => menu(x, y)
    onLongPressed: (x, y) => menu(x, y)
    onWaitingChanged: waiting ? busy.come() : pingText.bump()

    Spring on shape { to: t.wide ? M.lgPlus : M.lg; stiffness: M.fastK; damping: 0.8 }

    Flag {
        id: flag
        x: 8
        y: 8
        height: t.height - 16
        width: height * 4 / 3
        radius: t.shape - 8
        flag: t.info.flag || ""
    }
    Column {
        anchors.left: flag.right
        anchors.leftMargin: t.wide ? 16 : 12
        anchors.right: parent.right
        anchors.rightMargin: t.wide ? 16 : 12
        anchors.verticalCenter: parent.verticalCenter
        spacing: 1
        T {
            width: parent.width
            text: t.info.name || ""
            type: t.wide ? (t.selected ? "titleMediumE" : "titleMedium") : (t.selected ? "titleSmallE" : "titleSmall")
            ColorTo on color { to: t.content }
        }
        Item {
            width: parent.width
            height: Math.max(line.implicitHeight, 16)
            visible: line.text !== "" || pingText.visible || busy.visible
            T {
                id: line
                anchors.left: parent.left
                anchors.right: tail.left
                anchors.rightMargin: 6
                anchors.verticalCenter: parent.verticalCenter
                text: t.info.line || ""
                type: t.wide ? "labelLarge" : "labelMedium"
                color: t.info.lineError ? (t.selected ? t.content : Theme.error) : t.dim
            }
            Item {
                id: tail
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                width: busy.visible ? busy.width : pingText.visible ? pingText.implicitWidth : 0
                height: 20
                LoadingIndicator {
                    id: busy
                    property real pop: 1
                    function come() {
                        grow.animateFrom(0.3)
                    }
                    anchors.centerIn: parent
                    width: t.wide ? 22 : 20
                    height: width
                    visible: t.waiting
                    running: visible
                    color: t.selected ? t.content : Theme.primary
                    scale: pop
                    Spring on pop { id: grow; to: 1; stiffness: M.fastK; damping: 0.7 }
                }
                PingText {
                    id: pingText
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    visible: !busy.visible && t.info.supported === true && (t.info.ping >= 0 || t.info.pingFail === true)
                    ping: t.info.ping >= 0 ? { ms: t.info.ping } : { error: "x" }
                    type: t.wide ? "labelLargeE" : "labelMediumE"
                }
            }
        }
    }
}
