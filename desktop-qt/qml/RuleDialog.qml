import QtQuick

Dialog {
    id: dlg

    property int index: -1
    property string kind: "domain"
    property string action: "direct"
    property string sub
    property string profile
    readonly property var rule: ({ kind: kind, value: field.text.trim(), action: action, sub: sub, profile: profile })
    readonly property bool valid: (field.text, kind, action, sub, profile, App.ruleValid(rule))
    property var procs: []
    property var warnings: []
    readonly property string coverKey: index < 0 && valid ? kind + "\n" + rule.value + "\n" + action : ""
    onCoverKeyChanged: {
        warnings = []
        if (coverKey !== "")
            coverTimer.restart()
    }
    Timer {
        id: coverTimer
        interval: 300
        onTriggered: {
            const key = dlg.coverKey
            if (key !== "")
                App.coverWarnings(dlg.kind, dlg.rule.value, dlg.action, (w) => {
                    if (dlg.coverKey === key)
                        dlg.warnings = w
                })
        }
    }
    signal saving(int index)

    function edit(i, r) {
        index = i
        kind = r ? r.kind : "domain"
        action = r ? r.action : App.settings.routingMode === "direct" ? "proxy" : "direct"
        sub = r ? r.sub : ""
        profile = r ? r.profile : ""
        field.text = r ? r.value : ""
        title = r ? Ui.tr("Правило", "Rule") : Ui.tr("Новое правило", "New rule")
        show()
        field.input.forceActiveFocus()
    }
    onKindChanged: procs = kind === "app" ? App.processes() : []

    preferredWidth: 440

    Column {
        width: parent.width
        spacing: 0
        Choice {
            width: parent.width
            options: ["domain", "ip", "app"]
            labels: [Ui.tr("Домен", "Domain"), "IP", Ui.tr("Приложение", "App")]
            selected: dlg.kind
            onPicked: (v) => dlg.kind = v
        }
        Item { width: 1; height: 16 }
        Field {
            id: field
            width: parent.width
            bg: Theme.surfaceContainerHigh
            placeholder: dlg.kind === "domain" ? "youtube.com" : dlg.kind === "ip" ? "1.2.3.0/24" : "firefox"
            error: text.trim() !== "" && !dlg.valid ? Ui.tr("Неверное значение", "Invalid value") : ""
        }
        Item {
            id: suggBox
            readonly property var matches: Ui.suggest(dlg.procs, field.text, 30)
            readonly property bool shown: dlg.kind === "app" && matches.length > 0
            property real h: 0
            width: parent.width
            height: h
            visible: h > 0.5
            clip: suggFold.running
            Spring on h { id: suggFold; to: suggBox.shown ? 48 : 0; stiffness: M.spatialK; damping: 0.86 }
            Flickable {
                x: -6
                y: 10
                width: parent.width + 12
                height: 44
                opacity: suggBox.shown ? 1 : 0
                contentWidth: sugg.implicitWidth + 12
                flickableDirection: Flickable.HorizontalFlick
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                Behavior on opacity { NumberAnimation { duration: M.effectsMs; easing.type: Easing.OutCubic } }
                Row {
                    id: sugg
                    x: 6
                    y: 6
                    spacing: 8
                    Repeater {
                        model: suggBox.matches
                        Chip {
                            required property string modelData
                            text: modelData
                            onClicked: field.text = modelData
                        }
                    }
                }
            }
        }
        Repeater {
            model: dlg.warnings
            T {
                required property string modelData
                width: parent.width
                topPadding: 8
                leftPadding: 4
                text: modelData
                type: "bodySmall"
                color: Theme.tertiary
                wrapMode: Text.Wrap
            }
        }
        Item { width: 1; height: 16 }
        Choice {
            width: parent.width
            options: ["proxy", "direct", "block", "server"]
            labels: [Ui.tr("Прокси", "Proxy"), Ui.tr("Напрямую", "Direct"), Ui.tr("Блок", "Block"), Ui.tr("Сервер", "Server")]
            selected: dlg.action
            onPicked: (v) => dlg.action = v
        }
        Item {
            id: pickerBox
            property real h: 0
            width: parent.width
            height: h
            visible: h > 0.5
            clip: pickerFold.running
            Spring on h { id: pickerFold; to: picker.item ? picker.item.height + 16 : 0; stiffness: M.spatialK; damping: 0.86 }
            Loader {
                id: picker
                y: 16
                width: parent.width
                active: dlg.action === "server" && dlg.open
                sourceComponent: ServerPicker {
                    sub: dlg.sub !== "" ? dlg.sub : App.currentSub
                    profile: dlg.profile
                    onPicked: (s, p) => {
                        dlg.sub = s
                        dlg.profile = p
                    }
                }
            }
        }
    }

    leadingButtons: [
        Btn {
            visible: dlg.index >= 0
            kind: "danger"
            text: Ui.tr("Удалить", "Delete")
            onClicked: {
                App.deleteRule(dlg.index)
                dlg.close()
            }
        }
    ]
    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Сохранить", "Save")
            enabled: dlg.valid
            onClicked: {
                dlg.saving(dlg.index)
                App.saveRule(dlg.index, dlg.rule)
                dlg.close()
            }
        }
    ]
}
