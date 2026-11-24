import QtQuick

Item {
    id: menu

    property bool open: false
    property var items: []

    property real turn: 0
    property real tone: 0
    readonly property real turned: Math.max(0, Math.min(1, turn))

    width: 56
    height: 56

    Keys.onEscapePressed: (e) => {
        e.accepted = menu.open
        menu.open = false
    }

    Spring on turn { to: menu.open ? 1 : 0; stiffness: M.fastK; damping: 0.7 }
    Spring on tone { to: menu.open ? 1 : 0; stiffness: M.effectsK; damping: 1 }

    MouseArea {
        parent: menu.parent
        anchors.fill: parent
        visible: menu.open
        z: menu.z - 0.5
        onPressed: menu.open = false
    }

    Repeater {
        model: menu.items
        Item {
            id: entry
            required property var modelData
            required property int index
            readonly property int fromEnd: menu.items.length - 1 - index
            readonly property color container: modelData.container || Theme.primaryContainer
            readonly property color content: modelData.content || Theme.onPrimaryContainer
            property real k: 0
            property real corner: 28
            readonly property real full: 68 + label.implicitWidth

            width: full
            height: 56
            x: menu.width - width
            y: -64 - fromEnd * 60 + 10 * (1 - Math.min(1, k))
            z: 81
            visible: opacity > 0
            opacity: Math.max(0, Math.min(1, (k - 0.08) * 4))

            Spring on k {
                id: unroll
                to: 0
                stiffness: menu.open ? M.fastK : M.pressInK
                damping: menu.open ? 0.65 : 1
            }
            Spring on corner {
                to: pill.down ? 12 : 28
                stiffness: pill.down ? M.pressInK : M.pressOutK
                damping: pill.down ? 1 : M.pressOutZ
            }
            Timer {
                id: stagger
                onTriggered: unroll.to = menu.open ? 1 : 0
            }
            Connections {
                target: menu
                function onOpenChanged() {
                    if (!menu.open && pill.activeFocus)
                        fab.forceActiveFocus()
                    stagger.interval = 1 + (menu.open ? entry.fromEnd * 35 : entry.index * 20)
                    stagger.restart()
                }
            }

            Shadow {
                anchors.fill: pill
                radius: pill.radius
                level: 2
            }
            Surface {
                id: pill
                x: entry.width - width
                width: 56 + (entry.full - 56) * Math.max(0, entry.k)
                height: 56
                radius: entry.corner
                interactive: true
                color: entry.container
                tint: entry.content
                Accessible.name: entry.modelData.text
                onClicked: {
                    menu.open = false
                    entry.modelData.action()
                }
                Item {
                    anchors.fill: parent
                    clip: unroll.running || entry.k < 1
                    Icon {
                        x: 16
                        anchors.verticalCenter: parent.verticalCenter
                        name: entry.modelData.icon
                        color: entry.content
                    }
                    T {
                        id: label
                        x: 48
                        anchors.verticalCenter: parent.verticalCenter
                        visible: opacity > 0
                        opacity: Math.max(0, Math.min(1, (entry.k - 0.3) * 2.5))
                        text: entry.modelData.text
                        type: "titleMedium"
                        color: entry.content
                    }
                }
            }
        }
    }

    Item {
        id: button
        property real lift: 3
        anchors.fill: parent
        z: 82

        Spring on lift { to: fab.hovered && !fab.down ? 4 : 3; stiffness: M.effectsK; damping: 1 }

        Shadow {
            anchors.fill: fab
            radius: fab.radius
            level: button.lift
            scale: fab.scale
        }
        Surface {
            id: fab
            anchors.fill: parent
            radius: 16 + 12 * menu.turned
            interactive: true
            squish: true
            squishScale: 0.94
            animateColor: false
            color: Theme.mix(Theme.primaryContainer, Theme.primary, menu.tone)
            tint: Theme.mix(Theme.onPrimaryContainer, Theme.onPrimary, menu.tone)
            Accessible.name: Ui.tr("Добавить", "Add")
            Accessible.checkable: true
            Accessible.checked: menu.open
            onClicked: menu.open = !menu.open

            Item {
                anchors.centerIn: parent
                width: 24
                height: 24
                scale: 1 - 0.17 * menu.turned
                Icon {
                    anchors.centerIn: parent
                    visible: opacity > 0
                    opacity: 1 - menu.turned
                    rotation: 135 * menu.turn
                    name: "add"
                    color: fab.tint
                }
                Icon {
                    anchors.centerIn: parent
                    visible: opacity > 0
                    opacity: menu.turned
                    rotation: 135 * (menu.turn - 1)
                    name: "close"
                    color: fab.tint
                }
            }
        }
    }
}
