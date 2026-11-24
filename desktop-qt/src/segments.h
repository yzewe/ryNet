#pragma once

#include <QPointer>
#include <QQuickItem>

class SegmentRow : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(qreal gap READ gap WRITE setGap NOTIFY gapChanged)

public:
    explicit SegmentRow(QQuickItem *parent = nullptr);

    qreal gap() const { return m_gap; }
    void setGap(qreal v);

    Q_INVOKABLE int indexAt(qreal x) const;
    Q_INVOKABLE QQuickItem *segment(int index) const;

signals:
    void gapChanged();

protected:
    void itemChange(ItemChange change, const ItemChangeData &value) override;
    void geometryChange(const QRectF &now, const QRectF &before) override;
    void updatePolish() override;

private slots:
    void relayout();

private:
    QList<QQuickItem *> segments() const;
    qreal m_gap = 4;
};

class Picker : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QList<QQuickItem *> items READ items WRITE setItems NOTIFY itemsChanged)
    Q_PROPERTY(QQuickItem *container READ container WRITE setContainer NOTIFY itemsChanged)
    Q_PROPERTY(QString selectedRole READ selectedRole WRITE setSelectedRole NOTIFY itemsChanged)
    Q_PROPERTY(QQuickItem *selected READ selected NOTIFY pickedChanged)
    Q_PROPERTY(QQuickItem *hovered READ hovered NOTIFY pickedChanged)

public:
    explicit Picker(QObject *parent = nullptr);

    QList<QQuickItem *> items() const { return m_items; }
    void setItems(const QList<QQuickItem *> &items);
    QQuickItem *container() const { return m_container; }
    void setContainer(QQuickItem *c);
    QString selectedRole() const { return m_role; }
    void setSelectedRole(const QString &r);
    QQuickItem *selected() const { return m_selected; }
    QQuickItem *hovered() const { return m_hovered; }

signals:
    void itemsChanged();
    void pickedChanged();

private slots:
    void update();
    void rewatch();

private:
    QList<QQuickItem *> watched() const;

    QList<QQuickItem *> m_items;
    QPointer<QQuickItem> m_container;
    QString m_role = QStringLiteral("selected");
    QList<QPointer<QQuickItem>> m_watching;
    QPointer<QQuickItem> m_selected, m_hovered;
};
