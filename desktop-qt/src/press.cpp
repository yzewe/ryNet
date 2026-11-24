#include "press.h"

#include <QGuiApplication>
#include <QCursor>
#include <QKeyEvent>
#include <QStyleHints>

namespace {
constexpr int kMinPressMs = 90;
}

Press::Press(QQuickItem *parent)
    : StateLayer(parent)
{
    m_squeeze.value = m_squeeze.target = 1;
    connect(this, &StateLayer::focusRingChanged, this, &Press::fill);
}

void Press::setInteractive(bool on)
{
    if (on == m_interactive)
        return;
    m_interactive = on;
    emit interactiveChanged();
    sync();
}

void Press::setRightClicks(bool on)
{
    if (on == m_rightClicks)
        return;
    m_rightClicks = on;
    emit interactiveChanged();
    sync();
}

void Press::setSquish(bool on)
{
    if (on == m_squish)
        return;
    m_squish = on;
    emit squishChanged();
    hold();
}

void Press::setSquishScale(qreal s)
{
    if (s == m_squishScale)
        return;
    m_squishScale = s;
    emit squishChanged();
    hold();
}

bool Press::contains(const QPointF &p) const
{
    const qreal i = inset();
    return QRectF(i, i, width() - 2 * i, height() - 2 * i).contains(p);
}

void Press::sync()
{
    const bool on = live();
    setAcceptedMouseButtons(on ? (m_rightClicks ? Qt::LeftButton | Qt::RightButton : Qt::LeftButton) : Qt::NoButton);
    setAcceptHoverEvents(on);
    if (on)
        setCursor(QCursor(Qt::PointingHandCursor));
    else
        unsetCursor();
    if (!on) {
        setContainsMouse(false);
        if (m_mousePressed) {
            m_longTimer.stop();
            setMousePressed(false);
        }
    }
    setHovered(on && m_containsMouse);
}

void Press::setContainsMouse(bool on)
{
    if (on == m_containsMouse)
        return;
    m_containsMouse = on;
    emit containsMouseChanged();
    setHovered(live() && on);
}

void Press::setMousePressed(bool on)
{
    if (on == m_mousePressed)
        return;
    m_mousePressed = on;
    emit mousePressedChanged();
    hold();
}

void Press::hold()
{
    const bool d = down();
    if (d != m_wasDown) {
        m_wasDown = d;
        setPressed(d);
        emit downChanged();
    }
    const double to = m_squish && d ? m_squishScale : 1;
    m_squeeze.stiffness = d ? 1800 : 650;
    m_squeeze.damping = d ? 1 : 0.42;
    if (to != m_squeeze.target) {
        m_squeeze.target = to;
        if (complete())
            wake();
        else
            m_squeeze.value = to;
    }
}

bool Press::advance(double dt)
{
    if (m_squeeze.value == m_squeeze.target && m_squeeze.velocity == 0)
        return true;
    const bool rest = m_squeeze.step(dt);
    emit squeezeChanged();
    return rest;
}

void Press::mousePressEvent(QMouseEvent *e)
{
    if (!live()) {
        e->ignore();
        return;
    }
    setContainsMouse(contains(e->position()));
    m_at = e->position() - QPointF(inset(), inset());
    setOrigin(m_at);
    m_long = false;
    m_hold.start(kMinPressMs, this);
    m_longTimer.start(QGuiApplication::styleHints()->mousePressAndHoldInterval(), this);
    setMousePressed(true);
    e->accept();
}

void Press::mouseMoveEvent(QMouseEvent *e)
{
    m_at = e->position() - QPointF(inset(), inset());
    setContainsMouse(contains(e->position()));
    if (m_long && m_mousePressed)
        emit holdMoved(m_at.x(), m_at.y());
}

void Press::mouseReleaseEvent(QMouseEvent *e)
{
    if (!m_mousePressed) {
        e->ignore();
        return;
    }
    m_longTimer.stop();
    setMousePressed(false);
    const QPointF p = e->position();
    const bool inside = contains(p);
    setContainsMouse(inside && acceptHoverEvents());
    if (m_long) {
        const QPointF at = p - QPointF(inset(), inset());
        emit holdReleased(at.x(), at.y());
    }
    if (inside && !m_long) {
        const QPointF at = p - QPointF(inset(), inset());
        if (e->button() == Qt::RightButton)
            emit rightClicked(at.x(), at.y());
        else
            emit clicked(at.x(), at.y());
    }
}

void Press::mouseUngrabEvent()
{
    m_longTimer.stop();
    setMousePressed(false);
}

void Press::hoverEnterEvent(QHoverEvent *) { setContainsMouse(true); }

void Press::hoverLeaveEvent(QHoverEvent *) { setContainsMouse(false); }

void Press::timerEvent(QTimerEvent *e)
{
    if (e->timerId() == m_hold.timerId()) {
        m_hold.stop();
        hold();
    } else if (e->timerId() == m_longTimer.timerId()) {
        m_longTimer.stop();
        if (m_mousePressed) {
            m_long = true;
            emit longPressed(m_at.x(), m_at.y());
        }
    } else {
        StateLayer::timerEvent(e);
    }
}

bool Press::eventFilter(QObject *o, QEvent *e)
{
    if (o == m_parent) {
        if (e->type() == QEvent::KeyPress && m_interactive) {
            auto *k = static_cast<QKeyEvent *>(e);
            if (k->key() == Qt::Key_Return || k->key() == Qt::Key_Enter || k->key() == Qt::Key_Space) {
                const QPointF mid(m_parent->width() / 2, m_parent->height() / 2);
                setOrigin(mid);
                m_hold.start(kMinPressMs, this);
                hold();
                emit clicked(mid.x(), mid.y());
                k->accept();
                return true;
            }
        }
    }
    return StateLayer::eventFilter(o, e);
}

void Press::itemChange(ItemChange change, const ItemChangeData &data)
{
    StateLayer::itemChange(change, data);
    if (change == ItemParentHasChanged) {
        if (m_parent) {
            m_parent->removeEventFilter(this);
            disconnect(m_parent, nullptr, this, nullptr);
        }
        m_parent = data.item;
        if (m_parent) {
            m_parent->installEventFilter(this);
            connect(m_parent, &QQuickItem::widthChanged, this, &Press::fill);
            connect(m_parent, &QQuickItem::heightChanged, this, &Press::fill);
            connect(m_parent, &QQuickItem::activeFocusChanged, this, [this] { setFocused(m_parent->hasActiveFocus()); });
            fill();
        }
    } else if (change == ItemEnabledHasChanged) {
        sync();
    }
}

void Press::fill()
{
    if (!m_parent)
        return;
    const qreal i = inset();
    setPosition(QPointF(-i, -i));
    setSize(QSizeF(m_parent->width() + 2 * i, m_parent->height() + 2 * i));
}
