import QtQuick

Item {
    id: shell

    readonly property int widthClass: width < 600 ? 0 : width < 840 ? 1 : 2
    readonly property bool onboarding: (App.subs.length === 0 && App.nav !== "settings" && App.nav !== "rules") || App.adding
    readonly property var keys: ["home", "servers", "subs", "rules", "log", "settings"]
    readonly property var files: ["HomePage.qml", "ServersPage.qml", "SubsPage.qml", "RulesPage.qml", "LogPage.qml", "SettingsPage.qml"]
    property int lastIndex: Math.max(0, keys.indexOf(App.nav))

    signal pageShown()

    onWidthClassChanged: Ui.widthClass = widthClass
    readonly property bool split: width >= 1000
    onSplitChanged: Ui.split = split
    Component.onCompleted: {
        Ui.widthClass = widthClass
        Ui.split = split
        takeLink()
    }

    function takeLink() {
        const link = App.takeIncoming()
        if (link.kind === undefined)
            return
        if (link.kind === "plugin") {
            pluginFile.path = link.path
            if (pluginFile.item)
                pluginFile.item.installFrom(link.path)
            else
                pluginFile.active = true
            return
        }
        if (link.kind === "set") {
            setLink.text = link.text
            if (setLink.item)
                setLink.item.askWith(link.text)
            else
                setLink.active = true
            return
        }
        if (link.kind === "servers") {
            linkSub.link = link
            linkSub.title = link.count === 1 ? Ui.tr("Добавить сервер?", "Add server?") : Ui.tr("Добавить серверы?", "Add servers?")
            linkSub.text = link.names
            linkSub.confirm = Ui.tr("Добавить", "Add")
            linkSub.show()
            return
        }
        linkSub.link = link
        linkSub.title = link.have ? Ui.tr("Подписка уже добавлена", "Subscription already added") : Ui.tr("Добавить подписку?", "Add subscription?")
        linkSub.text = Ui.joined([link.have ? link.title : link.name, link.host], " · ")
        linkSub.confirm = link.have ? Ui.tr("Обновить", "Update") : Ui.tr("Добавить", "Add")
        linkSub.show()
    }
    Connections {
        target: App
        function onIncomingChanged() { shell.takeLink() }
    }
    ConfirmDialog {
        id: linkSub
        property var link: ({})
        icon: "link"
        danger: false
        onAccepted: App.addLink(link)
    }
    Loader {
        id: setLink
        property string text
        active: false
        sourceComponent: SetImportDialog {
            Component.onCompleted: askWith(setLink.text)
        }
    }
    Loader {
        id: pluginFile
        property string path
        active: false
        sourceComponent: PluginsDialog {
            Component.onCompleted: installFrom(pluginFile.path)
        }
    }
    onOnboardingChanged: {
        if (onboarding) {
            leavingIntro.stop()
            intro.held = false
            intro.opacity = 1
            intro.scale = 1
            return
        }
        if (intro.status === Loader.Ready) {
            intro.held = true
            leavingIntro.restart()
        }
        const host = hosts.itemAt(keys.indexOf(App.nav))
        if (host && host.status === Loader.Ready) {
            host.through = true
            host.opacity = 0
            host.play()
        }
    }

    Item {
        id: sections
        readonly property bool gapped: App.settings.sectionDivider !== "none"
        readonly property real radius: App.settings.sectionRadius !== undefined ? App.settings.sectionRadius : 16
        property real outer: 16
        property real inner: 4
        property real gap: 2
        Spring on outer { id: outerSpring; to: sections.radius; stiffness: M.spatialK; damping: 1 }
        Spring on inner { id: innerSpring; to: sections.gapped ? Math.min(4, sections.radius) : 0; stiffness: M.spatialK; damping: 1 }
        Spring on gap { id: gapSpring; to: sections.gapped ? 2 : 0; stiffness: M.spatialK; damping: 1 }
        Component.onCompleted: {
            outerSpring.snap(outerSpring.to)
            innerSpring.snap(innerSpring.to)
            gapSpring.snap(gapSpring.to)
        }
    }
    Binding { target: Ui; property: "outer"; value: Math.max(0, sections.outer) }
    Binding { target: Ui; property: "inner"; value: Math.max(0, sections.inner) }
    Binding { target: Ui; property: "gap"; value: Math.max(0, sections.gap) }

    Rectangle {
        anchors.fill: parent
        ColorTo on color { to: shell.widthClass === 0 ? Theme.surface : Theme.surfaceContainer; duration: M.effectsSlowMs; linear: true }
    }

    Loader {
        id: rail
        active: !shell.onboarding && shell.widthClass > 0
        visible: active
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        sourceComponent: Rail { expanded: shell.widthClass === 2 }
    }
    Loader {
        id: bar
        active: !shell.onboarding && shell.widthClass === 0
        visible: active
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        sourceComponent: NavBar {}
    }

    Item {
        id: pages
        visible: !shell.onboarding
        anchors.left: rail.visible ? rail.right : parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: bar.visible ? bar.top : parent.bottom

        Repeater {
            id: hosts
            model: shell.keys
            Loader {
                id: host
                required property string modelData
                required property int index
                readonly property bool current: App.nav === modelData
                property real dy: 0
                property real zoom: 1

                property int dir: 1
                property bool holding: false
                property bool through: true

                anchors.fill: parent
                active: false
                asynchronous: true
                visible: current || leaving.running || holding
                source: active ? shell.files[index] : ""
                transform: [
                    Translate { y: host.dy },
                    Scale {
                        origin.x: host.width / 2
                        origin.y: host.height / 2
                        xScale: host.zoom
                        yScale: host.zoom
                    }
                ]

                Spring on dy {
                    id: drift
                    to: 0
                    stiffness: M.spatialK
                    damping: M.spatialZ
                }
                Spring on zoom {
                    id: grow
                    to: 1
                    stiffness: M.spatialK
                    damping: M.spatialZ
                }

                function enter() {
                    leaving.stop()
                    if (holding) {
                        holding = false
                        shell.lastIndex = index
                        return
                    }
                    dir = index > shell.lastIndex ? 1 : -1
                    shell.lastIndex = index
                    through = true
                    opacity = 0
                    if (status === Loader.Ready)
                        play()
                    else
                        active = true
                }
                function play() {
                    drift.to = 0
                    grow.to = 1
                    appearing.restart()
                    shell.pageShown()
                }
                onLoaded: if (current) play()
                function leave() {
                    appearing.stop()
                    const next = hosts.itemAt(shell.keys.indexOf(App.nav))
                    if (next && next.status !== Loader.Ready) {
                        holding = true
                        return
                    }
                    go()
                }
                function go() {
                    holding = false
                    leaving.restart()
                }
                onCurrentChanged: current ? enter() : leave()
                Connections {
                    target: shell
                    enabled: host.holding
                    function onPageShown() {
                        if (!host.current)
                            host.go()
                    }
                }
                Component.onCompleted: {
                    if (current) {
                        through = false
                        opacity = 0
                        asynchronous = false
                        active = true
                    }
                }

                SequentialAnimation {
                    id: appearing
                    PauseAnimation { duration: host.through ? 90 : 0 }
                    ScriptAction {
                        script: {
                            drift.animateFrom(host.dir * 20)
                            grow.animateFrom(0.95)
                        }
                    }
                    NumberAnimation { target: host; property: "opacity"; to: 1; duration: 210; easing.type: Easing.BezierSpline; easing.bezierCurve: M.decelerate }
                }
                NumberAnimation {
                    id: leaving
                    target: host
                    property: "opacity"
                    to: 0
                    duration: 90
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: M.accelerate
                }
            }
        }
    }

    Loader {
        id: intro
        property bool held: false
        anchors.fill: parent
        active: shell.onboarding || held
        enabled: shell.onboarding
        source: "Onboarding.qml"
    }
    ParallelAnimation {
        id: leavingIntro
        NumberAnimation { target: intro; property: "opacity"; to: 0; duration: 200; easing.type: Easing.BezierSpline; easing.bezierCurve: M.accelerate }
        NumberAnimation { target: intro; property: "scale"; to: 1.06; duration: 200; easing.type: Easing.BezierSpline; easing.bezierCurve: M.accelerate }
        onFinished: {
            intro.held = false
            intro.opacity = 1
            intro.scale = 1
        }
    }

    Item {
        id: snack
        property string text
        readonly property bool shown: App.notice !== ""
        readonly property real inset: bar.visible ? bar.height : 0
        readonly property real room: pages.width - (Ui.compact ? 0 : 24)
        property real fade: 0
        property real rise: 0
        property real grow: 1

        width: Ui.compact ? room - 32 : Math.min(560, room - 32, Math.max(344, label.implicitWidth + ok.width + 40))
        height: Math.max(48, label.implicitHeight + 28)
        x: pages.x + (room - width) / 2
        y: shell.height - height - inset - (Ui.compact ? 16 : 40)
        opacity: fade
        visible: fade > 0.01
        z: 50
        transform: [
            Scale {
                origin.x: snack.width / 2
                origin.y: snack.height / 2
                xScale: snack.grow
                yScale: 1 - (1 - snack.grow) * 0.5
            },
            Translate { y: snack.rise }
        ]

        Binding {
            target: Ui
            property: "snackHeight"
            value: snack.shown ? snack.height : 0
        }
        Connections {
            target: App
            function onNoticeChanged() {
                if (App.notice === "") {
                    rising.to = 10
                    return
                }
                const fresh = snack.fade < 0.05
                const changed = !fresh && snack.text !== App.notice
                snack.text = App.notice
                rising.to = 0
                if (fresh) {
                    rising.animateFrom(40)
                    growing.animateFrom(0.86)
                } else if (changed) {
                    growing.animateFrom(0.96)
                }
            }
        }
        Spring on fade {
            to: snack.shown ? 1 : 0
            stiffness: snack.shown ? M.effectsK : M.effectsFastK
            damping: 1
        }
        Spring on grow {
            id: growing
            to: 1
            stiffness: M.fastK
            damping: 0.62
        }
        Spring on rise {
            id: rising
            to: 0
            stiffness: M.spatialK
            damping: 0.72
        }

        Shadow {
            anchors.fill: parent
            radius: M.xs
            level: 3
        }
        Rectangle {
            anchors.fill: parent
            radius: M.xs
            color: Theme.inverseSurface
        }
        T {
            id: label
            anchors.left: parent.left
            anchors.leftMargin: 16
            anchors.right: ok.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            text: snack.text
            type: "bodyMedium"
            color: Theme.inverseOnSurface
            opacity: Math.max(0, Math.min(1, (snack.grow - 0.9) * 10))
            wrapMode: Text.Wrap
            maximumLineCount: 3
        }
        Surface {
            id: ok
            anchors.right: parent.right
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            width: okText.implicitWidth + 24
            height: 40
            radius: 20
            interactive: true
            tint: Theme.inversePrimary
            opacity: Math.max(0, Math.min(1, (snack.grow - 0.93) * 14))
            Accessible.name: okText.text
            onClicked: App.notice = ""
            T {
                id: okText
                anchors.centerIn: parent
                text: "OK"
                type: "labelLarge"
                color: Theme.inversePrimary
            }
        }
    }
}
