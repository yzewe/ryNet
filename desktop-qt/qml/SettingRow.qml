import QtQuick

Surface {
    id: r

    property string icon
    property string shape: "Circle"
    property string iconStyle: "plain"
    property string title
    property string summary
    property var checked: undefined
    property bool groupFirst: true
    property bool groupLast: true
    property bool danger: false
    property bool faded: false
    property bool middleElide: false
    property real trailing: 0
    property string tailIcon: ""
    property real tailTurn: 0
    signal activated()

    readonly property bool shaped: iconStyle === "shape"
    property real topEnd: 1
    property real bottomEnd: 1
    property double shownAt: 0
    readonly property real topCorner: Ui.inner + (Ui.outer - Ui.inner) * topEnd
    readonly property real bottomCorner: Ui.inner + (Ui.outer - Ui.inner) * bottomEnd

    function shapeCorners() {
        const top = groupFirst ? 1 : 0, bottom = groupLast ? 1 : 0
        if (!visible || Date.now() - shownAt < 200) {
            topMorph.snap(top)
            bottomMorph.snap(bottom)
        } else {
            topMorph.to = top
            bottomMorph.to = bottom
        }
    }

    width: parent ? parent.width : implicitWidth
    implicitHeight: Math.max(shaped ? 72 : 56, column.implicitHeight + 32)
    interactive: true
    color: Theme.surfaceContainerHigh
    topLeftRadius: Math.max(0, topCorner)
    topRightRadius: Math.max(0, topCorner)
    bottomLeftRadius: Math.max(0, bottomCorner)
    bottomRightRadius: Math.max(0, bottomCorner)
    Accessible.role: checked !== undefined ? Accessible.Switch : Accessible.Button
    Accessible.name: title
    Accessible.description: summary
    Accessible.checkable: checked !== undefined
    Accessible.checked: checked === true
    Connections {
        target: r.checked !== undefined ? r.Accessible : null
        function onToggleAction() {
            if (r.enabled)
                r.clicked(r.width / 2, r.height / 2)
        }
    }
    onClicked: {
        if (r.checked !== undefined) {
            if (r.shaped)
                glyph.item.turnStep()
            else
                plain.item.pop()
        }
        r.activated()
    }
    onGroupFirstChanged: shapeCorners()
    onGroupLastChanged: shapeCorners()
    onVisibleChanged: {
        if (visible)
            shownAt = Date.now()
    }
    Component.onCompleted: {
        shownAt = Date.now()
        shapeCorners()
    }

    Spring on topEnd { id: topMorph; stiffness: M.fastK; damping: 0.7 }
    Spring on bottomEnd { id: bottomMorph; stiffness: M.fastK; damping: 0.7 }
    HoverHandler {
        id: pointer
        enabled: r.checked !== undefined
    }

    Item {
        id: lead
        x: 16
        width: r.shaped ? 40 : 24
        height: width
        anchors.verticalCenter: parent.verticalCenter
        opacity: r.enabled && !r.faded ? 1 : 0.38
        Loader {
            id: glyph
            active: r.shaped
            sourceComponent: ShapeIcon {
                icon: r.icon
                shape: r.shape
                pressed: r.pressed
                container: r.danger ? Theme.errorContainer : Theme.primaryContainer
                color: r.danger ? Theme.onErrorContainer : Theme.onPrimaryContainer
            }
        }
        Loader {
            id: plain
            active: !r.shaped
            anchors.centerIn: parent
            sourceComponent: Icon {
                property real press: 1
                property real bounce: 1
                function pop() { popping.kick(3) }
                name: r.icon
                scale: press * bounce
                ColorTo on color { to: r.danger ? Theme.error : r.checked === true ? Theme.primary : Theme.onSurfaceVariant }
                Spring on press {
                    to: r.pressed ? 0.86 : 1
                    stiffness: r.pressed ? M.pressInK : M.pressOutK
                    damping: r.pressed ? 1 : M.pressOutZ
                }
                Spring on bounce { id: popping; to: 1; stiffness: 420; damping: 0.45 }
            }
        }
    }
    Column {
        id: column
        anchors.left: lead.right
        anchors.leftMargin: 16
        anchors.right: tail.left
        anchors.rightMargin: 16 + r.trailing
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2
        opacity: r.enabled && !r.faded ? 1 : 0.38
        T {
            width: parent.width
            text: r.title
            type: "titleMediumE"
            font.pixelSize: 15
            color: r.danger ? Theme.error : Theme.onSurface
            wrapMode: r.middleElide ? Text.NoWrap : Text.Wrap
            elide: r.middleElide ? Text.ElideMiddle : Text.ElideRight
            maximumLineCount: 2
        }
        T {
            visible: r.summary !== ""
            width: parent.width
            text: r.summary
            type: "bodyMedium"
            font.pixelSize: 13
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
            maximumLineCount: 3
        }
    }
    Item {
        id: tail
        anchors.right: parent.right
        anchors.rightMargin: 16
        anchors.verticalCenter: parent.verticalCenter
        width: r.checked !== undefined ? sw.width : 24
        height: 32
        Loader {
            id: sw
            active: r.checked !== undefined
            anchors.verticalCenter: parent.verticalCenter
            sourceComponent: Switch {
                checked: r.checked === true
                interactive: false
                pressedOutside: r.down
                hoveredOutside: pointer.hovered && contains(mapFromItem(r, pointer.point.position))
                enabled: r.enabled
            }
        }
        Loader {
            active: r.checked === undefined
            anchors.centerIn: parent
            anchors.horizontalCenterOffset: r.tailIcon === "" && item ? item.nudge : 0
            sourceComponent: Icon {
                property real nudge: 0
                name: r.tailIcon !== "" ? r.tailIcon : "chevron_right"
                rotation: r.tailTurn
                opacity: r.enabled && !r.faded ? 1 : 0.38
                Spring on nudge {
                    to: r.hovered || r.activeFocus ? (r.pressed ? 4 : 2) : 0
                    stiffness: M.fastK
                    damping: M.fastZ
                }
            }
        }
    }
}
