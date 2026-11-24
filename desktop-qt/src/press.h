#pragma once

#include "statelayer.h"

#include <QBasicTimer>
#include <QPointer>

class Press : public StateLayer {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool interactive READ interactive WRITE setInteractive NOTIFY interactiveChanged)
    Q_PROPERTY(bool rightClicks READ rightClicks WRITE setRightClicks NOTIFY interactiveChanged)
    Q_PROPERTY(bool squish READ squish WRITE setSquish NOTIFY squishChanged)
    Q_PROPERTY(qreal squishScale READ squishScale WRITE setSquishScale NOTIFY squishChanged)
    Q_PROPERTY(bool containsMouse READ containsMouse NOTIFY containsMouseChanged)
    Q_PROPERTY(bool mousePressed READ mousePressed NOTIFY mousePressedChanged)
    Q_PROPERTY(bool down READ down NOTIFY downChanged)
    Q_PROPERTY(qreal squeeze READ squeeze NOTIFY squeezeChanged)

public:
    explicit Press(QQuickItem *parent = nullptr);

    bool interactive() const { return m_interactive; }
    void setInteractive(bool on);
    bool rightClicks() const { return m_rightClicks; }
    void setRightClicks(bool on);
    bool squish() const { return m_squish; }
    void setSquish(bool on);
    qreal squishScale() const { return m_squishScale; }
    void setSquishScale(qreal s);
    bool containsMouse() const { return m_containsMouse; }
    bool mousePressed() const { return m_mousePressed; }
    bool down() const { return m_mousePressed || m_hold.isActive(); }
    qreal squeeze() const { return m_squeeze.value; }

    bool contains(const QPointF &p) const override;

signals:
    void interactiveChanged();
    void squishChanged();
    void containsMouseChanged();
    void mousePressedChanged();
    void downChanged();
    void squeezeChanged();
    void clicked(qreal x, qreal y);
    void rightClicked(qreal x, qreal y);
    void longPressed(qreal x, qreal y);
    void holdMoved(qreal x, qreal y);
    void holdReleased(qreal x, qreal y);

protected:
    void mousePressEvent(QMouseEvent *e) override;
    void mouseMoveEvent(QMouseEvent *e) override;
    void mouseReleaseEvent(QMouseEvent *e) override;
    void mouseUngrabEvent() override;
    void hoverEnterEvent(QHoverEvent *e) override;
    void hoverLeaveEvent(QHoverEvent *e) override;
    void timerEvent(QTimerEvent *e) override;
    bool eventFilter(QObject *o, QEvent *e) override;
    void itemChange(ItemChange change, const ItemChangeData &data) override;
    bool advance(double dt) override;

private:
    bool live() const { return m_interactive && isEnabled(); }
    void fill();
    void sync();
    void setContainsMouse(bool on);
    void setMousePressed(bool on);
    void hold();

    QPointer<QQuickItem> m_parent;
    bool m_interactive = false;
    bool m_rightClicks = false;
    bool m_squish = false;
    qreal m_squishScale = 0.96;
    bool m_containsMouse = false;
    bool m_mousePressed = false;
    bool m_wasDown = false;
    bool m_long = false;
    QPointF m_at;
    QBasicTimer m_hold, m_longTimer;
    SpringState m_squeeze;
};
