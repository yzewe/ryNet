import QtQuick

Item {
    id: m

    property bool open: false
    property real menuX: 0
    property real menuY: 0
    property bool upward: false
    property real minWidth: 168
    default property alias items: column.data
    property real pop: 0
    property real reach: 1
    property real anchorX: 0
    property Item returnTo: null
    property real wanted: 0
    property var spans: [{ top: 0, bottom: -1 }]
    readonly property real pad: spans.length > 1 ? 4 : 8
    property Item held: null
    property bool grouped: false
    property Item hot: null

    function popup(item, dx, dy) {
        const p = item.mapToItem(Ui.overlay, dx === undefined ? 0 : dx, dy === undefined ? item.height + 4 : dy)
        place(p.x, p.y, item.mapToItem(Ui.overlay, 0, 0).y)
    }
    function popupEnd(item) {
        const p = item.mapToItem(Ui.overlay, item.width, item.height + 4)
        place(p.x, p.y, item.mapToItem(Ui.overlay, 0, 0).y)
        menuX = Math.max(8, Math.min(p.x - card.width, Ui.overlay.width - card.width - 8))
        anchorX = Math.max(0, Math.min(card.width, p.x - menuX))
    }
    function place(x, y, top) {
        const fresh = !open && pop < 0.05
        open = true
        wanted = Ui.menuItems(column, m)
        column.forceLayout()
        layoutSpans()
        if (grouped) {
            const shown = []
            for (const c of column.children)
                if (c.visible && c.first !== undefined)
                    shown.push(c)
            for (let i = 0; i < shown.length; ++i) {
                shown[i].first = i === 0
                shown[i].last = i === shown.length - 1
            }
        }
        column.forceLayout()
        const w = card.width, h = column.implicitHeight + (grouped ? 0 : 2 * pad)
        menuX = Math.max(8, Math.min(x, Ui.overlay.width - w - 8))
        upward = y + h > Ui.overlay.height - 8 && (top === undefined ? y : top) - h > 8
        menuY = Math.max(8, upward ? (top === undefined ? y : top) - h - 4 : Math.min(y, Ui.overlay.height - h - 8))
        anchorX = Math.max(0, Math.min(w, x - menuX))
        if (fresh)
            growing.animateFrom(0)
        sweep()
    }
    function close() {
        open = false
        held = null
        drag(null)
    }
    function drag(p) {
        let at = null
        if (p && open) {
            const walk = (item) => {
                for (const c of item.children) {
                    if (!c.visible || at)
                        continue
                    if (c.hot !== undefined) {
                        const q = c.mapFromItem(Ui.overlay, p.x, p.y)
                        if (c.enabled && q.x >= 0 && q.y >= 0 && q.x < c.width && q.y < c.height)
                            at = c
                    } else {
                        walk(c)
                    }
                }
            }
            walk(column)
        }
        if (at === hot)
            return
        if (hot)
            hot.hot = false
        hot = at
        if (hot)
            hot.hot = true
    }
    function release() {
        const it = hot
        drag(null)
        if (it)
            it.clicked(it.width / 2, it.height / 2)
    }

    function layoutSpans() {
        const kids = column.children
        let gaps = 0
        for (let i = 0; i < kids.length; ++i) {
            const c = kids[i]
            if (c.menuGap === true)
                c.shown = false
        }
        let before = false, pending = null
        for (let i = 0; i < kids.length; ++i) {
            const c = kids[i]
            if (c.menuGap === true) {
                if (before && pending === null)
                    pending = c
            } else if (c.visible) {
                if (pending !== null) {
                    pending.shown = true
                    ++gaps
                    pending = null
                }
                before = true
            }
        }
        const p = gaps > 0 ? 4 : 8
        column.forceLayout()
        const out = []
        let top = 0
        for (let i = 0; i < kids.length; ++i) {
            const c = kids[i]
            if (c.menuGap === true && c.shown) {
                const mid = p + c.y + c.height / 2
                out.push({ top: top, bottom: mid - 1 })
                top = mid + 1
            }
        }
        out.push({ top: top, bottom: -1 })
        spans = out
    }
    function itemAt(x, y) {
        const p = column.mapFromItem(Ui.overlay, x, y)
        for (let i = 0; i < column.children.length; ++i) {
            const c = column.children[i]
            if (c.visible && c.enabled && c.menu !== undefined && c.menuGap !== true
                    && p.x >= c.x && p.x < c.x + c.width && p.y >= c.y && p.y < c.y + c.height)
                return c
        }
        return null
    }
    function hoverAt(x, y) {
        if (open)
            held = itemAt(x, y)
    }
    function pickAt(x, y) {
        const it = open ? itemAt(x, y) : null
        held = null
        if (it)
            it.clicked(it.width / 2, it.height / 2)
    }

    function step(by) {
        if (by === Infinity || by === -Infinity)
            Ui.moveFocus(column, null, by > 0 ? -1 : 1)
        else
            Ui.moveFocus(column, m.Window.window.activeFocusItem, by)
    }

    onOpenChanged: {
        if (open) {
            returnTo = Ui.takeFocus(card, returnTo)
            return
        }
        Ui.giveFocusBack(m, returnTo)
        returnTo = null
    }
    Connections {
        target: m.open ? m.Window.window : null
        function onActiveFocusItemChanged() {
            if (m.open && !Ui.within(m.Window.window.activeFocusItem, card))
                m.close()
        }
    }
    Connections {
        target: m.open ? Ui.overlay : null
        function onWidthChanged() { m.close() }
        function onHeightChanged() { m.close() }
    }

    function extent() {
        return card.height * (0.3 + 0.7 * reach)
    }
    function sweep() {
        Ui.menuSweep(column, upward ? card.height - extent() : extent(), upward)
    }
    onReachChanged: sweep()

    parent: Ui.overlay
    anchors.fill: parent
    z: 90
    visible: open || pop > 0.01

    Spring on pop {
        to: m.open ? 1 : 0
        stiffness: M.effectsFastK
        damping: 1
    }
    Spring on reach {
        id: growing
        to: 1
        stiffness: M.fastK
        damping: 0.75
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
        onPressed: m.close()
        onWheel: (w) => {
            m.close()
            w.accepted = false
        }
    }

    FocusScope {
        id: card
        x: m.menuX
        y: m.menuY
        width: Math.max(m.minWidth, Math.min(280, m.wanted))
        height: column.implicitHeight + (m.grouped ? 0 : 2 * m.pad)
        opacity: m.pop
        transform: Scale {
            origin.x: m.anchorX
            origin.y: m.upward ? card.height : 0
            xScale: m.open ? 1 : 0.95 + 0.05 * m.pop
            yScale: m.open ? 1 : 0.9 + 0.1 * m.pop
        }

        Item {
            id: sheet
            width: card.width * (0.86 + 0.14 * m.reach)
            height: m.extent()
            x: m.anchorX * (card.width - width) / Math.max(1, card.width)
            y: m.upward ? card.height - height : 0

            Repeater {
                model: m.grouped ? [] : m.spans
                Item {
                    id: group
                    required property var modelData
                    required property int index
                    readonly property real from: Math.max(modelData.top, sheet.y)
                    readonly property real till: Math.min(modelData.bottom < 0 ? card.height : modelData.bottom, sheet.y + sheet.height)
                    readonly property real outerTop: index === 0 ? M.lg : M.xs
                    readonly property real outerBottom: index === m.spans.length - 1 ? M.lg : M.xs
                    y: from - sheet.y
                    width: sheet.width
                    height: Math.max(0, till - from)
                    visible: height > 0.5
                    Shadow {
                        anchors.fill: parent
                        radius: Math.min(group.outerTop, group.outerBottom)
                        level: 2
                    }
                    Rectangle {
                        anchors.fill: parent
                        topLeftRadius: Math.min(group.outerTop, group.height / 2)
                        topRightRadius: topLeftRadius
                        bottomLeftRadius: Math.min(group.outerBottom, group.height / 2)
                        bottomRightRadius: bottomLeftRadius
                        color: Theme.surfaceContainer
                    }
                }
            }
        }
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
        }
        Keys.onPressed: (e) => {
            switch (e.key) {
            case Qt.Key_Escape: m.close(); break
            case Qt.Key_Down: m.step(1); break
            case Qt.Key_Up: m.step(-1); break
            case Qt.Key_Home: m.step(-Infinity); break
            case Qt.Key_End: m.step(Infinity); break
            case Qt.Key_Tab: m.step(1); break
            case Qt.Key_Backtab: m.step(-1); break
            }
            e.accepted = true
        }

        Column {
            id: column
            y: m.grouped ? 0 : m.pad
            spacing: m.grouped ? 2 : 0
            width: card.width
        }
    }
}
