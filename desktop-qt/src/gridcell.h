#pragma once

#include "spring.h"

#include <QBasicTimer>
#include <QEasingCurve>
#include <QElapsedTimer>
#include <QPointer>
#include <QQuickItem>

class ServerGrid;

class GridCell : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(ServerGrid *grid READ grid WRITE setGrid NOTIFY gridChanged)
    Q_PROPERTY(QString key READ key WRITE setKey NOTIFY keyChanged)
    Q_PROPERTY(QString kind READ kind WRITE setKind NOTIFY keyChanged)
    Q_PROPERTY(bool current READ current WRITE setCurrent NOTIFY currentChanged)
    Q_PROPERTY(bool ready READ ready WRITE setReady NOTIFY readyChanged)
    Q_PROPERTY(bool live READ live NOTIFY liveChanged)

public:
    struct Place {
        double x = 0, y = 0, w = 0, h = 0;
        int e = 0;
        double d = 0;
        int gen = 0;
    };

    explicit GridCell(QQuickItem *parent = nullptr);
    ~GridCell() override;

    ServerGrid *grid() const { return m_grid; }
    void setGrid(ServerGrid *g);
    QString key() const { return m_key; }
    void setKey(const QString &k);
    QString kind() const { return m_kind; }
    void setKind(const QString &k);
    bool current() const { return m_current; }
    void setCurrent(bool on);
    bool ready() const { return m_ready; }
    void setReady(bool on);
    bool live() const { return m_live; }
    void setLive(bool on);

    bool placed() const { return m_placed; }
    double tx() const { return m_tx; }
    double ty() const { return m_ty; }
    int wave() const { return m_wave; }
    bool near() const { return m_near; }
    void setNear(bool on) { m_near = on; }
    bool moving() const;

    void place(const Place &p, bool snapping, int wave, double waveDir);

signals:
    void gridChanged();
    void keyChanged();
    void currentChanged();
    void readyChanged();
    void liveChanged();

protected:
    void componentComplete() override;
    void timerEvent(QTimerEvent *e) override;

private:
    void move();
    void come(int kind, double delay, double waveDir);
    void wake();
    bool tick(double dt);
    void apply();
    void restack();
    void attach();

    QPointer<ServerGrid> m_grid;
    QString m_key, m_kind;
    bool m_current = false;
    bool m_ready = false;
    bool m_live = false;
    bool m_near = false;
    bool m_placed = false;
    bool m_complete = false;
    bool m_attached = false;
    int m_wave = 0;
    int m_taken = -1;
    Place m_p, m_base;
    double m_tx = 0, m_ty = 0;
    int m_pendingKind = 0;
    double m_pendingDelay = 0, m_pendingDir = 1;
    qint64 m_pendingAt = -1;
    SpringState m_dx, m_dy, m_dw, m_dh, m_zoom, m_rise;
    double m_shade = 1;
    qint64 m_fadeStart = -1;
    double m_fadeFrom = 0;
    QElapsedTimer m_clock;
    QBasicTimer m_wait, m_mover;
    SpringClock *m_driver = nullptr;
};
