import QtQuick

Window {
    id: win
    width: 1100
    height: 720
    title: "ryNet"
    color: Theme.surfaceContainer

    Shell {
        anchors.fill: parent
    }

    Item {
        id: overlay
        anchors.fill: parent
        z: 10
        Component.onCompleted: Ui.overlay = overlay

        Item {
            id: tip
            property string text
            property real ax: 0
            property real ay: 0
            property bool below: true
            readonly property bool shown: Ui.tipText !== "" && Ui.tipItem !== null
            property real fade: 0
            property real grow: 1

            z: 200
            anchors.fill: parent
            visible: fade > 0.01

            Spring on fade {
                to: tip.shown ? 1 : 0
                stiffness: tip.shown ? M.effectsK : M.effectsFastK
                damping: 1
            }
            Spring on grow {
                id: growing
                to: 1
                stiffness: M.fastK
                damping: 0.7
            }

            Connections {
                target: Ui
                function onTipTextChanged() {
                    const item = Ui.tipItem
                    if (Ui.tipText === "" || !item)
                        return
                    const fresh = tip.fade < 0.3
                    tip.text = Ui.tipText
                    const p = item.mapToItem(overlay, item.width / 2, item.height)
                    tip.below = p.y + 4 + box.height <= overlay.height - 8
                    tip.ax = p.x
                    tip.ay = tip.below ? p.y : p.y - item.height
                    if (fresh)
                        growing.animateFrom(0.88)
                }
            }

            Rectangle {
                id: box
                width: Math.min(200, label.implicitWidth + 16)
                height: Math.max(24, label.implicitHeight + 8)
                x: Math.max(8, Math.min(overlay.width - width - 8, tip.ax - width / 2))
                y: tip.below ? tip.ay + 4 : tip.ay - 4 - height
                radius: M.xs
                color: Theme.inverseSurface
                opacity: tip.fade
                transform: Scale {
                    origin.x: Math.max(0, Math.min(box.width, tip.ax - box.x))
                    origin.y: tip.below ? 0 : box.height
                    xScale: tip.grow
                    yScale: tip.grow
                }

                T {
                    id: label
                    x: 8
                    width: Math.min(184, implicitWidth)
                    anchors.verticalCenter: parent.verticalCenter
                    text: tip.text
                    type: "bodySmall"
                    color: Theme.inverseOnSurface
                    wrapMode: Text.Wrap
                    elide: Text.ElideNone
                }
            }
        }
    }
}
