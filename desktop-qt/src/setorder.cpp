#include "setorder.h"

#include <cmath>

SetOrder::SetOrder(QObject *parent)
    : QObject(parent)
{
}

void SetOrder::setSets(const QVariantList &sets)
{
    m_sets = sets;
    if (m_dragKey.isEmpty())
        fromSets();
    else
        bump();
    emit setsChanged();
}

void SetOrder::fromSets()
{
    m_on.clear();
    m_off.clear();
    for (const QVariant &v : std::as_const(m_sets)) {
        const QVariantMap s = v.toMap();
        (s.value(QStringLiteral("enabled")).toBool() ? m_on : m_off) << s.value(QStringLiteral("id")).toString();
    }
    bump();
}

void SetOrder::bump()
{
    ++m_revision;
    emit changed();
}

void SetOrder::setTop(qreal v)
{
    if (qFuzzyCompare(v + 1, m_top + 1))
        return;
    m_top = v;
    bump();
}

void SetOrder::setRowH(qreal v)
{
    if (qFuzzyCompare(v, m_rowH))
        return;
    m_rowH = v;
    bump();
}

void SetOrder::setHeadH(qreal v)
{
    if (qFuzzyCompare(v, m_headH))
        return;
    m_headH = v;
    bump();
}

qreal SetOrder::height() const
{
    const int n = int(m_sets.size());
    return m_top + (n > 0 ? m_headH : 0) + n * m_rowH + (!m_off.isEmpty() ? m_headH : 0);
}

qreal SetOrder::offY() const { return m_top + m_headH + m_on.size() * m_rowH; }

QVariantMap SetOrder::spot(const QString &key) const
{
    const qreal first = m_top + (m_sets.isEmpty() ? 0 : m_headH);
    if (const int i = int(m_on.indexOf(key)); i >= 0)
        return {{"y", first + i * m_rowH}, {"on", true}, {"pos", i}, {"count", m_on.size()}};
    if (const int i = int(m_off.indexOf(key)); i >= 0)
        return {{"y", first + (m_on.size() + i) * m_rowH + m_headH}, {"on", false}, {"pos", i}, {"count", m_off.size()}};
    return {{"y", first + m_sets.size() * m_rowH}, {"on", false}, {"pos", 0}, {"count", 0}};
}

QVariantMap SetOrder::set(const QString &key) const
{
    for (const QVariant &v : m_sets)
        if (v.toMap().value(QStringLiteral("id")).toString() == key)
            return v.toMap();
    return {};
}

void SetOrder::pick(const QString &key, qreal y)
{
    m_dragKey = key;
    m_dragY = y;
    bump();
}

void SetOrder::move(qreal y)
{
    m_dragY = y;
    const int from = int(m_on.indexOf(m_dragKey));
    const qreal mid = y + m_rowH / 2;
    const int to = std::clamp(int(std::floor((mid - m_top - m_headH) / m_rowH)), 0, std::max(0, int(m_on.size()) - 1));
    if (from >= 0 && to != from)
        m_on.move(from, to);
    bump();
}

void SetOrder::drop()
{
    if (m_dragKey.isEmpty())
        return;
    m_dragKey.clear();
    const QStringList on = m_on;
    emit reordered(on + m_off, int(on.size()));
    fromSets();
}
