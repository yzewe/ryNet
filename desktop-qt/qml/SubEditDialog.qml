import QtQuick

Dialog {
    id: e

    property var sub: ({})
    property bool waiting: false
    property string tried
    property string failed
    readonly property bool local: sub.local === true
    readonly property string link: linkField.text.trim()
    readonly property bool linkOk: local || App.validSubUrl(link)
    property bool balancers: false
    property bool split: false

    function edit(s) {
        sub = s
        title = s.name || ""
        waiting = false
        failed = ""
        nameField.text = s.customName || ""
        linkField.text = s.url || ""
        split = s.split === true
        balancers = split
        App.checkBalancers(s.id)
        show()
        nameField.input.forceActiveFocus()
    }
    function hop(field) {
        if (e.local)
            return
        field.input.forceActiveFocus()
        field.input.selectAll()
    }
    function submit() {
        if (!linkOk || waiting)
            return
        waiting = true
        failed = ""
        tried = link
        App.editSub(sub.id, nameField.text, local ? "" : link, split)
    }

    icon: "edit"
    maxWidth: 560
    preferredWidth: 480

    Connections {
        target: App
        function onBalancersOf(id, has) {
            if (id === e.sub.id)
                e.balancers = has || e.split
        }
        function onSubEdited(id, error) {
            if (id !== e.sub.id || !e.waiting)
                return
            e.waiting = false
            if (error === "")
                e.close()
            else if (e.open)
                e.failed = error
            else
                App.notice = error
        }
    }

    Column {
        width: parent.width
        spacing: 8
        Field {
            id: nameField
            width: parent.width
            label: Ui.tr("Название", "Name")
            placeholder: e.sub.panelName || ""
            bg: Theme.surfaceContainerHigh
            onAccepted: e.submit()
            Keys.onTabPressed: e.hop(linkField)
            Keys.onBacktabPressed: e.hop(linkField)
        }
        Field {
            id: linkField
            visible: !e.local
            width: parent.width
            label: Ui.tr("Ссылка на подписку", "Subscription link")
            mono: true
            bg: Theme.surfaceContainerHigh
            error: e.failed !== "" && e.link === e.tried ? e.failed
                 : e.link !== "" && !e.linkOk ? Ui.tr("Неверная ссылка подписки", "Invalid subscription link") : ""
            onAccepted: e.submit()
            Keys.onTabPressed: e.hop(nameField)
            Keys.onBacktabPressed: e.hop(nameField)
        }
        Surface {
            id: splitRow
            visible: e.balancers
            width: parent.width
            height: Math.max(56, words.implicitHeight + 20)
            interactive: true
            radius: M.lg
            Accessible.role: Accessible.CheckBox
            Accessible.name: Ui.tr("Разобрать балансеры", "Split balancers")
            Accessible.checked: e.split
            onClicked: e.split = !e.split
            Column {
                id: words
                anchors.left: parent.left
                anchors.leftMargin: 16
                anchors.right: toggle.left
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                T {
                    width: parent.width
                    text: Ui.tr("Разобрать балансеры", "Split balancers")
                    type: "bodyLarge"
                    wrapMode: Text.Wrap
                }
                T {
                    width: parent.width
                    text: Ui.tr("Каждый сервер балансера — отдельной строкой", "Each balancer server on its own row")
                    type: "bodyMedium"
                    color: Theme.onSurfaceVariant
                    wrapMode: Text.Wrap
                }
            }
            Switch {
                id: toggle
                anchors.right: parent.right
                anchors.rightMargin: 16
                anchors.verticalCenter: parent.verticalCenter
                checked: e.split
                interactive: false
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: e.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Сохранить", "Save")
            busy: e.waiting
            enabled: e.linkOk
            onClicked: e.submit()
        }
    ]
}
