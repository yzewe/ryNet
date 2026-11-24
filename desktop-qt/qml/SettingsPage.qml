import QtQuick

Item {
    id: page

    readonly property bool wide: !Ui.compact && width >= 760
    readonly property var sections: [
        { key: "Network", title: Ui.tr("Сеть", "Network"), icon: "conversion_path", shape: "Circle", summary: Ui.tr("Маршрутизация, DNS, обход блокировок", "Routing, DNS, anti-censorship") },
        { key: "Servers", title: Ui.tr("Серверы", "Servers"), icon: "grid_view", shape: "Circle", summary: Ui.tr("Вид списка и сортировка", "List view and sorting") },
        { key: "Ping", title: Ui.tr("Пинг", "Ping"), icon: "bolt", shape: "Circle", summary: Ui.tr("Способ проверки, слежение за сервером", "Test method, server monitoring") },
        { key: "Auto", title: Ui.tr("Автовыбор", "Auto select"), icon: "swap_horiz", shape: "Circle", summary: Ui.tr("Лучший сервер, белые списки", "Best server, whitelists") },
        { key: "Subscription", title: Ui.tr("Подписка", "Subscription"), icon: "link", shape: "Circle", summary: Ui.tr("Обновление, маскировка", "Updates, disguise") },
        { key: "App", title: Ui.tr("Приложение", "App"), icon: "palette", shape: "Circle", summary: Ui.tr("Оформление, запуск, резервная копия", "Look, startup, backup") }
    ]
    readonly property string open: App.settingsPage
    readonly property string shownKey: wide ? (open !== "" ? open : "Network") : open
    readonly property var shownSection: sections[indexOf(shownKey)]
    property string lastKey: ""

    function indexOf(key) {
        return Ui.indexOfKey(sections, "key", key)
    }

    onShownKeyChanged: {
        const from = lastKey
        lastKey = shownKey
        if (wide && from !== "" && shownKey !== "")
            body.show(shownKey, indexOf(shownKey) > indexOf(from) ? 1 : -1, false)
        else
            body.show(shownKey, shownKey !== "" ? 1 : -1, true)
    }
    Component.onCompleted: {
        lastKey = shownKey
        body.place(shownKey)
    }

    component Slot: Item {
        id: slot
        property string key
        property real dx: 0
        property real dy: 0
        property real fade: 0
        property int cascadeDir: 0

        anchors.fill: parent
        opacity: fade
        visible: fade > 0.005
        transform: Translate { x: slot.dx; y: slot.dy }

        Spring on dx { id: slideX; to: 0; stiffness: M.spatialK; damping: 0.86 }
        Spring on dy { id: slideY; to: 0; stiffness: M.spatialK; damping: 0.86 }
        NumberAnimation {
            id: fadeIn
            target: slot
            property: "fade"
            to: 1
            duration: 260
            easing.type: Easing.BezierSpline
            easing.bezierCurve: M.decelerate
        }
        NumberAnimation {
            id: fadeOut
            target: slot
            property: "fade"
            to: 0
            duration: 130
            easing.type: Easing.BezierSpline
            easing.bezierCurve: M.accelerate
            onFinished: slot.key = ""
        }

        function load(next, dir, cascade) {
            cascadeDir = cascade ? dir : 0
            key = next
            scroll.contentY = 0
        }
        function place(next) {
            fadeOut.stop()
            load(next, 1, false)
            fade = next !== "" ? 1 : 0
            slideX.snap(0)
            slideY.snap(0)
        }
        function enter(next, dir, sideways) {
            fadeOut.stop()
            fade = 0.01
            load(next, dir, !sideways)
            fadeIn.restart()
            slideX.snap(sideways ? dir * 56 : 0)
            slideY.snap(0)
            slideX.to = 0
        }
        function leave(dir, sideways) {
            fadeIn.stop()
            if (sideways)
                slideX.to = -dir * 40
            else
                slideY.to = -dir * 16
            fadeOut.restart()
        }

        Scroll {
            id: scroll
            anchors.fill: parent
            anchors.rightMargin: -14
            bottomPadding: 24
            Loader {
                id: content
                width: Math.min(760, scroll.width - 14)
                source: slot.key !== "" ? "Settings" + slot.key + ".qml" : ""
                onLoaded: {
                    if (slot.cascadeDir !== 0)
                        Ui.cascadeIn(item, slot.cascadeDir)
                    slot.cascadeDir = 0
                }
            }
        }
    }

    Panes {
        Pane {
            anchors.fill: parent
            title: page.wide || page.open === "" ? Ui.tr("Настройки", "Settings") : (page.shownSection ? page.shownSection.title : "")
            actions: [
                Item {
                    id: back
                    readonly property bool on: !page.wide && page.open !== ""
                    property real k: 0
                    visible: k > 0.01
                    width: backBtn.width
                    height: backBtn.height
                    Spring on k {
                        to: back.on ? 1 : 0
                        stiffness: back.on ? M.bouncyK : M.effectsK
                        damping: back.on ? M.bouncyZ : 1
                    }
                    IconBtn {
                        id: backBtn
                        enabled: back.on
                        opacity: Math.max(0, Math.min(1, back.k * 1.5))
                        scale: 0.5 + 0.5 * back.k
                        transform: Translate { x: 10 * (1 - back.k) }
                        icon: "arrow_back"
                        tip: Ui.tr("Назад", "Back")
                        onClicked: App.settingsPage = ""
                    }
                }
            ]

            Item {
                id: frame
                width: Math.min(1080, parent.width)
                height: parent.height

                Item {
                    id: cats
                    readonly property Item sel: catPicker.selected
                    readonly property Item hov: catPicker.hovered
                    Picker {
                        id: catPicker
                        container: catColumn
                        selectedRole: "on"
                    }

                    visible: page.wide
                    width: 232
                    height: catColumn.height

                    NavIndicator {
                        anchors.fill: parent
                        linger: 90
                        color: Qt.rgba(Theme.onSurface.r, Theme.onSurface.g, Theme.onSurface.b, cats.hov && cats.hov.keyboardFocus ? 0.10 : 0.08)
                        target: cats.hov ? ({ key: cats.hov.key, x: cats.hov.x, y: cats.hov.y, w: cats.hov.width, h: cats.hov.height }) : ({ key: "" })
                    }
                    NavIndicator {
                        anchors.fill: parent
                        target: cats.sel ? ({ key: cats.sel.key, x: cats.sel.x, y: cats.sel.y, w: cats.sel.width, h: cats.sel.height }) : ({ key: "" })
                        down: cats.sel ? cats.sel.down : false
                        tint: !cats.sel ? 0 : cats.sel.keyboardFocus ? 0.10 : cats.sel.hovered ? 0.08 : 0
                    }

                    Column {
                        id: catColumn
                        width: parent.width
                        spacing: 4
                        Repeater {
                            id: catRep
                            model: page.sections

                            Item {
                                id: cat
                                required property var modelData
                                readonly property string key: modelData.key
                                readonly property bool on: key === page.shownKey
                                readonly property bool hovered: area.containsMouse
                                readonly property bool down: area.pressed
                                readonly property bool keyboardFocus: activeFocus

                                width: catColumn.width
                                height: 56
                                activeFocusOnTab: true
                                Accessible.role: Accessible.PageTab
                                Accessible.name: modelData.title
                                Accessible.selectable: true
                                Accessible.selected: on
                                Accessible.onPressAction: App.settingsPage = cat.key
                                onOnChanged: if (on) glyph.play()
                                Keys.onPressed: (e) => {
                                    if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Space) {
                                        App.settingsPage = cat.key
                                        e.accepted = true
                                    }
                                }

                                StateLayer {
                                    property real squeeze: 1
                                    anchors.fill: parent
                                    radii: Qt.vector4d(28, 28, 28, 28)
                                    tint: cat.on ? Theme.onSecondaryContainer : Theme.onSurface
                                    pressed: area.pressed
                                    origin: area.at
                                    scale: squeeze
                                    Spring on squeeze {
                                        to: area.pressed ? 0.94 : 1
                                        stiffness: area.pressed ? M.pressInK : M.pressOutK
                                        damping: area.pressed ? 1 : M.pressOutZ
                                    }
                                }
                                StateLayer {
                                    anchors.fill: parent
                                    anchors.margins: -inset
                                    radii: Qt.vector4d(28, 28, 28, 28)
                                    tint: "transparent"
                                    focusRing: true
                                    focused: cat.keyboardFocus
                                }
                                ShapeIcon {
                                    id: glyph
                                    x: 10
                                    anchors.verticalCenter: parent.verticalCenter
                                    size: 36
                                    icon: cat.modelData.icon
                                    shape: cat.modelData.shape
                                    container: cat.on ? Theme.primary : Theme.primaryContainer
                                    color: cat.on ? Theme.onPrimary : Theme.onPrimaryContainer
                                }
                                T {
                                    x: 60
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: cat.modelData.title
                                    type: cat.on ? "titleSmallE" : "titleSmall"
                                    font.pixelSize: 15
                                    ColorTo on color { to: cat.on ? Theme.onSecondaryContainer : Theme.onSurface; linear: true }
                                }
                                MouseArea {
                                    id: area
                                    property point at
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onPressed: (m) => at = Qt.point(m.x, m.y)
                                    onClicked: App.settingsPage = cat.key
                                }
                            }
                        }
                    }
                }

                Scroll {
                    id: list
                    readonly property bool shown: !page.wide && page.open === ""
                    property real dx: 0
                    anchors.fill: parent
                    anchors.rightMargin: -14
                    opacity: shown ? 1 : 0
                    visible: opacity > 0.005
                    transform: Translate { x: list.dx }
                    Behavior on opacity {
                        NumberAnimation {
                            duration: list.shown ? 260 : 130
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: list.shown ? M.decelerate : M.accelerate
                        }
                    }
                    Spring on dx {
                        to: list.shown ? 0 : -40
                        stiffness: M.spatialK
                        damping: 0.86
                    }

                    Column {
                        width: Math.min(720, parent.width - 14)
                        Group {
                            Repeater {
                                model: page.sections
                                SettingRow {
                                    required property var modelData
                                    iconStyle: "shape"
                                    icon: modelData.icon
                                    shape: modelData.shape
                                    title: modelData.title
                                    summary: modelData.summary
                                    onActivated: App.settingsPage = modelData.key
                                }
                            }
                        }
                        Header {
                            visible: Ui.compact
                            text: Ui.tr("Ещё", "More")
                        }
                        Group {
                            visible: Ui.compact
                            SettingRow {
                                iconStyle: "shape"
                                icon: "list"
                                title: Ui.tr("Правила", "Rules")
                                summary: Ui.tr("Какие сайты и приложения идут мимо VPN", "Which sites and apps skip the VPN")
                                onActivated: App.nav = "rules"
                            }
                            SettingRow {
                                iconStyle: "shape"
                                icon: "bug_report"
                                title: Ui.tr("Журнал", "Log")
                                summary: Ui.tr("Что происходит с подключением", "What happens with the connection")
                                onActivated: App.nav = "log"
                            }
                        }
                    }
                }

                Item {
                    id: body
                    property int current: 0
                    readonly property var slots: [first, second]

                    anchors.left: page.wide ? cats.right : parent.left
                    anchors.leftMargin: page.wide ? 24 : 0
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom

                    function place(key) {
                        slots[current].place(key)
                    }
                    function show(key, dir, sideways) {
                        const out = slots[current]
                        current = 1 - current
                        const into = slots[current]
                        if (out.key !== "")
                            out.leave(dir, sideways)
                        if (key !== "")
                            into.enter(key, dir, sideways)
                    }

                    Slot { id: first }
                    Slot { id: second }
                }
            }
        }
    }
}
