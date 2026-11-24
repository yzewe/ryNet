#include "gridcell.h"

#include "grid.h"

namespace {
const QEasingCurve &decelerate()
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.05, 0.7), QPointF(0.1, 1), QPointF(1, 1));
        return e;
    }();
    return c;
}

void rest(SpringState &s, double at)
{
    s.value = s.target = at;
    s.velocity = 0;
    s.span = 0;
}

bool still(const SpringState &s) { return s.value == s.target && s.velocity == 0; }
}

GridCell::GridCell(QQuickItem *parent)
    : QQuickItem(parent)
{
    m_clock.start();
    for (SpringState *s : {&m_dx, &m_dy}) {
        s->stiffness = 380;
        s->damping = 0.82;
    }
    for (SpringState *s : {&m_dw, &m_dh}) {
        s->stiffness = 380;
        s->damping = 0.85;
    }
    m_zoom.stiffness = 380;
    m_zoom.damping = 0.7;
    rest(m_zoom, 1);
    m_rise.stiffness = 420;
    m_rise.damping = 0.78;
    setVisible(false);
}

GridCell::~GridCell()
{
    if (m_grid && m_attached)
        m_grid->removeCell(this);
}

void GridCell::setGrid(ServerGrid *g)
{
    if (g == m_grid)
        return;
    if (m_grid && m_attached)
        m_grid->removeCell(this);
    m_attached = false;
    m_grid = g;
    emit gridChanged();
    attach();
}

void GridCell::setKey(const QString &k)
{
    if (k == m_key)
        return;
    if (m_grid && m_attached)
        m_grid->removeCell(this);
    m_attached = false;
    m_key = k;
    emit keyChanged();
    attach();
}

void GridCell::setKind(const QString &k)
{
    if (k == m_kind)
        return;
    m_kind = k;
    emit keyChanged();
}

void GridCell::setCurrent(bool on)
{
    if (on == m_current)
        return;
    m_current = on;
    emit currentChanged();
    restack();
}

void GridCell::setReady(bool on)
{
    if (on == m_ready)
        return;
    m_ready = on;
    emit readyChanged();
    if (!on) {
        m_mover.stop();
        m_wait.stop();
        for (SpringState *s : {&m_dx, &m_dy, &m_dw, &m_dh, &m_rise})
            rest(*s, 0);
        rest(m_zoom, 1);
        m_shade = 1;
        m_fadeStart = -1;
        apply();
        return;
    }
    if (m_pendingAt >= 0) {
        const qint64 waited = m_clock.elapsed() - m_pendingAt;
        m_pendingAt = -1;
        if (waited < 400)
            come(m_pendingKind, std::max(1.0, m_pendingDelay - double(waited)), m_pendingDir);
    }
}

void GridCell::setLive(bool on)
{
    if (on == m_live)
        return;
    m_live = on;
    emit liveChanged();
}

bool GridCell::moving() const { return !still(m_dx) || !still(m_dy); }

void GridCell::componentComplete()
{
    QQuickItem::componentComplete();
    m_complete = true;
    attach();
}

void GridCell::attach()
{
    if (!m_complete || !m_grid || m_key.isEmpty() || m_attached)
        return;
    m_attached = true;
    m_grid->addCell(this);
}

void GridCell::place(const Place &p, bool snapping, int wave, double waveDir)
{
    if (p.gen == m_taken)
        return;
    m_taken = p.gen;
    m_p = p;
    if (!m_placed || snapping) {
        m_mover.stop();
        for (SpringState *s : {&m_dx, &m_dy, &m_dw, &m_dh})
            rest(*s, 0);
        m_tx = p.x;
        m_ty = p.y;
        m_base = p;
        m_placed = true;
        setVisible(true);
        apply();
    } else if (p.d > 0 && !p.e && m_ready) {
        m_mover.start(int(p.d), this);
    } else {
        m_mover.stop();
        move();
    }
    if (p.e) {
        m_wave = wave;
        come(p.e, p.d, waveDir);
    }
}

void GridCell::move()
{
    if (m_ready) {
        m_dx.value += m_base.x - m_p.x;
        m_dy.value += m_base.y - m_p.y;
        m_dw.value += m_base.w - m_p.w;
        m_dh.value += m_base.h - m_p.h;
    }
    m_tx = m_p.x;
    m_ty = m_p.y;
    m_base = m_p;
    apply();
    wake();
}

void GridCell::come(int kind, double delay, double waveDir)
{
    if (!m_ready) {
        m_pendingKind = kind;
        m_pendingDelay = delay;
        m_pendingDir = waveDir;
        m_pendingAt = m_clock.elapsed();
        return;
    }
    m_fadeStart = -1;
    m_shade = 0;
    rest(m_rise, kind == 1 ? waveDir * 22 : 0);
    rest(m_zoom, kind == 1 ? 1 : 0.85);
    m_wait.start(std::max(1, int(delay)), this);
    apply();
}

void GridCell::timerEvent(QTimerEvent *e)
{
    if (e->timerId() == m_mover.timerId()) {
        m_mover.stop();
        move();
    } else if (e->timerId() == m_wait.timerId()) {
        m_wait.stop();
        m_fadeFrom = m_shade;
        m_fadeStart = m_clock.elapsed();
        m_rise.target = 0;
        m_zoom.target = 1;
        wake();
    } else {
        QQuickItem::timerEvent(e);
    }
}

void GridCell::wake()
{
    if (!m_driver)
        m_driver = new SpringClock([this](double dt) { return tick(dt); }, this);
    if (m_driver->state() != QAbstractAnimation::Running)
        m_driver->start();
}

bool GridCell::tick(double dt)
{
    bool rest = true;
    for (SpringState *s : {&m_dx, &m_dy, &m_dw, &m_dh, &m_zoom, &m_rise})
        if (!still(*s))
            rest = s->step(dt) && rest;
    if (m_fadeStart >= 0) {
        const double t = std::min(1.0, (m_clock.elapsed() - m_fadeStart) / 260.0);
        m_shade = m_fadeFrom + (1 - m_fadeFrom) * decelerate().valueForProgress(t);
        if (t >= 1)
            m_fadeStart = -1;
        else
            rest = false;
    }
    apply();
    return rest;
}

void GridCell::apply()
{
    setPosition(QPointF(m_base.x + m_dx.value, m_base.y + m_dy.value + m_rise.value));
    setSize(QSizeF(m_base.w + m_dw.value, m_base.h + m_dh.value));
    setScale(m_zoom.value);
    setOpacity(m_shade);
    restack();
}

void GridCell::restack()
{
    setZ(m_current ? 2 : m_kind == u"tile" && m_ready && moving() ? 1 : 0);
}
