#include "cards.h"

#include <QDateTime>
#include <QEasingCurve>
#include <QTimerEvent>
#include <cmath>

namespace {
const QEasingCurve &decelerate()
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.05, 0.7), QPointF(0.1, 1.0), QPointF(1, 1));
        return e;
    }();
    return c;
}

const QEasingCurve &accelerate()
{
    static const QEasingCurve c = [] {
        QEasingCurve e(QEasingCurve::BezierSpline);
        e.addCubicBezierSegment(QPointF(0.3, 0.0), QPointF(0.8, 0.15), QPointF(1, 1));
        return e;
    }();
    return c;
}

constexpr double kSpatialK = 380, kSpatialZ = 0.8, kFastK = 800;

void snap(SpringState &s, double v)
{
    s.value = s.target = v;
    s.velocity = 0;
    s.span = 0;
}

void tune(SpringState &s, double stiffness, double damping)
{
    s.stiffness = stiffness;
    s.damping = damping;
}
}

KeyedRows::KeyedRows(QObject *parent)
    : QAbstractListModel(parent)
{
}

int KeyedRows::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_rows.size()); }

QVariant KeyedRows::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_rows.size())
        return {};
    const Row &r = m_rows.at(index.row());
    switch (role) {
    case KeyRole:
        return r.key;
    case ItemRole:
        return r.item;
    case FreshRole:
        return r.fresh;
    case LeavingRole:
        return r.leaving;
    }
    return {};
}

QHash<int, QByteArray> KeyedRows::roleNames() const
{
    return {{KeyRole, "key"}, {ItemRole, "item"}, {FreshRole, "fresh"}, {LeavingRole, "leaving"}};
}

void KeyedRows::setItems(const QVariantList &items)
{
    m_items = items;
    sync();
    emit itemsChanged();
}

void KeyedRows::setKeyField(const QString &f)
{
    if (f == m_keyField)
        return;
    m_keyField = f;
    sync();
    emit itemsChanged();
}

void KeyedRows::setOrdered(bool on)
{
    if (on == m_ordered)
        return;
    m_ordered = on;
    emit itemsChanged();
}

void KeyedRows::setLinger(int ms)
{
    if (ms == m_linger)
        return;
    m_linger = ms;
    emit itemsChanged();
}

void KeyedRows::setEpoch(int e)
{
    if (e == m_epoch)
        return;
    m_epoch = e;
    m_synced = false;
    emit itemsChanged();
}

void KeyedRows::sync()
{
    QStringList want;
    QHash<QString, QVariantMap> byKey;
    for (const QVariant &v : std::as_const(m_items)) {
        const bool plain = v.typeId() == QMetaType::QString;
        const QVariantMap m = plain ? QVariantMap{{QStringLiteral("value"), v}} : v.toMap();
        const QString k = plain ? v.toString() : m.value(m_keyField).toString();
        want << k;
        byKey.insert(k, m);
    }
    const int before = int(m_rows.size());
    bool gone = false;
    for (int i = 0; i < m_rows.size(); ++i) {
        Row &r = m_rows[i];
        const auto it = byKey.constFind(r.key);
        const bool leave = it == byKey.cend();
        QList<int> roles;
        if (r.leaving != leave) {
            r.leaving = leave;
            roles << LeavingRole;
        }
        if (!leave && *it != r.item) {
            r.item = *it;
            roles << ItemRole;
        }
        if (!roles.isEmpty())
            emit dataChanged(index(i), index(i), roles);
        gone = gone || leave;
    }
    QStringList staying;
    for (const Row &r : std::as_const(m_rows))
        if (!r.leaving)
            staying << r.key;
    if (m_ordered) {
        QStringList kept;
        for (const QString &k : std::as_const(want))
            if (staying.contains(k))
                kept << k;
        if (kept != staying) {
            beginResetModel();
            m_rows.clear();
            for (const QString &k : std::as_const(want))
                m_rows.append({k, byKey.value(k), false, false});
            endResetModel();
            m_drop.stop();
            m_synced = true;
            if (m_rows.size() != before)
                emit countChanged();
            return;
        }
    }
    for (int w = 0; w < want.size(); ++w) {
        if (staying.contains(want[w]))
            continue;
        int at = int(m_rows.size());
        if (m_ordered) {
            at = 0;
            for (int i = 0; w > 0 && i < m_rows.size(); ++i)
                if (m_rows[i].key == want[w - 1])
                    at = i + 1;
        }
        beginInsertRows({}, at, at);
        m_rows.insert(at, {want[w], byKey.value(want[w]), m_synced, false});
        endInsertRows();
        staying << want[w];
    }
    m_synced = true;
    if (gone && m_linger > 0)
        m_drop.start(m_linger, this);
    else if (gone)
        dropLeaving();
    if (m_rows.size() != before)
        emit countChanged();
}

void KeyedRows::timerEvent(QTimerEvent *e)
{
    if (e->timerId() != m_drop.timerId())
        return QAbstractListModel::timerEvent(e);
    dropLeaving();
}

void KeyedRows::dropLeaving()
{
    m_drop.stop();
    for (int i = int(m_rows.size()) - 1; i >= 0; --i) {
        if (!m_rows[i].leaving)
            continue;
        beginRemoveRows({}, i, i);
        m_rows.removeAt(i);
        endRemoveRows();
    }
    emit countChanged();
}

CardSlot::CardSlot(QQuickItem *parent)
    : QQuickItem(parent)
    , m_driver(new SpringClock([this](double dt) { return tick(dt); }, this))
{
    tune(m_x, kSpatialK, kSpatialZ);
    tune(m_y, kSpatialK, kSpatialZ);
    tune(m_rise, kSpatialK, kSpatialZ);
    tune(m_grow, kSpatialK, kSpatialZ);
    snap(m_grow, 1);
    m_clock.start();
}

void CardSlot::setItem(const QVariantMap &item)
{
    if (item == m_item)
        return;
    m_item = item;
    emit itemChanged();
}

void CardSlot::setStamp(const QString &s)
{
    if (s == m_stamp)
        return;
    const bool again = m_ready;
    m_stamp = s;
    emit stampChanged();
    if (again) {
        tune(m_grow, kSpatialK, 0.5);
        m_grow.velocity += 0.9;
        wake();
        emit updated();
    }
}

void CardSlot::setFresh(bool on)
{
    if (on == m_fresh)
        return;
    m_fresh = on;
    emit freshChanged();
}

void CardSlot::setLeaving(bool on)
{
    if (on == m_leaving)
        return;
    m_leaving = on;
    setZ(on ? -1 : 0);
    emit leavingChanged();
    if (!m_ready)
        return;
    on ? leave() : popIn();
    emit reflow();
}

void CardSlot::setOrder(int o)
{
    if (o == m_order)
        return;
    m_order = o;
    emit orderChanged();
}

void CardSlot::setCascadeFrom(double t)
{
    if (t == m_cascadeFrom)
        return;
    m_cascadeFrom = t;
    emit cascadeFromChanged();
}

void CardSlot::setEdges(bool first, bool last)
{
    if (first == m_first && last == m_last)
        return;
    m_first = first;
    m_last = last;
    emit edgesChanged();
}

void CardSlot::place(qreal x, qreal y, bool glide)
{
    if (glide && m_placed) {
        m_x.target = x;
        m_y.target = y;
    } else {
        snap(m_x, x);
        snap(m_y, y);
    }
    m_placed = true;
    apply();
    wake();
}

void CardSlot::enter(int delay)
{
    if (m_leaving)
        return;
    m_fadeAt = -1;
    m_opacity = 0;
    snap(m_rise, 24);
    tune(m_grow, kSpatialK, kSpatialZ);
    snap(m_grow, 0.95);
    apply();
    m_wait.start(std::max(1, delay), this);
}

void CardSlot::popIn()
{
    m_wait.stop();
    snap(m_rise, 0);
    tune(m_grow, 520, 0.62);
    snap(m_grow, 0.8);
    m_grow.target = 1;
    fadeTo(1, 200, decelerate());
    emit shownUp();
}

void CardSlot::leave()
{
    m_wait.stop();
    tune(m_grow, kFastK, 1);
    m_grow.target = 0.86;
    fadeTo(0, 170, accelerate());
}

void CardSlot::fadeTo(double v, int ms, const QEasingCurve &curve)
{
    m_fadeFrom = m_opacity;
    m_fadeTo = v;
    m_fadeMs = ms;
    m_fadeCurve = &curve;
    m_fadeAt = m_clock.elapsed();
    wake();
}

void CardSlot::componentComplete()
{
    QQuickItem::componentComplete();
    m_ready = true;
    if (m_fresh) {
        popIn();
    } else if (m_cascadeFrom > 0) {
        const double t = double(QDateTime::currentMSecsSinceEpoch()) - m_cascadeFrom;
        if (t < 400)
            enter(int(70 + 45 * m_order - t));
    }
}

void CardSlot::timerEvent(QTimerEvent *e)
{
    if (e->timerId() != m_wait.timerId())
        return QQuickItem::timerEvent(e);
    m_wait.stop();
    m_rise.target = 0;
    m_grow.target = 1;
    fadeTo(1, 240, decelerate());
    emit shownUp();
}

void CardSlot::wake()
{
    if (m_driver->state() != QAbstractAnimation::Running)
        m_driver->start();
}

bool CardSlot::tick(double dt)
{
    bool rest = true;
    for (SpringState *s : {&m_x, &m_y, &m_rise, &m_grow})
        rest = s->step(dt) && rest;
    if (m_fadeAt >= 0) {
        const double t = m_fadeMs > 0 ? std::min(1.0, double(m_clock.elapsed() - m_fadeAt) / m_fadeMs) : 1.0;
        m_opacity = m_fadeFrom + (m_fadeTo - m_fadeFrom) * m_fadeCurve->valueForProgress(t);
        if (t >= 1)
            m_fadeAt = -1;
        else
            rest = false;
    }
    apply();
    return rest;
}

void CardSlot::apply()
{
    setX(m_x.value);
    setY(m_y.value + m_rise.value);
    setScale(m_grow.value);
    setOpacity(m_opacity);
}

CardFlow::CardFlow(QQuickItem *parent)
    : QQuickItem(parent)
{
}

void CardFlow::setCellMin(qreal v)
{
    if (qFuzzyCompare(v, m_cellMin))
        return;
    m_cellMin = v;
    emit layoutChanged();
    relayout(false);
}

void CardFlow::setGap(qreal v)
{
    if (qFuzzyCompare(v, m_gap))
        return;
    m_gap = v;
    emit layoutChanged();
    relayout(false);
}

static int columnsFor(qreal width, qreal cellMin, qreal gap)
{
    return cellMin > 0 ? std::max(1, int(std::floor((width + gap) / (cellMin + gap)))) : 1;
}

qreal CardFlow::cellWidth() const
{
    const int cols = columnsFor(width(), m_cellMin, m_gap);
    return (width() - m_gap * (cols - 1)) / cols;
}

QList<CardSlot *> CardFlow::cards() const
{
    QList<CardSlot *> out;
    for (QQuickItem *child : childItems())
        if (auto *c = qobject_cast<CardSlot *>(child); c && !c->leaving())
            out << c;
    return out;
}

int CardFlow::cascade(int from, int step)
{
    const QList<CardSlot *> list = cards();
    for (int k = 0; k < list.size(); ++k)
        list[k]->enter(from + step * k);
    return int(list.size());
}

void CardFlow::itemChange(ItemChange change, const ItemChangeData &value)
{
    QQuickItem::itemChange(change, value);
    if (change == ItemChildAddedChange) {
        if (auto *c = qobject_cast<CardSlot *>(value.item)) {
            connect(c, &QQuickItem::heightChanged, this, [this] { relayout(true); });
            connect(c, &CardSlot::reflow, this, [this] { relayout(true); });
            relayout(true);
        }
    } else if (change == ItemChildRemovedChange) {
        if (auto *c = qobject_cast<CardSlot *>(value.item)) {
            disconnect(c, nullptr, this, nullptr);
            relayout(true);
        }
    }
}

void CardFlow::geometryChange(const QRectF &now, const QRectF &before)
{
    QQuickItem::geometryChange(now, before);
    if (now.width() != before.width()) {
        m_resized.start();
        emit layoutChanged();
        relayout(false);
    }
}

void CardFlow::relayout(bool glide)
{
    m_glide = m_glide || glide;
    polish();
}

void CardFlow::updatePolish()
{
    const bool animate = m_glide && (!m_resized.isValid() || m_resized.elapsed() > 300);
    m_glide = false;
    const QList<CardSlot *> list = cards();
    const int cols = columnsFor(width(), m_cellMin, m_gap);
    const qreal w = cellWidth();
    qreal top = 0;
    for (int r = 0; r < list.size(); r += cols) {
        const int end = std::min(int(list.size()), r + cols);
        qreal h = 0;
        for (int k = r; k < end; ++k)
            h = std::max(h, list[k]->height());
        for (int k = r; k < end; ++k) {
            list[k]->setEdges(k == 0, k == list.size() - 1);
            list[k]->place((k - r) * (w + m_gap), top, animate);
        }
        top += h + m_gap;
    }
    setHeight(std::max<qreal>(0, top - m_gap));
}
