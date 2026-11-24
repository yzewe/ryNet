#include "segments.h"

#include <QQmlProperty>

SegmentRow::SegmentRow(QQuickItem *parent)
    : QQuickItem(parent)
{
}

void SegmentRow::setGap(qreal v)
{
    if (qFuzzyCompare(v, m_gap))
        return;
    m_gap = v;
    emit gapChanged();
    relayout();
}

QList<QQuickItem *> SegmentRow::segments() const
{
    QList<QQuickItem *> out;
    for (QQuickItem *c : childItems())
        if (c->property("weight").isValid())
            out << c;
    return out;
}

QQuickItem *SegmentRow::segment(int index) const
{
    const QList<QQuickItem *> list = segments();
    return index >= 0 && index < list.size() ? list[index] : nullptr;
}

int SegmentRow::indexAt(qreal x) const
{
    const QList<QQuickItem *> list = segments();
    for (int i = 0; i < list.size(); ++i)
        if (x < list[i]->x() + list[i]->width() + m_gap / 2)
            return i;
    return int(list.size()) - 1;
}

void SegmentRow::itemChange(ItemChange change, const ItemChangeData &value)
{
    QQuickItem::itemChange(change, value);
    if (change == ItemChildAddedChange && value.item) {
        for (const char *name : {"weight", "swell"}) {
            QQmlProperty p(value.item, QLatin1String(name));
            if (p.isValid())
                p.connectNotifySignal(this, SLOT(relayout()));
        }
        relayout();
    } else if (change == ItemChildRemovedChange) {
        if (value.item)
            disconnect(value.item, nullptr, this, nullptr);
        relayout();
    }
}

void SegmentRow::geometryChange(const QRectF &now, const QRectF &before)
{
    QQuickItem::geometryChange(now, before);
    if (now.size() != before.size())
        relayout();
}

void SegmentRow::relayout()
{
    polish();
}

void SegmentRow::updatePolish()
{
    const QList<QQuickItem *> list = segments();
    qreal total = 0, natural = 0;
    for (QQuickItem *s : list) {
        const qreal w = s->property("weight").toReal();
        const QVariant swell = s->property("swell");
        total += w * (swell.isValid() ? swell.toReal() : 1);
        natural += w;
    }
    setImplicitWidth(natural + m_gap * std::max<qsizetype>(0, list.size() - 1));
    const qreal room = width() - m_gap * std::max<qsizetype>(0, list.size() - 1);
    qreal x = 0;
    for (QQuickItem *s : list) {
        const QVariant swell = s->property("swell");
        const qreal w = std::max<qreal>(0, room * s->property("weight").toReal() * (swell.isValid() ? swell.toReal() : 1) / std::max<qreal>(1, total));
        s->setX(x);
        s->setY(0);
        s->setWidth(w);
        s->setHeight(height());
        x += w + m_gap;
    }
}

Picker::Picker(QObject *parent)
    : QObject(parent)
{
}

void Picker::setItems(const QList<QQuickItem *> &items)
{
    if (items == m_items)
        return;
    m_items = items;
    emit itemsChanged();
    rewatch();
}

void Picker::setContainer(QQuickItem *c)
{
    if (c == m_container)
        return;
    if (m_container)
        disconnect(m_container, nullptr, this, nullptr);
    m_container = c;
    if (c)
        connect(c, &QQuickItem::childrenChanged, this, &Picker::rewatch);
    emit itemsChanged();
    rewatch();
}

void Picker::setSelectedRole(const QString &r)
{
    if (r == m_role)
        return;
    m_role = r;
    emit itemsChanged();
    rewatch();
}

QList<QQuickItem *> Picker::watched() const
{
    if (!m_container)
        return m_items;
    QList<QQuickItem *> out;
    for (QQuickItem *c : m_container->childItems())
        if (c->property(m_role.toUtf8().constData()).isValid())
            out << c;
    return out;
}

void Picker::rewatch()
{
    for (const QPointer<QQuickItem> &i : std::as_const(m_watching))
        if (i)
            disconnect(i, nullptr, this, nullptr);
    m_watching.clear();
    for (QQuickItem *i : watched()) {
        if (!i)
            continue;
        for (const QString &name : {m_role, QStringLiteral("hovered"), QStringLiteral("keyboardFocus")}) {
            QQmlProperty p(i, name);
            if (p.isValid())
                p.connectNotifySignal(this, SLOT(update()));
        }
        connect(i, &QObject::destroyed, this, &Picker::rewatch, Qt::QueuedConnection);
        m_watching << i;
    }
    update();
}

void Picker::update()
{
    QQuickItem *sel = nullptr, *hov = nullptr, *focus = nullptr;
    const QByteArray role = m_role.toUtf8();
    for (const QPointer<QQuickItem> &i : std::as_const(m_watching)) {
        if (!i)
            continue;
        if (!sel && i->property(role.constData()).toBool())
            sel = i;
        if (!hov && i->property("hovered").toBool())
            hov = i;
        if (!focus && i->property("keyboardFocus").toBool())
            focus = i;
    }
    if (!hov)
        hov = focus;
    if (sel == m_selected && hov == m_hovered)
        return;
    m_selected = sel;
    m_hovered = hov;
    emit pickedChanged();
}
