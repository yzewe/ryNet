import QtQuick

Dialog {
    id: dlg

    readonly property var st: App.autoState
    readonly property bool running: st.running === true
    readonly property var candidates: st.candidates || []
    readonly property var probes: st.probes || []
    readonly property var chosen: st.chosen || ({ sub: App.currentSub, profile: App.settings.profile || "" })

    title: Ui.tr("Почему этот сервер", "Why this server")
    preferredWidth: 480

    component Tag: Rectangle {
        id: tag
        property alias text: label.text
        property color ink: Theme.onSecondaryContainer
        property color fill: Theme.secondaryContainer
        implicitWidth: label.implicitWidth + 16
        implicitHeight: 28
        radius: M.sm
        ColorTo on color { to: tag.fill }
        T {
            id: label
            anchors.centerIn: parent
            type: "labelLarge"
            color: parent.ink
        }
    }

    Column {
        width: parent.width
        spacing: 2

        Rectangle {
            width: parent.width
            height: net.implicitHeight + 32
            radius: M.xl
            color: Theme.surfaceContainer
            Column {
                id: net
                x: 16
                y: 16
                width: parent.width - 32
                Item {
                    width: parent.width
                    height: Math.max(28, netTitle.implicitHeight)
                    T {
                        id: netTitle
                        anchors.left: parent.left
                        anchors.right: mode.left
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        text: Ui.tr("Сеть", "Network") + (dlg.st.network ? " · " + dlg.st.network : "")
                        type: "titleSmall"
                        elide: Text.ElideRight
                    }
                    Tag {
                        id: mode
                        visible: (dlg.st.at || 0) > 0
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        text: dlg.st.mode === "whitelist" ? Ui.tr("белые списки", "whitelists")
                            : dlg.st.mode === "offline" ? Ui.tr("нет доступа", "no access") : Ui.tr("обычный интернет", "open internet")
                        fill: dlg.st.mode === "whitelist" ? Theme.tertiaryContainer : dlg.st.mode === "offline" ? Theme.errorContainer : Theme.secondaryContainer
                        ink: dlg.st.mode === "whitelist" ? Theme.onTertiaryContainer : dlg.st.mode === "offline" ? Theme.onErrorContainer : Theme.onSecondaryContainer
                    }
                }
                T {
                    visible: dlg.probes.length > 0
                    topPadding: 8
                    bottomPadding: 4
                    text: Ui.tr("Напрямую", "Directly")
                    type: "labelMedium"
                    color: Theme.onSurfaceVariant
                }
                Repeater {
                    model: dlg.probes
                    Item {
                        required property var modelData
                        readonly property bool ok: modelData.ms >= 0
                        width: parent.width
                        height: 36
                        Icon {
                            id: mark
                            anchors.verticalCenter: parent.verticalCenter
                            name: parent.ok ? "check" : "close"
                            size: 20
                            color: parent.ok ? Theme.primary : Theme.error
                        }
                        T {
                            anchors.left: mark.right
                            anchors.leftMargin: 10
                            anchors.right: result.left
                            anchors.rightMargin: 8
                            anchors.verticalCenter: parent.verticalCenter
                            text: parent.modelData.host
                            type: "bodyLarge"
                            elide: Text.ElideRight
                        }
                        T {
                            id: result
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            text: parent.ok ? Ui.tr(parent.modelData.ms + " мс", parent.modelData.ms + " ms") : Ui.tr("не открывается", "doesn't open")
                            type: "labelLarge"
                            color: parent.ok ? Theme.onSurfaceVariant : Theme.error
                        }
                    }
                }
                T {
                    visible: dlg.probes.length === 0 && !((dlg.st.at || 0) > 0)
                    topPadding: 6
                    text: Ui.tr("Ещё не проверял", "Not checked yet")
                    type: "bodyMedium"
                    color: Theme.onSurfaceVariant
                }
            }
        }

        Header {
            visible: dlg.candidates.length > 0
            text: Ui.tr("Кандидаты", "Candidates") + ((dlg.st.total || 0) > 0
                ? Ui.tr(" · %1 из %2", " · %1 of %2").arg(dlg.candidates.length).arg(dlg.st.total) : "")
        }
        Repeater {
            model: dlg.candidates
            Surface {
                id: cand
                required property var modelData
                required property int index
                readonly property bool best: modelData.sub === dlg.chosen.sub && modelData.profile === dlg.chosen.profile
                readonly property bool ok: modelData.ms >= 0
                readonly property real topCorner: index === 0 ? M.lgPlus : M.xs
                readonly property real bottomCorner: index === dlg.candidates.length - 1 ? M.lgPlus : M.xs
                width: parent.width
                height: Math.max(60, info.implicitHeight + 16)
                interactive: true
                color: best ? Theme.secondaryContainer : Theme.surfaceContainer
                topLeftRadius: topCorner
                topRightRadius: topCorner
                bottomLeftRadius: bottomCorner
                bottomRightRadius: bottomCorner
                Accessible.name: modelData.name || modelData.profile
                onClicked: {
                    dlg.close()
                    App.autoChoose(modelData.sub, modelData.profile)
                }
                Item {
                    anchors.fill: parent
                    opacity: cand.ok ? 1 : 0.6
                    Flag {
                        id: flag
                        x: 12
                        anchors.verticalCenter: parent.verticalCenter
                        width: 36
                        height: 27
                        radius: M.xs
                        flag: cand.modelData.flag || ""
                    }
                    Column {
                        id: info
                        anchors.left: flag.right
                        anchors.leftMargin: 12
                        anchors.right: tail.left
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        T {
                            width: parent.width
                            text: cand.modelData.name || cand.modelData.profile
                            type: "bodyLarge"
                            color: cand.best ? Theme.onSecondaryContainer : Theme.onSurface
                            elide: Text.ElideRight
                        }
                        T {
                            readonly property string line: Ui.joined([cand.modelData.subTitle || "",
                                cand.modelData.learned ? Ui.tr("проходит при БС", "passes whitelists") : ""], " · ")
                            visible: line !== ""
                            width: parent.width
                            text: line
                            type: "bodySmall"
                            color: Theme.onSurfaceVariant
                            elide: Text.ElideRight
                        }
                    }
                    Item {
                        id: tail
                        anchors.right: parent.right
                        anchors.rightMargin: 16
                        anchors.verticalCenter: parent.verticalCenter
                        width: cand.ok ? ping.implicitWidth : fails.implicitWidth
                        height: 28
                        Tag {
                            id: fails
                            visible: !cand.ok
                            anchors.verticalCenter: parent.verticalCenter
                            text: Ui.tr("не проходит", "fails")
                            fill: Theme.errorContainer
                            ink: Theme.onErrorContainer
                        }
                        PingText {
                            id: ping
                            visible: cand.ok
                            anchors.verticalCenter: parent.verticalCenter
                            ping: ({ ms: cand.modelData.ms })
                            type: "titleMediumE"
                        }
                    }
                }
            }
        }
    }

    buttons: [
        IconBtn {
            icon: "tune"
            tip: Ui.tr("Настройки автовыбора", "Auto select settings")
            onClicked: {
                dlg.close()
                App.nav = "settings"
                App.settingsPage = "Auto"
            }
        },
        Btn {
            kind: "text"
            text: Ui.tr("Закрыть", "Close")
            onClicked: dlg.close()
        },
        Btn {
            kind: "tonal"
            icon: dlg.running ? "" : "refresh"
            busy: dlg.running
            enabled: !dlg.running
            text: dlg.running ? Ui.tr("Проверяю", "Checking") : Ui.tr("Проверить сейчас", "Check now")
            onClicked: App.autoCheck()
        }
    ]
}
