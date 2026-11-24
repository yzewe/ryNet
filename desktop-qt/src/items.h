#pragma once

#include "spring.h"

#include <QColor>
#include <QElapsedTimer>
#include <QQuickPaintedItem>
#include <QTimer>

class QVariantAnimation;

class MShape : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString shape READ shape WRITE setShape NOTIFY changed)
    Q_PROPERTY(QString morphTo READ morphTo WRITE setMorphTo NOTIFY changed)
    Q_PROPERTY(qreal progress READ progress WRITE setProgress NOTIFY changed)
    Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY changed)

public:
    explicit MShape(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;

    QString shape() const { return m_shape; }
    void setShape(const QString &s);
    QString morphTo() const { return m_morphTo; }
    void setMorphTo(const QString &s);
    qreal progress() const { return m_progress; }
    void setProgress(qreal v);
    QColor color() const { return m_color; }
    void setColor(const QColor &c);

    static bool exists(const QString &name);

signals:
    void changed();

private:
    QString m_shape = QStringLiteral("Circle");
    QString m_morphTo;
    qreal m_progress = 0;
    QColor m_color = Qt::black;
};

class LoadingIndicator : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY changed)
    Q_PROPERTY(QColor containerColor READ containerColor WRITE setContainerColor NOTIFY changed)
    Q_PROPERTY(bool running READ running WRITE setRunning NOTIFY changed)

public:
    explicit LoadingIndicator(QQuickItem *parent = nullptr);

    QColor color() const { return m_color; }
    void setColor(const QColor &c);
    QColor containerColor() const { return m_container; }
    void setContainerColor(const QColor &c);
    bool running() const { return m_running; }
    void setRunning(bool on);

signals:
    void changed();

protected:
    void itemChange(ItemChange change, const ItemChangeData &value) override;
    QSGNode *updatePaintNode(QSGNode *old, UpdatePaintNodeData *) override;

private:
    void sync();
    QColor m_color = Qt::black;
    QColor m_container = Qt::transparent;
    bool m_running = true;
    SpringClock *m_clock = nullptr;
    double m_t = 0;
};

class WavyProgress : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(qreal value READ value WRITE setValue NOTIFY changed)
    Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY changed)
    Q_PROPERTY(QColor trackColor READ trackColor WRITE setTrackColor NOTIFY changed)
    Q_PROPERTY(bool moving READ moving WRITE setMoving NOTIFY changed)
    Q_PROPERTY(qreal amplitude READ amplitude WRITE setAmplitude NOTIFY changed)

public:
    explicit WavyProgress(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;

    qreal value() const { return m_value; }
    void setValue(qreal v);
    QColor color() const { return m_color; }
    void setColor(const QColor &c);
    QColor trackColor() const { return m_track; }
    void setTrackColor(const QColor &c);
    bool moving() const { return m_moving; }
    void setMoving(bool on);
    qreal amplitude() const { return m_amplitude; }
    void setAmplitude(qreal a);

signals:
    void changed();

protected:
    void itemChange(ItemChange change, const ItemChangeData &value) override;

private:
    void sync();
    qreal m_value = 0;
    QColor m_color = Qt::black;
    QColor m_track = Qt::gray;
    bool m_moving = true;
    qreal m_amplitude = 3;
    double m_phase = 0;
    SpringState m_shown{0, 0, 0, 260, 1.0, 0.0005};
    bool m_hasValue = false;
    SpringClock *m_clock;
};

class ConnectOrb : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(int status READ status WRITE setStatus NOTIFY statusChanged)
    Q_PROPERTY(int shown READ shown NOTIFY shownChanged)
    Q_PROPERTY(QColor ink READ ink WRITE setInk NOTIFY colorsChanged)
    Q_PROPERTY(QColor container READ container WRITE setContainer NOTIFY colorsChanged)
    Q_PROPERTY(bool pressed READ pressed WRITE setPressed NOTIFY pressedChanged)
    Q_PROPERTY(qreal field READ field CONSTANT)

public:
    explicit ConnectOrb(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;

    int status() const { return m_status; }
    void setStatus(int s);
    int shown() const { return m_shown; }
    QColor ink() const { return m_ink; }
    void setInk(const QColor &c);
    QColor container() const { return m_container; }
    void setContainer(const QColor &c);
    bool pressed() const { return m_pressed; }
    void setPressed(bool on);
    qreal field() const;

signals:
    void statusChanged();
    void shownChanged();
    void colorsChanged();
    void pressedChanged();

protected:
    void componentComplete() override;

private:
    struct Tween {
        double from = 0, to = 0, value = 0, elapsed = 0, duration = 0;
        int easing = 0;
        bool running = false;
    };
    void show(int s);
    void finishOn();
    bool tick(double dt);
    void wake();
    void startTween(Tween &t, double from, double to, double ms, int easing);

    int m_status = 0;
    int m_shown = 0;
    int m_generation = 0;
    QColor m_ink = Qt::black;
    QColor m_container = Qt::lightGray;
    bool m_pressed = false;
    qint64 m_since = 0;

    SpringState m_amp{0, 0, 0, 160, 1.0, 0.001};
    SpringState m_bounce{1, 0, 1, 400, 0.45, 0.0005};
    SpringState m_press{1, 0, 1, 800, 0.6, 0.0005};
    SpringState m_thick{0, 0, 0, 800, 1.0, 0.001};
    SpringState m_morph{1, 0, 1, 380, 0.8, 0.001};
    bool m_ampSpring = false;
    Tween m_ampTween, m_burst, m_shake;
    QString m_from, m_to;
    double m_clockT = 0;
    SpringClock *m_clock;
    bool m_complete = false;
};

class LogoMark : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY changed)
    Q_PROPERTY(bool animated READ animated WRITE setAnimated NOTIFY changed)

public:
    explicit LogoMark(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;
    QColor color() const { return m_color; }
    void setColor(const QColor &c);
    bool animated() const { return m_animated; }
    void setAnimated(bool on);

    static void drawMark(QPainter *p, const QPointF &center, double size, const QColor &color, double drawn = 1.0);

signals:
    void changed();

protected:
    void componentComplete() override;

private:
    QColor m_color = Qt::black;
    bool m_animated = true;
    double m_drawn = 1;
    double m_elapsed = 0;
    SpringClock *m_clock;
};

class QrCode : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY changed)
    Q_PROPERTY(bool stream READ stream WRITE setStream NOTIFY changed)
    Q_PROPERTY(QColor ink READ ink WRITE setInk NOTIFY changed)
    Q_PROPERTY(QColor background READ background WRITE setBackground NOTIFY changed)

public:
    explicit QrCode(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;
    QString text() const { return m_text; }
    void setText(const QString &t);
    bool stream() const { return m_stream; }
    void setStream(bool on);
    QColor ink() const { return m_ink; }
    void setInk(const QColor &c);
    QColor background() const { return m_bg; }
    void setBackground(const QColor &c);

signals:
    void changed();

private:
    void encode();
    QString m_text;
    bool m_stream = false;
    QColor m_ink = Qt::black, m_bg = Qt::white;
    int m_n = 0;
    QByteArray m_bits;
    double m_appear = 1;
    double m_elapsed = 0;
    SpringClock *m_clock;
};

class ChainWave : public QQuickPaintedItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool running READ running WRITE setRunning NOTIFY changed)
    Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY changed)

public:
    explicit ChainWave(QQuickItem *parent = nullptr);
    void paint(QPainter *p) override;
    bool running() const { return m_running; }
    void setRunning(bool on);
    QColor color() const { return m_color; }
    void setColor(const QColor &c);

signals:
    void changed();

private:
    void sync();
    bool m_running = false;
    QColor m_color = Qt::black;
    double m_phase = 0;
    SpringClock *m_clock;
};
