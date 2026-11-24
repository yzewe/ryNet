import QtQuick

Item {
    id: ind

    property var target: ({ key: "" })
    property bool vertical: true
    property bool down: false
    property real tint: 0
    property color color: Theme.mix(Theme.secondaryContainer, Theme.onSecondaryContainer, tint)
    property int linger: 0

    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real squeeze: 1
    property real bloom: 1
    property bool forward: true
    property bool lit: false
    property string shown: ""

    readonly property real natural: vertical ? (target.h || 0) : (target.w || 0)
    readonly property real broad: vertical ? (target.w || 0) : (target.h || 0)
    readonly property real cap: broad > natural ? Math.min(natural * 0.45, (broad - natural) * 0.5) : natural * 0.6
    readonly property real ea: forward ? Math.max(a, b - natural - cap) : a
    readonly property real eb: forward ? b : Math.min(b, a + natural + cap)
    readonly property real stretch: cap > 0 ? Math.min(1, Math.max(0, eb - ea - natural) / cap) : 0
    readonly property real squash: natural > 0 ? Math.min(0.3, Math.max(0, natural - (eb - ea)) / natural) : 0

    readonly property real leadK: 900
    readonly property real leadZ: 0.6
    readonly property real trailK: 480
    readonly property real trailZ: 0.88

    opacity: lit ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: M.effectsMs; easing.type: Easing.OutCubic } }

    Spring on a { id: sa; stiffness: ind.trailK; damping: ind.trailZ }
    Spring on b { id: sb; stiffness: ind.leadK; damping: ind.leadZ }
    Spring on c { id: sc; stiffness: 500; damping: 0.8 }
    Spring on d { id: sd; stiffness: 500; damping: 0.8 }
    Spring on squeeze {
        to: ind.down ? 0.92 : 1
        stiffness: ind.down ? M.pressInK : M.pressOutK
        damping: ind.down ? 1 : M.pressOutZ
    }
    Spring on bloom { id: blooming; to: 1; stiffness: M.fastK; damping: 0.7 }

    onTargetChanged: place()
    Component.onCompleted: place()

    Timer {
        id: fading
        interval: Math.max(1, ind.linger)
        onTriggered: ind.lit = false
    }

    function place() {
        const t = target
        if (!t || !t.key) {
            if (linger > 0 && lit)
                fading.restart()
            else
                lit = false
            return
        }
        fading.stop()
        if (!lit && opacity < 0.3)
            shown = ""
        const appearing = !lit
        lit = true
        const na = vertical ? t.y : t.x
        const nb = vertical ? t.y + t.h : t.x + t.w
        const nc = vertical ? t.x : t.y
        const nd = vertical ? t.w : t.h
        if (t.key === shown || shown === "") {
            if (shown === "" || !(sa.running || sb.running)) {
                sa.snap(na)
                sb.snap(nb)
                sc.snap(nc)
                sd.snap(nd)
            } else {
                sa.to = na
                sb.to = nb
                sc.to = nc
                sd.to = nd
            }
            if (appearing && shown === "")
                blooming.animateFrom(0.9)
            shown = t.key
            return
        }
        shown = t.key
        forward = na > a
        const f = Math.min(0.2, 5 / Math.max(1, Math.abs(na - a)))
        const lz = Math.min(0.9, Math.max(leadZ, -Math.log(f) / Math.sqrt(Math.PI * Math.PI + Math.log(f) * Math.log(f))))
        sa.stiffness = forward ? trailK : leadK
        sa.damping = forward ? trailZ : lz
        sb.stiffness = forward ? leadK : trailK
        sb.damping = forward ? lz : trailZ
        sa.to = na
        sb.to = nb
        sc.to = nc
        sd.to = nd
    }

    Rectangle {
        readonly property real len: ind.eb - ind.ea
        readonly property real across: ind.d * (1 - (ind.broad > ind.natural ? 0 : 0.12) * ind.stretch + 0.4 * ind.squash)
        x: ind.vertical ? ind.c + (ind.d - across) / 2 : ind.ea
        y: ind.vertical ? ind.ea : ind.c + (ind.d - across) / 2
        width: ind.vertical ? across : len
        height: ind.vertical ? len : across
        radius: Math.min(width, height) / 2
        scale: ind.squeeze * ind.bloom
        ColorTo on color { to: ind.color; duration: 140 }
    }
}
