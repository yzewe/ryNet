import QtQuick

Dialog {
    id: dlg

    property string remarks
    property var config: null
    property bool jsonOpen: false
    readonly property bool hasJson: config !== null && (config.json || "") !== ""

    function showFor(r) {
        remarks = r
        config = null
        jsonOpen = false
        App.details(r)
        show()
    }

    title: Ui.tr("Подробности", "Details")
    preferredWidth: 520

    Connections {
        target: App
        function onDetailsReady(r, details) {
            if (r === dlg.remarks)
                dlg.config = details
        }
    }

    Column {
        width: parent.width
        spacing: 0

        LoadingIndicator {
            visible: dlg.config === null
            running: visible
            anchors.horizontalCenter: parent.horizontalCenter
            width: 48
            height: 48
            color: Theme.primary
        }
        Notice {
            visible: dlg.config !== null && (dlg.config.error || "") !== ""
            width: parent.width
            icon: "close"
            error: true
            text: dlg.config ? dlg.config.error || "" : ""
        }

        Repeater {
            model: dlg.config && dlg.config.groups ? dlg.config.groups : []
            Column {
                id: group
                required property var modelData
                required property int index
                width: parent.width
                Header {
                    first: group.index === 0
                    text: group.modelData.title
                }
                Column {
                    width: parent.width
                    spacing: 2
                    Repeater {
                        model: group.modelData.rows
                        Rectangle {
                            id: line
                            required property var modelData
                            required property int index
                            readonly property int total: group.modelData.rows.length
                            property bool shown: false
                            readonly property bool copyable: modelData.copy === true || modelData.secret === true
                            width: parent.width
                            height: Math.max(52, value.implicitHeight + 16)
                            color: Theme.surfaceContainerHighest
                            topLeftRadius: index === 0 ? M.lg : M.xs
                            topRightRadius: index === 0 ? M.lg : M.xs
                            bottomLeftRadius: index === total - 1 ? M.lg : M.xs
                            bottomRightRadius: index === total - 1 ? M.lg : M.xs
                            T {
                                x: 16
                                width: 112
                                anchors.verticalCenter: parent.verticalCenter
                                text: line.modelData.k
                                type: "labelLarge"
                                color: Theme.onSurfaceVariant
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                            }
                            T {
                                id: value
                                x: 136
                                width: parent.width - x - 8 - (eye.visible ? 40 : 0) - (copy.visible ? 40 : 0)
                                anchors.verticalCenter: parent.verticalCenter
                                text: line.modelData.secret === true && !line.shown ? "••••••••" + line.modelData.v.slice(-4) : line.modelData.v
                                type: "bodyMedium"
                                font.family: line.modelData.mono === true ? "monospace" : M.font
                                wrapMode: Text.WrapAnywhere
                                maximumLineCount: 3
                            }
                            IconBtn {
                                id: eye
                                visible: line.modelData.secret === true
                                anchors.right: copy.visible ? copy.left : parent.right
                                anchors.rightMargin: copy.visible ? 0 : 4
                                anchors.verticalCenter: parent.verticalCenter
                                icon: line.shown ? "visibility_off" : "visibility"
                                iconSize: 20
                                tip: line.shown ? Ui.tr("Скрыть", "Hide") : Ui.tr("Показать", "Show")
                                onClicked: line.shown = !line.shown
                            }
                            IconBtn {
                                id: copy
                                visible: line.copyable
                                anchors.right: parent.right
                                anchors.rightMargin: 4
                                anchors.verticalCenter: parent.verticalCenter
                                icon: "content_copy"
                                iconSize: 20
                                tip: Ui.tr("Скопировать", "Copy")
                                onClicked: {
                                    App.copy(line.modelData.v)
                                    App.notice = Ui.tr("Скопировано", "Copied")
                                }
                            }
                        }
                    }
                }
            }
        }

        Item {
            visible: dlg.hasJson
            width: 1
            height: 16
        }
        Rectangle {
            visible: dlg.hasJson
            width: parent.width
            height: jsonHead.height + jsonBox.height + 12
            radius: M.lg
            color: Theme.surfaceContainerHighest
            Item {
                id: jsonHead
                width: parent.width
                height: 48
                T {
                    x: 16
                    anchors.verticalCenter: parent.verticalCenter
                    text: Ui.tr("Конфиг JSON", "Config JSON")
                    type: "titleSmall"
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: 4
                    anchors.verticalCenter: parent.verticalCenter
                    IconBtn {
                        icon: "content_copy"
                        iconSize: 20
                        tip: Ui.tr("Скопировать", "Copy")
                        onClicked: {
                            App.copy(dlg.config.json)
                            App.notice = Ui.tr("Скопировано", "Copied")
                        }
                    }
                    IconBtn {
                        visible: dlg.hasJson && dlg.config.long === true
                        icon: dlg.jsonOpen ? "keyboard_arrow_up" : "keyboard_arrow_down"
                        iconSize: 20
                        tip: dlg.jsonOpen ? Ui.tr("Свернуть", "Collapse") : Ui.tr("Развернуть", "Expand")
                        onClicked: dlg.jsonOpen = !dlg.jsonOpen
                    }
                }
            }
            Item {
                id: jsonBox
                property real h: json.implicitHeight
                y: jsonHead.height
                width: parent.width
                height: h
                clip: unfold.running
                Spring on h { id: unfold; to: json.implicitHeight; stiffness: M.spatialK; damping: 0.9 }
                T {
                    id: json
                    x: 16
                    width: parent.width - 32
                    text: dlg.hasJson ? (dlg.jsonOpen ? dlg.config.json : dlg.config.short) : ""
                    type: "bodySmall"
                    font.family: "monospace"
                    color: Theme.onSurfaceVariant
                    wrapMode: Text.WrapAnywhere
                    elide: Text.ElideNone
                }
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Закрыть", "Close")
            onClicked: dlg.close()
        }
    ]
}
