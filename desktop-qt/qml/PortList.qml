import QtQuick

Column {
    id: pl

    readonly property var s: App.settings
    readonly property var entries: {
        const out = []
        for (const n of String(s.directPorts || "").split(/[,;\r\n]/).map(x => x.trim())) {
            if (n !== "" && !out.includes(n))
                out.push(n)
        }
        return out
    }
    readonly property string typed: field.text.trim()
    readonly property string normal: {
        const m = /^(\d{1,5})\s*(?:-\s*(\d{1,5}))?$/.exec(typed)
        if (!m)
            return ""
        const a = parseInt(m[1]), b = m[2] === undefined ? a : parseInt(m[2])
        if (a < 1 || b > 65535 || (m[2] !== undefined && a >= b))
            return ""
        return m[2] === undefined ? String(a) : a + "-" + b
    }
    readonly property string problem: {
        if (typed === "")
            return ""
        if (normal === "")
            return Ui.tr("Порт от 1 до 65535 или диапазон 8000-8100", "A port from 1 to 65535, or a range like 8000-8100")
        if (entries.includes(normal))
            return Ui.tr("Уже в списке", "Already in the list")
        return ""
    }

    function add() {
        if (typed === "" || problem !== "")
            return
        App.set("directPorts", entries.concat([normal]).join(", "), true)
        field.text = ""
    }
    function remove(i) {
        App.set("directPorts", entries.filter((n, j) => j !== i).join(", "), true)
    }

    readonly property bool carded: parent !== null && parent.objectName === "cardInner"
    readonly property int side: carded ? 0 : 16

    width: parent ? parent.width : 0

    T {
        width: parent.width - 2 * pl.side
        x: pl.side
        topPadding: pl.carded ? 0 : 20
        bottomPadding: 12
        text: Ui.tr("Порты напрямую", "Ports that go direct")
        type: pl.carded ? "titleMediumE" : "titleSmall"
        font.pixelSize: pl.carded ? 15 : spec[0]
        color: pl.carded ? Theme.onSurface : Theme.primary
    }
    T {
        width: parent.width - 2 * pl.side
        x: pl.side
        bottomPadding: 8
        text: Ui.tr("Трафик на эти порты (TCP и UDP) идёт напрямую, минуя VPN", "Traffic to these ports (TCP and UDP) goes direct, bypassing the VPN")
        type: "bodySmall"
        color: Theme.onSurfaceVariant
        wrapMode: Text.Wrap
    }
    Column {
        width: parent.width
        Repeater {
            model: pl.entries
            Item {
                required property string modelData
                required property int index
                width: pl.width
                height: 48
                T {
                    anchors.left: parent.left
                    anchors.leftMargin: pl.side
                    anchors.right: del.left
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData
                    type: "bodyLarge"
                    color: Theme.onSurface
                    elide: Text.ElideRight
                }
                IconBtn {
                    id: del
                    anchors.right: parent.right
                    anchors.rightMargin: 4
                    anchors.verticalCenter: parent.verticalCenter
                    icon: "close"
                    tip: Ui.tr("Убрать", "Remove")
                    onClicked: pl.remove(index)
                }
            }
        }
    }
    Item {
        width: parent.width
        height: field.implicitHeight + 16
        Field {
            id: field
            anchors.left: parent.left
            anchors.right: addBtn.left
            anchors.rightMargin: 8
            anchors.top: parent.top
            anchors.topMargin: 8
            mono: true
            placeholder: "443 / 8000-8100"
            error: pl.problem
            onAccepted: pl.add()
        }
        IconBtn {
            id: addBtn
            anchors.right: parent.right
            anchors.rightMargin: 4
            y: 8 + (56 - height) / 2
            icon: "add"
            kind: "tonal"
            enabled: pl.typed !== "" && pl.problem === ""
            tip: Ui.tr("Добавить", "Add")
            onClicked: pl.add()
        }
    }
}
