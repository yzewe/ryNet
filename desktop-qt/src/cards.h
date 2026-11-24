#pragma once

#include "spring.h"

#include <QAbstractListModel>
#include <QBasicTimer>
#include <QElapsedTimer>
#include <QQuickItem>
#include <QVariantMap>

class KeyedRows : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QVariantList items READ items WRITE setItems NOTIFY itemsChanged)
    Q_PROPERTY(QString keyField READ keyField WRITE setKeyField NOTIFY itemsChanged)
    Q_PROPERTY(bool ordered READ ordered WRITE setOrdered NOTIFY itemsChanged)
    Q_PROPERTY(int linger READ linger WRITE setLinger NOTIFY itemsChanged)
    Q_PROPERTY(int epoch READ epoch WRITE setEpoch NOTIFY itemsChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)

public:
    enum Role { KeyRole = Qt::UserRole + 1, ItemRole, FreshRole, LeavingRole };

    explicit KeyedRows(QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    QVariantList items() const { return m_items; }
    void setItems(const QVariantList &items);
    QString keyField() const { return m_keyField; }
    void setKeyField(const QString &f);
    bool ordered() const { return m_ordered; }
    void setOrdered(bool on);
    int linger() const { return m_linger; }
    void setLinger(int ms);
    int epoch() const { return m_epoch; }
    void setEpoch(int e);

signals:
    void itemsChanged();
    void countChanged();

protected:
    void timerEvent(QTimerEvent *e) override;

private:
    struct Row {
        QString key;
        QVariantMap item;
        bool fresh = false;
        bool leaving = false;
    };
    void sync();
    void dropLeaving();

    QVariantList m_items;
    QString m_keyField = QStringLiteral("id");
    bool m_ordered = true;
    int m_linger = 260;
    int m_epoch = 0;
    bool m_synced = false;
    QList<Row> m_rows;
    QBasicTimer m_drop;
};

class CardSlot : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QVariantMap item READ item WRITE setItem NOTIFY itemChanged)
    Q_PROPERTY(QString stamp READ stamp WRITE setStamp NOTIFY stampChanged)
    Q_PROPERTY(bool fresh READ fresh WRITE setFresh NOTIFY freshChanged)
    Q_PROPERTY(bool leaving READ leaving WRITE setLeaving NOTIFY leavingChanged)
    Q_PROPERTY(int order READ order WRITE setOrder NOTIFY orderChanged)
    Q_PROPERTY(double cascadeFrom READ cascadeFrom WRITE setCascadeFrom NOTIFY cascadeFromChanged)
    Q_PROPERTY(bool first READ first NOTIFY edgesChanged)
    Q_PROPERTY(bool last READ last NOTIFY edgesChanged)

public:
    explicit CardSlot(QQuickItem *parent = nullptr);

    QVariantMap item() const { return m_item; }
    void setItem(const QVariantMap &item);
    QString stamp() const { return m_stamp; }
    void setStamp(const QString &s);
    bool fresh() const { return m_fresh; }
    void setFresh(bool on);
    bool leaving() const { return m_leaving; }
    void setLeaving(bool on);
    int order() const { return m_order; }
    void setOrder(int o);
    double cascadeFrom() const { return m_cascadeFrom; }
    void setCascadeFrom(double t);
    bool first() const { return m_first; }
    bool last() const { return m_last; }
    void setEdges(bool first, bool last);

    void place(qreal x, qreal y, bool glide);
    Q_INVOKABLE void enter(int delay);
    void popIn();
    void leave();

signals:
    void itemChanged();
    void stampChanged();
    void freshChanged();
    void leavingChanged();
    void orderChanged();
    void cascadeFromChanged();
    void edgesChanged();
    void shownUp();
    void updated();
    void reflow();

protected:
    void componentComplete() override;
    void timerEvent(QTimerEvent *e) override;

private:
    void fadeTo(double v, int ms, const QEasingCurve &curve);
    void wake();
    bool tick(double dt);
    void apply();

    QVariantMap m_item;
    QString m_stamp;
    bool m_fresh = false;
    bool m_leaving = false;
    int m_order = 0;
    double m_cascadeFrom = 0;
    bool m_first = false, m_last = false;
    bool m_ready = false;
    bool m_placed = false;
    SpringState m_x, m_y, m_rise, m_grow;
    double m_opacity = 1, m_fadeFrom = 1, m_fadeTo = 1;
    int m_fadeMs = 0;
    qint64 m_fadeAt = -1;
    const QEasingCurve *m_fadeCurve = nullptr;
    QElapsedTimer m_clock;
    QBasicTimer m_wait;
    SpringClock *m_driver;
};

class CardFlow : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(qreal cellMin READ cellMin WRITE setCellMin NOTIFY layoutChanged)
    Q_PROPERTY(qreal gap READ gap WRITE setGap NOTIFY layoutChanged)
    Q_PROPERTY(qreal cellWidth READ cellWidth NOTIFY layoutChanged)

public:
    explicit CardFlow(QQuickItem *parent = nullptr);

    qreal cellMin() const { return m_cellMin; }
    void setCellMin(qreal v);
    qreal gap() const { return m_gap; }
    void setGap(qreal v);
    qreal cellWidth() const;

    Q_INVOKABLE int cascade(int from, int step);

signals:
    void layoutChanged();

protected:
    void itemChange(ItemChange change, const ItemChangeData &value) override;
    void geometryChange(const QRectF &now, const QRectF &before) override;
    void updatePolish() override;

private:
    QList<CardSlot *> cards() const;
    void relayout(bool glide);

    qreal m_cellMin = 0;
    qreal m_gap = 12;
    bool m_glide = false;
    QElapsedTimer m_resized;
};
