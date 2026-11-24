import QtQuick

Item {
    id: d

    property string title
    property string icon
    property bool open: false
    property real maxWidth: 560
    property real preferredWidth: 0
    property bool dismissable: true
    default property alias content: body.data
    property alias buttons: trail.data
    property alias leadingButtons: lead.data
    signal closed()

    property real pop: 0
    property real fade: 0
    property real reach: 1
    property point growFrom: Qt.point(0, 0)
    property real iconPop: 1
    property Item returnTo: null
    property Item lastInside: null

    function show() {
        open = true
        if (!within(d.Window.window ? d.Window.window.activeFocusItem : null, card))
            card.forceActiveFocus()
    }
    function close() {
        if (!open)
            return
        closed()
        if (open)
            open = false
    }

    function within(item, root) { return Ui.within(item, root) }
    function move(from, by) { Ui.moveFocus(card, from, by) }

    parent: Ui.overlay
    anchors.fill: parent
    z: 100
    visible: open || fade > 0.01

    onOpenChanged: {
        if (!open) {
            Ui.giveFocusBack(d, returnTo)
            returnTo = null
            lastInside = null
            return
        }
        returnTo = Ui.takeFocus(card, returnTo)
        if (fade > 0.05)
            return
        growFrom = Ui.growFrom(width, height)
        popping.animateFrom(0)
        unfolding.animateFrom(0)
        if (icon !== "")
            iconPopping.animateFrom(0.4)
    }

    Connections {
        target: d.open ? d.Window.window : null
        function onActiveFocusItemChanged() {
            const f = d.Window.window.activeFocusItem
            if (!d.open)
                return
            if (d.within(f, card)) {
                d.lastInside = f
                return
            }
            if (f && f !== Ui.overlay && d.within(f, Ui.overlay))
                return
            const from = d.lastInside
            d.move(from, from && from.nextItemInFocusChain(false) === f ? -1 : 1)
        }
    }

    Spring on pop {
        id: popping
        to: d.open ? 1 : 0
        stiffness: d.open ? M.spatialK : M.effectsK
        damping: d.open ? 0.72 : 1
    }
    Spring on fade {
        to: d.open ? 1 : 0
        stiffness: d.open ? M.effectsK : M.effectsFastK
        damping: 1
    }
    Spring on reach {
        id: unfolding
        to: 1
        stiffness: 520
        damping: 0.68
    }
    Spring on iconPop {
        id: iconPopping
        to: 1
        stiffness: M.bouncyK
        damping: M.bouncyZ
    }

    Rectangle {
        id: scrim
        anchors.fill: parent
        color: Theme.scrim
        opacity: 0.32 * d.fade
    }
    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.AllButtons
        onClicked: if (d.dismissable) d.close()
        onWheel: (w) => w.accepted = true
    }

    FocusScope {
        id: card
        readonly property real edge: sheet.height
        function reveal(item) {
            return Math.max(0, Math.min(1, (edge - column.y - item.y) / Math.max(24, item.height * 0.6)))
        }

        anchors.centerIn: parent
        width: Math.min(d.maxWidth, d.width - 48, Math.max(280, column.implicitWidth + 48, heading.implicitWidth + 48, d.preferredWidth))
        height: Math.min(d.height - 48, column.implicitHeight + 48)
        opacity: d.fade
        transform: [
            Scale {
                origin.x: card.width / 2
                origin.y: card.height / 2
                xScale: 0.92 + 0.08 * d.pop
                yScale: 0.92 + 0.08 * d.pop
            },
            Translate {
                x: (1 - d.pop) * 0.15 * d.growFrom.x
                y: (1 - d.pop) * 0.15 * d.growFrom.y
            }
        ]
        Keys.onPressed: (e) => {
            if (e.key === Qt.Key_Escape) {
                if (d.dismissable)
                    d.close()
            } else if ((e.key === Qt.Key_Tab || e.key === Qt.Key_Backtab) && !(e.modifiers & (Qt.ControlModifier | Qt.AltModifier))) {
                const w = d.Window.window
                d.move(w ? w.activeFocusItem : null, e.key === Qt.Key_Backtab || (e.modifiers & Qt.ShiftModifier) ? -1 : 1)
            }
            e.accepted = true
        }

        Item {
            id: sheet
            width: parent.width
            height: parent.height * (0.6 + 0.4 * d.reach)
            Shadow {
                anchors.fill: parent
                radius: M.xl
                level: 3
            }
            Rectangle {
                anchors.fill: parent
                radius: M.xl
                color: Theme.surfaceContainerHigh
            }
        }
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
        }

        Column {
            id: column
            x: 24
            y: 24
            width: card.width - 48
            spacing: 16

            Icon {
                id: hero
                visible: d.icon !== ""
                name: d.icon
                color: Theme.secondary
                scale: d.iconPop
                opacity: card.reveal(hero)
                anchors.horizontalCenter: parent.horizontalCenter
            }
            T {
                id: heading
                visible: d.title !== ""
                width: parent.width
                text: d.title
                type: "headlineSmall"
                wrapMode: Text.Wrap
                elide: Text.ElideNone
                horizontalAlignment: d.icon !== "" ? Text.AlignHCenter : Text.AlignLeft
                opacity: card.reveal(heading)
                transform: Translate { y: -10 * (1 - heading.opacity) }
            }
            Flickable {
                id: scroll
                readonly property real room: d.height - 96 - (hero.visible ? hero.height + 16 : 0)
                    - (heading.visible ? heading.implicitHeight + 16 : 0) - (actions.visible ? actions.implicitHeight + 16 : 0)
                width: parent.width
                height: Math.max(0, Math.min(body.childrenRect.height, room))
                contentHeight: body.childrenRect.height
                clip: contentHeight > height
                interactive: contentHeight > height
                boundsBehavior: Flickable.StopAtBounds
                opacity: card.reveal(scroll)
                transform: Translate { y: -12 * (1 - scroll.opacity) }
                Item {
                    id: body
                    width: scroll.width
                    height: childrenRect.height
                }
            }
            Item {
                id: actions
                width: parent.width
                implicitWidth: lead.implicitWidth + (lead.implicitWidth > 0 ? 16 : 0) + trail.implicitWidth
                implicitHeight: Math.max(lead.implicitHeight, trail.implicitHeight) + 8
                height: implicitHeight
                visible: trail.children.length > 0 || lead.children.length > 0
                opacity: card.reveal(actions)
                transform: Translate { y: -12 * (1 - actions.opacity) }
                Row {
                    id: lead
                    y: 8
                    spacing: 8
                }
                Row {
                    id: trail
                    y: 8
                    anchors.right: parent.right
                    spacing: 8
                }
            }
        }
        Loader {
            active: scroll.interactive
            x: card.width - 16
            y: column.y + scroll.y
            width: 12
            height: scroll.height
            sourceComponent: Bar { flick: scroll }
        }
    }
}
