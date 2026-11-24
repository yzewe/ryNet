import QtQuick

Dialog {
    id: dlg

    property var groups: []
    property bool whitelist: false
    readonly property var s: App.settings

    function ask(wl) {
        whitelist = wl
        title = wl ? Ui.tr("Для белых списков", "For whitelists") : Ui.tr("Не брать", "Never use")
        show()
    }
    function on(sub, remarks) {
        return whitelist ? (s.autoWlSubs || []).indexOf(sub) >= 0 || ((s.autoWl || {})[sub] || []).indexOf(remarks) >= 0
                         : (s.autoSkipSubs || []).indexOf(sub) >= 0 || ((s.autoSkip || {})[sub] || []).indexOf(remarks) >= 0
    }

    preferredWidth: 480

    component Tag: Rectangle {
        property alias text: label.text
        implicitWidth: label.implicitWidth + 16
        implicitHeight: 24
        radius: M.sm
        color: Theme.tertiaryContainer
        T {
            id: label
            anchors.centerIn: parent
            type: "labelMedium"
            color: Theme.onTertiaryContainer
        }
    }

    Column {
        width: parent.width
        spacing: 2

        T {
            visible: dlg.whitelist
            width: parent.width
            bottomPadding: 6
            text: Ui.tr("Когда сеть пускает только по белым спискам, автовыбор сначала проверяет отмеченные серверы. Переключатель у подписки отмечает её целиком.",
                        "When the network only lets whitelisted traffic through, auto select tries the marked servers first. The switch by a subscription marks all of it.")
            type: "bodyMedium"
            color: Theme.onSurfaceVariant
            wrapMode: Text.Wrap
        }
        LoadingIndicator {
            visible: dlg.groups.length === 0
            running: visible && dlg.open
            width: 40
            height: 40
            color: Theme.primary
            anchors.horizontalCenter: parent.horizontalCenter
        }

        Repeater {
            model: dlg.groups
            Column {
                id: group
                required property var modelData
                readonly property bool whole: (dlg.whitelist ? (dlg.s.autoWlSubs || []) : (dlg.s.autoSkipSubs || [])).indexOf(modelData.sub) >= 0
                    || (modelData.servers.length > 0 && modelData.servers.every((x) => dlg.on(modelData.sub, x.remarks)))
                width: parent.width
                spacing: 2

                Surface {
                    width: parent.width
                    height: 48
                    radius: M.lg
                    interactive: true
                    color: "transparent"
                    Accessible.role: Accessible.CheckBox
                    Accessible.name: group.modelData.title
                    Accessible.checked: group.whole
                    onClicked: dlg.whitelist ? App.autoMarkSub(group.modelData.sub, !group.whole) : App.autoSkipSub(group.modelData.sub, !group.whole)
                    T {
                        anchors.left: parent.left
                        anchors.leftMargin: 4
                        anchors.right: wholeSwitch.left
                        anchors.rightMargin: 12
                        anchors.verticalCenter: parent.verticalCenter
                        text: group.modelData.title
                        type: "titleSmall"
                        color: Theme.primary
                        elide: Text.ElideRight
                    }
                    Switch {
                        id: wholeSwitch
                        visible: true
                        anchors.right: parent.right
                        anchors.rightMargin: 16
                        anchors.verticalCenter: parent.verticalCenter
                        checked: group.whole
                        interactive: false
                    }
                }
                Repeater {
                    model: group.modelData.servers
                    Surface {
                        id: row
                        required property var modelData
                        required property int index
                        readonly property bool on: dlg.on(group.modelData.sub, modelData.remarks)
                        readonly property real topCorner: index === 0 ? M.lg : M.xs
                        readonly property real bottomCorner: index === group.modelData.servers.length - 1 ? M.lg : M.xs
                        width: parent.width
                        height: 56
                        interactive: true
                        color: Theme.surfaceContainer
                        topLeftRadius: topCorner
                        topRightRadius: topCorner
                        bottomLeftRadius: bottomCorner
                        bottomRightRadius: bottomCorner
                        Accessible.role: Accessible.CheckBox
                        Accessible.name: modelData.name
                        Accessible.checked: on
                        onClicked: dlg.whitelist ? App.autoMarkServer(group.modelData.sub, modelData.remarks, !on)
                                                 : App.autoSkipServer(group.modelData.sub, modelData.remarks, !on)
                        Flag {
                            id: flag
                            x: 12
                            anchors.verticalCenter: parent.verticalCenter
                            width: 36
                            height: 27
                            radius: M.xs
                            flag: row.modelData.flag || ""
                        }
                        T {
                            anchors.left: flag.right
                            anchors.leftMargin: 12
                            anchors.right: tags.left
                            anchors.rightMargin: 8
                            anchors.verticalCenter: parent.verticalCenter
                            text: row.modelData.name
                            type: "bodyLarge"
                            elide: Text.ElideRight
                        }
                        Row {
                            id: tags
                            anchors.right: rowSwitch.left
                            anchors.rightMargin: 12
                            anchors.verticalCenter: parent.verticalCenter
                            Tag {
                                visible: !dlg.whitelist && row.modelData.passed
                                text: Ui.tr("проходит при БС", "passes whitelists")
                            }
                        }
                        Switch {
                            id: rowSwitch
                            anchors.right: parent.right
                            anchors.rightMargin: 16
                            anchors.verticalCenter: parent.verticalCenter
                            checked: row.on
                            interactive: false
                            pressedOutside: row.down
                        }
                    }
                }
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Готово", "Done")
            onClicked: dlg.close()
        }
    ]
}
