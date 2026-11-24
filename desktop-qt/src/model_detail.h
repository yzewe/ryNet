#pragma once

#include "model.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QMap>
#include <QRegularExpression>
#include <QString>
#include <QStringList>
#include <optional>

namespace ModelDetail {
inline std::optional<int> toIntStrict(const QString &s)
{
    static const QRegularExpression re(QStringLiteral("^[+-]?\\d{1,9}$"));
    if (!re.match(s).hasMatch())
        return {};
    return s.toInt();
}

inline QStringList lines(const QString &s)
{
    static const QRegularExpression re(QStringLiteral("\r\n|\r|\n"));
    return s.split(re);
}

inline QString str(const QJsonObject &o, const QString &k) { return o.value(k).isString() ? o.value(k).toString() : QString(); }

inline std::optional<QString> text(const QJsonObject &o, const QString &k)
{
    const QString v = str(o, k);
    if (v.trimmed().isEmpty())
        return {};
    return v;
}

inline int intOf(const QJsonObject &o, const QString &k, int d)
{
    const QJsonValue v = o.value(k);
    if (v.isDouble())
        return v.toInt(d);
    if (v.isString())
        return toIntStrict(v.toString().trimmed()).value_or(d);
    return d;
}

inline qint64 longOf(const QJsonObject &o, const QString &k)
{
    const QJsonValue v = o.value(k);
    if (v.isDouble())
        return qint64(v.toDouble());
    if (v.isString())
        return v.toString().toLongLong();
    return 0;
}

inline bool boolOf(const QJsonObject &o, const QString &k, bool d)
{
    const QJsonValue v = o.value(k);
    if (v.isBool())
        return v.toBool();
    if (v.isString()) {
        const QString s = v.toString().toLower();
        if (s == u"true")
            return true;
        if (s == u"false")
            return false;
    }
    return d;
}

inline QString keyOf(const QJsonObject &o, const QString &k, const QStringList &allowed, const QString &d)
{
    const QString v = str(o, k);
    return allowed.contains(v) ? v : d;
}

inline QStringList uniq(const QStringList &l)
{
    QStringList out;
    QSet<QString> seen;
    for (const QString &s : l)
        if (!seen.contains(s)) {
            seen.insert(s);
            out << s;
        }
    return out;
}

inline QJsonArray arr(const QStringList &l)
{
    QJsonArray a;
    for (const QString &s : l)
        a.append(s);
    return a;
}

inline QStringList strings(const QJsonValue &v)
{
    QStringList l;
    for (const QJsonValue &x : v.toArray())
        if (x.isString() && !x.toString().isEmpty())
            l << x.toString();
    return l;
}

inline QJsonObject listMap(const QMap<QString, QStringList> &m)
{
    QJsonObject o;
    for (auto it = m.begin(); it != m.end(); ++it)
        o[it.key()] = arr(it.value());
    return o;
}

inline QMap<QString, QStringList> listMapOf(const QJsonValue &v)
{
    QMap<QString, QStringList> m;
    const QJsonObject o = v.toObject();
    for (auto it = o.begin(); it != o.end(); ++it)
        if (const QStringList l = strings(it.value()); !l.isEmpty())
            m[it.key()] = l;
    return m;
}

inline QString substringAfter(const QString &s, const QString &d)
{
    const int i = s.indexOf(d);
    return i < 0 ? s : s.mid(i + d.size());
}

inline QString substringBefore(const QString &s, const QString &d)
{
    const int i = s.indexOf(d);
    return i < 0 ? s : s.left(i);
}

inline const QStringList kRuleKinds = {"domain", "ip", "app"};
inline const QStringList kActions = {"proxy", "direct", "block", "server"};
}
