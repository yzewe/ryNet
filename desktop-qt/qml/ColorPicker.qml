import QtQuick

Dialog {
    id: p

    property string value: "#0b6b57"
    property var save: (c) => {}
    property real hue: 0
    property real shownHue: 0
    property bool dragging: false
    property bool syncing: false
    readonly property bool ok: /^#[0-9a-fA-F]{6}$/.test(value)
    readonly property bool dark: Theme.dark

    function edit(initial) {
        value = initial
        setField(initial)
        hue = Theme.hueOf(initial)
        gliding.snap(hue)
        show()
    }
    function setField(v) {
        syncing = true
        field.text = v.toUpperCase()
        syncing = false
    }
    function pickHue(h) {
        hue = (h % 360 + 360) % 360
        value = Theme.hct(hue, 48, 50).toString()
        setField(value)
    }

    preferredWidth: 400
    title: Ui.tr("Цвет", "Color")

    Spring on shownHue {
        id: gliding
        to: p.hue
        stiffness: p.dragging ? M.effectsFastK : M.fastK
        damping: p.dragging ? 1 : 0.75
    }

    Column {
        width: parent.width
        spacing: 24

        Row {
            width: parent.width
            spacing: 16
            Swatch {
                y: 2
                interactive: false
                colors: p.ok ? Theme.swatch(p.value, p.dark) : []
                icon: "palette"
                Accessible.ignored: true
            }
            Field {
                id: field
                width: parent.width - 52 - 16
                label: "HEX"
                mono: true
                bg: Theme.surfaceContainerHigh
                error: p.ok ? "" : Ui.tr("Неверное значение", "Invalid value")
                onTextChanged: {
                    if (p.syncing)
                        return
                    const t = text.trim()
                    p.value = (t.startsWith("#") ? t : "#" + t).toLowerCase()
                    if (p.ok)
                        p.hue = Theme.hueOf(p.value)
                }
                onAccepted: if (p.ok) { p.save(p.value); p.close() }
            }
        }

        Item {
            id: strip
            width: parent.width
            height: 44
            activeFocusOnTab: true
            Accessible.role: Accessible.Slider
            Accessible.name: Ui.tr("Цвет", "Color")
            Keys.onLeftPressed: p.pickHue(p.hue - 5)
            Keys.onRightPressed: p.pickHue(p.hue + 5)

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width
                height: 24
                radius: 12
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0 / 12; color: Theme.hct(0, 48, 60) }
                    GradientStop { position: 1 / 12; color: Theme.hct(30, 48, 60) }
                    GradientStop { position: 2 / 12; color: Theme.hct(60, 48, 60) }
                    GradientStop { position: 3 / 12; color: Theme.hct(90, 48, 60) }
                    GradientStop { position: 4 / 12; color: Theme.hct(120, 48, 60) }
                    GradientStop { position: 5 / 12; color: Theme.hct(150, 48, 60) }
                    GradientStop { position: 6 / 12; color: Theme.hct(180, 48, 60) }
                    GradientStop { position: 7 / 12; color: Theme.hct(210, 48, 60) }
                    GradientStop { position: 8 / 12; color: Theme.hct(240, 48, 60) }
                    GradientStop { position: 9 / 12; color: Theme.hct(270, 48, 60) }
                    GradientStop { position: 10 / 12; color: Theme.hct(300, 48, 60) }
                    GradientStop { position: 11 / 12; color: Theme.hct(330, 48, 60) }
                    GradientStop { position: 12 / 12; color: Theme.hct(360, 48, 60) }
                }
            }
            Item {
                id: handle
                readonly property real at: 6 + (strip.width - 12) * p.shownHue / 360
                property real thin: 0
                x: at - width / 2
                width: 16
                height: parent.height
                Spring on thin {
                    to: drag.pressed ? 1 : 0
                    stiffness: drag.pressed ? M.pressInK : M.pressOutK
                    damping: drag.pressed ? 1 : M.pressOutZ
                }
                Rectangle {
                    anchors.centerIn: parent
                    width: 4 - 2 * handle.thin + 12
                    height: 24
                    color: Theme.surfaceContainerHigh
                }
                Rectangle {
                    anchors.centerIn: parent
                    width: 4 - 2 * handle.thin
                    height: parent.height
                    radius: width / 2
                    color: strip.activeFocus ? Theme.primary : Theme.onSurface
                }
            }
            MouseArea {
                id: drag
                anchors.fill: parent
                anchors.topMargin: -8
                anchors.bottomMargin: -8
                cursorShape: Qt.PointingHandCursor
                preventStealing: true
                function follow(x) { p.pickHue(360 * Math.max(0, Math.min(1, (x - 6) / (strip.width - 12)))) }
                onPressed: (m) => {
                    strip.forceActiveFocus()
                    follow(m.x)
                }
                onPositionChanged: (m) => {
                    if (pressed) {
                        p.dragging = true
                        follow(m.x)
                    }
                }
                onReleased: p.dragging = false
                onCanceled: p.dragging = false
            }
        }
    }

    buttons: [
        Btn {
            kind: "text"
            text: Ui.tr("Отмена", "Cancel")
            onClicked: p.close()
        },
        Btn {
            kind: "text"
            text: Ui.tr("Сохранить", "Save")
            enabled: p.ok
            onClicked: {
                p.save(p.value)
                p.close()
            }
        }
    ]
}
