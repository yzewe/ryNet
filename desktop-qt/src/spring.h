#pragma once

#include <QAbstractAnimation>
#include <QColor>
#include <QObject>
#include <QQmlEngine>
#include <QQmlProperty>
#include <QQmlPropertyValueSource>
#include <functional>

struct SpringState {
    double value = 0;
    double velocity = 0;
    double target = 0;
    double stiffness = 380;
    double damping = 0.8;
    double threshold = 0.001;
    double span = 0;

    bool step(double dt);
    bool settled() const;
};

class SpringClock : public QAbstractAnimation {
public:
    explicit SpringClock(std::function<bool(double)> tick, QObject *parent = nullptr)
        : QAbstractAnimation(parent)
        , m_tick(std::move(tick))
    {
    }
    int duration() const override { return -1; }

protected:
    void updateCurrentTime(int ms) override;
    void updateState(State next, State) override
    {
        if (next == Running)
            m_last = -1;
    }

private:
    std::function<bool(double)> m_tick;
    int m_last = -1;
};

class Spring : public QObject, public QQmlPropertyValueSource {
    Q_OBJECT
    Q_INTERFACES(QQmlPropertyValueSource)
    QML_ELEMENT
    Q_PROPERTY(qreal to READ to WRITE setTo NOTIFY toChanged)
    Q_PROPERTY(qreal stiffness READ stiffness WRITE setStiffness NOTIFY paramsChanged)
    Q_PROPERTY(qreal damping READ damping WRITE setDamping NOTIFY paramsChanged)
    Q_PROPERTY(qreal threshold READ threshold WRITE setThreshold NOTIFY paramsChanged)
    Q_PROPERTY(bool enabled READ enabled WRITE setEnabled NOTIFY paramsChanged)
    Q_PROPERTY(bool running READ running NOTIFY runningChanged)
    Q_PROPERTY(qreal velocity READ velocity NOTIFY runningChanged)

public:
    explicit Spring(QObject *parent = nullptr);
    void setTarget(const QQmlProperty &p) override;

    qreal to() const { return m_state.target; }
    void setTo(qreal v);
    qreal stiffness() const { return m_state.stiffness; }
    void setStiffness(qreal v);
    qreal damping() const { return m_state.damping; }
    void setDamping(qreal v);
    qreal threshold() const { return m_state.threshold; }
    void setThreshold(qreal v);
    bool enabled() const { return m_enabled; }
    void setEnabled(bool on);
    bool running() const;
    qreal velocity() const { return m_state.velocity; }

    Q_INVOKABLE void snap(qreal v);
    Q_INVOKABLE void kick(qreal velocity);
    Q_INVOKABLE void animateFrom(qreal v);
    Q_INVOKABLE void shift(qreal d);

signals:
    void toChanged();
    void paramsChanged();
    void runningChanged();

private:
    void run();
    bool tick(double dt);
    void write();

    QQmlProperty m_property;
    SpringState m_state;
    SpringClock *m_clock = nullptr;
    bool m_enabled = true;
    bool m_hasTarget = false;
    bool m_initialized = false;
};

class ColorTo : public QObject, public QQmlPropertyValueSource {
    Q_OBJECT
    Q_INTERFACES(QQmlPropertyValueSource)
    QML_ELEMENT
    Q_PROPERTY(QColor to READ to WRITE setTo NOTIFY toChanged)
    Q_PROPERTY(bool animate READ animate WRITE setAnimate NOTIFY paramsChanged)
    Q_PROPERTY(int duration READ duration WRITE setDuration NOTIFY paramsChanged)
    Q_PROPERTY(bool linear READ linear WRITE setLinear NOTIFY paramsChanged)

public:
    explicit ColorTo(QObject *parent = nullptr);
    void setTarget(const QQmlProperty &p) override;

    QColor to() const { return m_to; }
    void setTo(const QColor &c);
    bool animate() const { return m_animate; }
    void setAnimate(bool on);
    int duration() const { return m_duration; }
    void setDuration(int ms);
    bool linear() const { return m_linear; }
    void setLinear(bool on);

signals:
    void toChanged();
    void paramsChanged();

private:
    bool tick(double dt);
    void write(const QColor &c);

    QQmlProperty m_property;
    QColor m_to, m_from, m_now;
    bool m_animate = true;
    bool m_linear = false;
    bool m_set = false;
    int m_duration = 180;
    double m_elapsed = 0;
    SpringClock *m_clock = nullptr;
};
