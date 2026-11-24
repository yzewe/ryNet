#pragma once

#include "spring.h"

#include <QColor>
#include <QEasingCurve>
#include <QElapsedTimer>
#include <QPointF>
#include <QQuickItem>
#include <QVector4D>

class StateLayer : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QColor tint READ tint WRITE setTint NOTIFY tintChanged)
    Q_PROPERTY(QVector4D radii READ radii WRITE setRadii NOTIFY radiiChanged)
    Q_PROPERTY(bool hovered READ hovered WRITE setHovered NOTIFY hoveredChanged)
    Q_PROPERTY(bool pressed READ pressed WRITE setPressed NOTIFY pressedChanged)
    Q_PROPERTY(bool focused READ focused WRITE setFocused NOTIFY focusedChanged)
    Q_PROPERTY(bool focusRing READ focusRing WRITE setFocusRing NOTIFY focusRingChanged)
    Q_PROPERTY(QPointF origin READ origin WRITE setOrigin NOTIFY originChanged)
    Q_PROPERTY(qreal stateAlpha READ stateAlpha WRITE setStateAlpha NOTIFY stateAlphaChanged)
    Q_PROPERTY(qreal inset READ inset NOTIFY focusRingChanged)
    Q_PROPERTY(bool fadeTint READ fadeTint WRITE setFadeTint NOTIFY fadeTintChanged)

public:
    explicit StateLayer(QQuickItem *parent = nullptr);

    QColor tint() const;
    void setTint(const QColor &c);
    QVector4D radii() const { return m_radii; }
    void setRadii(const QVector4D &r);
    bool hovered() const { return m_hovered; }
    void setHovered(bool on);
    bool pressed() const { return m_pressed; }
    void setPressed(bool on);
    bool focused() const { return m_focused; }
    void setFocused(bool on);
    bool focusRing() const { return m_focusRing; }
    void setFocusRing(bool on);
    QPointF origin() const;
    void setOrigin(const QPointF &p);
    qreal stateAlpha() const { return m_alpha; }
    void setStateAlpha(qreal a);
    qreal inset() const { return m_focusRing ? 6 : 0; }
    bool fadeTint() const { return m_fadeTint; }
    void setFadeTint(bool on);

signals:
    void tintChanged();
    void radiiChanged();
    void hoveredChanged();
    void pressedChanged();
    void focusedChanged();
    void focusRingChanged();
    void originChanged();
    void stateAlphaChanged();
    void fadeTintChanged();

protected:
    QSGNode *updatePaintNode(QSGNode *old, UpdatePaintNodeData *) override;
    void geometryChange(const QRectF &next, const QRectF &old) override;
    void componentComplete() override;
    void wake();
    virtual bool advance(double) { return true; }
    bool complete() const { return m_complete; }

private:
    struct Tween {
        double from = 0, to = 0;
        int ms = 0;
        qint64 start = -1;
        const QEasingCurve *curve = nullptr;
        bool running() const { return start >= 0; }
        void run(double now, double target, int duration, const QEasingCurve *c, qint64 at)
        {
            from = now;
            to = target;
            ms = duration;
            curve = c;
            start = at;
        }
        double at(qint64 at, double current);
    };

    QColor drawnTint() const;
    qreal shapeWidth() const { return width() - 2 * inset(); }
    qreal shapeHeight() const { return height() - 2 * inset(); }
    double reach() const;
    void aim();
    bool tick();
    bool shown() const;

    QColor m_tint;
    bool m_tintSet = false;
    bool m_fadeTint = false;
    QColor m_tintFrom;
    Tween m_tintTween;
    double m_tintMix = 1;
    QVector4D m_radii;
    bool m_hovered = false, m_pressed = false, m_focused = false, m_focusRing = false;
    QPointF m_origin;
    bool m_originSet = false;
    bool m_alphaFixed = false;
    bool m_complete = false;

    QPointF m_center;
    double m_alpha = 0;
    double m_ripple = 0, m_rippleAlpha = 0;
    SpringState m_ring;
    Tween m_alphaTween, m_rippleTween, m_rippleFade;
    qint64 m_pressedAt = 0;
    qint64 m_fadeAt = -1;
    QElapsedTimer m_clock;
    SpringClock *m_driver = nullptr;
    qint64 m_lastTick = -1;
};
