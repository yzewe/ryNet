import QtQuick

Item {
    id: c

    readonly property bool on: App.settings.chain === true
    readonly property var live: (App.rev < 0 ? undefined : on ? App.chainServer() : null)
    property var entry: ({})
    readonly property bool hasEntry: entry.remarks !== undefined
    readonly property real flagMid: 8 + 75 / 2
    property real shown: 0
    property bool settled: false

    height: (chip.height + 20) * Math.max(0, Math.min(1, shown))
    visible: shown > 0.004
    onLiveChanged: if (live) entry = live
    Component.onCompleted: if (live) entry = live

    Spring on shown {
        to: c.on ? 1 : 0
        stiffness: c.on ? M.spatialK : M.effectsK
        damping: c.on ? 0.8 : 1
    }
    Timer {
        running: true
        interval: 300
        onTriggered: c.settled = true
    }

    Surface {
        id: chip
        x: c.flagMid - 14 - 10
        height: 40
        width: 26
        radius: height / 2
        color: Theme.surfaceContainerHigh
        interactive: true
        squish: true
        opacity: Math.min(1, c.shown * 1.4)
        transform: Scale {
            origin.x: 24
            origin.y: chip.height
            xScale: 0.8 + 0.2 * c.shown
            yScale: 0.8 + 0.2 * c.shown
        }
        Accessible.name: c.hasEntry ? Ui.tr("через ", "via ") + (c.entry.name || "") : Ui.tr("Выберите вход цепочки", "Choose chain entry")
        onClicked: App.openEntryPicker()

        Spring on width {
            enabled: c.settled
            to: row.implicitWidth + 26
            stiffness: M.fastK
            damping: 0.8
        }

        Row {
            id: row
            x: 10
            anchors.verticalCenter: parent.verticalCenter
            spacing: 10
            Flag {
                visible: c.hasEntry
                width: 28
                height: 21
                radius: 6
                flag: c.entry.flag || ""
                anchors.verticalCenter: parent.verticalCenter
            }
            Icon {
                visible: !c.hasEntry
                name: "link"
                size: 20
                color: Theme.primary
                anchors.verticalCenter: parent.verticalCenter
            }
            Row {
                anchors.verticalCenter: parent.verticalCenter
                T {
                    text: c.hasEntry ? Ui.tr("через ", "via ") : Ui.tr("Выберите вход цепочки", "Choose chain entry")
                    type: "labelLarge"
                    color: c.hasEntry ? Theme.onSurfaceVariant : Theme.primary
                }
                T {
                    visible: c.hasEntry
                    text: c.entry.name || ""
                    type: "labelLarge"
                }
            }
        }
    }
    ChainWave {
        x: c.flagMid - 12
        y: chip.height
        width: 24
        height: 20
        running: c.on && App.phase === 2
        color: Theme.primary
        opacity: (running ? 1 : 0.55) * Math.max(0, Math.min(1, c.shown * 2 - 1))
    }
}
