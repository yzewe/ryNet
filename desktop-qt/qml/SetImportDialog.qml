import QtQuick

Dialog {
    id: dlg

    readonly property var set: (field.text, App.parseSet(field.text))
    readonly property bool has: set.id !== undefined
    property string action: "proxy"
    property string sub
    property string profile

    function ask() {
        const clip = App.clipboard().trim()
        askWith(App.parseSet(clip).id !== undefined ? clip : "")
    }
    function askWith(link) {
        field.text = link
        sub = ""
        profile = ""
        show()
        field.input.forceActiveFocus()
    }
    onSetChanged: action = set.id !== undefined ? set.action : "proxy"

    title: Ui.tr("Набор по ссылке", "Set from a link")
    preferredWidth: 440

    Column {
        width: parent.width
        spacing: 0
        Field {
            id: field
            width: parent.width
            bg: Theme.surfaceContainerHigh
            placeholder: "rynet://set/…"
            error: text.trim() !== "" && !dlg.has ? Ui.tr("Это не ссылка на набор", "Not a set link") : ""
        }
        Item {
            id: preview
            property real h: 0
            width: parent.width
            height: h
            visible: h > 0.5
            clip: fold.running
            Spring on h { id: fold; to: dlg.has ? found.implicitHeight + 16 : 0; stiffness: M.spatialK; damping: 0.86 }
            Connections {
                target: dlg
                function onHasChanged() {
                    if (dlg.has)
                        setGlyph.play()
                }
            }
            Column {
                id: found
                y: 16
                width: parent.width
                spacing: 0
                opacity: dlg.has ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: M.effectsSlowMs; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate } }
                Row {
                    spacing: 16
                    ShapeIcon {
                        id: setGlyph
                        size: 48
                        icon: SetLook.icon(dlg.set.icon || 0)
                        shape: SetLook.shape(dlg.set.icon || 0)
                    }
                    T {
                        anchors.verticalCenter: parent.verticalCenter
                        text: SetLook.name(dlg.set)
                        type: "titleLarge"
                    }
                }
                Item { width: 1; height: 16 }
                Flow {
                    width: parent.width
                    spacing: 8
                    Repeater {
                        model: dlg.has ? Ui.setPreview(dlg.set) : []
                        Rectangle {
                            id: entryChip
                            required property var modelData
                            required property int index
                            property real pop: 1
                            scale: pop
                            opacity: 0
                            Spring on pop { id: popSpring; to: 1; stiffness: 520; damping: 0.62 }
                            NumberAnimation { id: popFade; target: entryChip; property: "opacity"; to: 1; duration: 180; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate }
                            Timer {
                                interval: 60 + Math.min(entryChip.index, 14) * 28
                                running: true
                                onTriggered: {
                                    popSpring.animateFrom(0.6)
                                    popFade.start()
                                }
                            }
                            height: 32
                            width: Math.min(260, r.implicitWidth + 24)
                            radius: M.sm
                            color: Theme.surfaceContainerHighest
                            Row {
                                id: r
                                x: 8
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 8
                                Icon { name: modelData[0]; size: 18; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
                                T { text: modelData[1]; type: "labelLarge"; anchors.verticalCenter: parent.verticalCenter; width: Math.min(implicitWidth, 218); elide: Text.ElideMiddle }
                            }
                        }
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
                        active: dlg.has && dlg.action === "server" && dlg.open
                        sourceComponent: ServerPicker {
                            profile: dlg.profile
                            onPicked: (s, p) => {
                                dlg.sub = s
                                dlg.profile = p
                            }
                        }
                    }
                }
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Из файла", "From file")
            onClicked: App.pickFile(false, "", Ui.tr("Набор правил", "Rule set"), "txt", (path) => {
                if (path)
                    field.text = App.readText(path, 1048576)
            })
        },
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: dlg.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Добавить", "Add")
            enabled: dlg.has && dlg.set.size > 0 && (dlg.action !== "server" || dlg.profile !== "")
            onClicked: {
                const s = Object.assign({}, dlg.set)
                s.action = dlg.action
                s.sub = dlg.action === "server" ? dlg.sub : ""
                s.profile = dlg.action === "server" ? dlg.profile : ""
                App.saveSet(s)
                dlg.close()
            }
        }
    ]
}
