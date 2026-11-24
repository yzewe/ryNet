#pragma once

#include <QObject>
#include <QQmlEngine>
#include <QStringList>
#include <QVariantList>

class SetOrder : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QVariantList sets READ sets WRITE setSets NOTIFY setsChanged)
    Q_PROPERTY(qreal top READ top WRITE setTop NOTIFY changed)
    Q_PROPERTY(qreal rowH READ rowH WRITE setRowH NOTIFY changed)
    Q_PROPERTY(qreal headH READ headH WRITE setHeadH NOTIFY changed)
    Q_PROPERTY(QString dragKey READ dragKey NOTIFY changed)
    Q_PROPERTY(qreal dragY READ dragY NOTIFY changed)
    Q_PROPERTY(int onCount READ onCount NOTIFY changed)
    Q_PROPERTY(int offCount READ offCount NOTIFY changed)
    Q_PROPERTY(qreal height READ height NOTIFY changed)
    Q_PROPERTY(qreal offY READ offY NOTIFY changed)
    Q_PROPERTY(int revision READ revision NOTIFY changed)

public:
    explicit SetOrder(QObject *parent = nullptr);

    QVariantList sets() const { return m_sets; }
    void setSets(const QVariantList &sets);
    qreal top() const { return m_top; }
    void setTop(qreal v);
    qreal rowH() const { return m_rowH; }
    void setRowH(qreal v);
    qreal headH() const { return m_headH; }
    void setHeadH(qreal v);
    QString dragKey() const { return m_dragKey; }
    qreal dragY() const { return m_dragY; }
    int onCount() const { return int(m_on.size()); }
    int offCount() const { return int(m_off.size()); }
    qreal height() const;
    qreal offY() const;
    int revision() const { return m_revision; }

    Q_INVOKABLE QVariantMap spot(const QString &key) const;
    Q_INVOKABLE QVariantMap set(const QString &key) const;
    Q_INVOKABLE void pick(const QString &key, qreal y);
    Q_INVOKABLE void move(qreal y);
    Q_INVOKABLE void drop();

signals:
    void setsChanged();
    void changed();
    void reordered(const QStringList &ids, int onCount);

private:
    void fromSets();
    void bump();

    QVariantList m_sets;
    QStringList m_on, m_off;
    qreal m_top = 0, m_rowH = 70, m_headH = 52;
    QString m_dragKey;
    qreal m_dragY = 0;
    int m_revision = 0;
};
