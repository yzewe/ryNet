import QtQuick

Column {
    id: d

    property string icon
    property string title
    property var options: []
    property var labels: []
    property var selected
    property string summary: {
        const i = options.indexOf(selected)
        return i >= 0 ? String(labels[i] !== undefined ? labels[i] : options[i]) : ""
    }
    property bool groupFirst: true
    property bool groupLast: true
    property bool open: false
    signal picked(var value)
    signal reselected(var value)

    property real reach: open ? 1 : 0
    property real shown: open ? 1 : 0
    Behavior on reach { NumberAnimation { duration: d.open ? 220 : 180; easing.type: Easing.BezierSpline; easing.bezierCurve: M.standard } }
    Behavior on shown { NumberAnimation { duration: d.open ? 180 : 140; easing.type: Easing.BezierSpline; easing.bezierCurve: d.open ? M.decelerate : M.accelerate } }

    property real turn: 0
    Spring on turn { to: d.open ? 1 : 0; stiffness: M.spatialK; damping: 0.8 }

    width: parent ? parent.width : 0
    spacing: 0
    onEnabledChanged: if (!enabled) open = false

    SettingRow {
        id: head
        icon: d.icon
        title: d.title
        summary: d.summary
        groupFirst: d.groupFirst
        groupLast: d.groupLast && !d.open
        tailIcon: "expand_more"
        tailTurn: 180 * d.turn
        Accessible.description: d.summary
        onActivated: d.open = !d.open
    }
    Item {
        id: holder
        width: parent.width
        height: (list.implicitHeight + Ui.gap) * d.reach
        visible: d.reach > 0.005
        clip: d.reach < 0.999
        Column {
            id: list
            y: Ui.gap
            width: parent.width
            spacing: Ui.gap
            opacity: d.shown
            Repeater {
                model: d.options.length
                Surface {
                    id: option
                    required property int index
                    readonly property bool on: d.options[index] === d.selected
                    readonly property bool last: index === d.options.length - 1
                    property real mark: 0
                    width: list.width
                    height: 52
                    interactive: true
                    color: Theme.surfaceContainerHigh
                    topLeftRadius: Ui.inner
                    topRightRadius: Ui.inner
                    bottomLeftRadius: last && d.groupLast ? Ui.outer : Ui.inner
                    bottomRightRadius: last && d.groupLast ? Ui.outer : Ui.inner
                    Accessible.role: Accessible.RadioButton
                    Accessible.name: label.text
                    Accessible.checkable: true
                    Accessible.checked: on
                    Spring on mark { id: marking; to: option.on ? 1 : 0; stiffness: M.bouncyK; damping: M.bouncyZ }
                    Component.onCompleted: marking.snap(on ? 1 : 0)
                    onClicked: {
                        d.open = false
                        if (!on)
                            d.picked(d.options[index])
                        else
                            d.reselected(d.options[index])
                    }
                    T {
                        id: label
                        x: 56
                        width: parent.width - 56 - 16 - 24 - 12
                        anchors.verticalCenter: parent.verticalCenter
                        text: String(d.labels[option.index] !== undefined ? d.labels[option.index] : d.options[option.index])
                        type: "titleMediumE"
                        font.pixelSize: 15
                        ColorTo on color { to: option.on ? Theme.primary : Theme.onSurface }
                    }
                    Icon {
                        anchors.right: parent.right
                        anchors.rightMargin: 16
                        anchors.verticalCenter: parent.verticalCenter
                        name: "check"
                        color: Theme.primary
                        scale: Math.max(0, option.mark)
                        opacity: Math.max(0, Math.min(1, option.mark))
                        visible: option.mark > 0.01
                    }
                }
            }
        }
    }
}
