import QtQuick

Column {
    id: pl
    objectName: carded ? "cardInner" : ""

    readonly property var s: App.settings
    readonly property var names: {
        const out = []
        for (const n of String(s.processes || "").split(/[,;\r\n]/).map(x => x.trim())) {
            if (n !== "" && !out.some(o => o.toLowerCase() === n.toLowerCase()))
                out.push(n)
        }
        return out
    }
    readonly property string typed: field.text.trim()
    readonly property string problem: {
        if (typed === "")
            return ""
        if (/[\\/,;]/.test(typed))
            return Ui.tr("Нужно имя файла (chrome.exe), не путь", "A file name is needed (chrome.exe), not a path")
        if (names.some(o => o.toLowerCase() === typed.toLowerCase()))
            return Ui.tr("Уже в списке", "Already in the list")
        return ""
    }

    function add() {
        if (typed === "" || problem !== "")
            return
        App.set("processes", names.concat([typed]).join(", "), true)
        field.text = ""
    }
    function remove(i) {
        App.set("processes", names.filter((n, j) => j !== i).join(", "), true)
    }

    readonly property bool carded: parent !== null && parent.objectName === "cardInner"
    readonly property int side: carded ? 0 : 16

    width: parent ? parent.width : 0

    ChoiceBlock {
        title: Ui.tr("Программы", "Programs")
        options: ["exclude", "include"]
        labels: [Ui.tr("Мимо VPN", "Skip VPN"), Ui.tr("Только через VPN", "Only via VPN")]
        selected: pl.s.processesMode
        onPicked: (v) => App.set("processesMode", v, true)
    }
    T {
        width: parent.width - 2 * pl.side
        x: pl.side
        bottomPadding: 8
        text: pl.s.processesMode === "include"
            ? Ui.tr("Через сервер пойдут только эти программы, остальное напрямую", "Only these programs go through the server, the rest go direct")
            : Ui.tr("Эти программы пойдут напрямую, минуя VPN", "These programs go direct, bypassing the VPN")
        type: "bodySmall"
        color: Theme.onSurfaceVariant
        wrapMode: Text.Wrap
    }
    Column {
        width: parent.width
        Repeater {
            model: pl.names
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
                    elide: Text.ElideMiddle
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
            placeholder: "chrome.exe"
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
