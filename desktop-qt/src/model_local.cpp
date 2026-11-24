#include "model_detail.h"
#include <QCollator>
#include <QDateTime>
#include <QHash>
#include <QHostAddress>
#include <QJsonDocument>
#include <QLocale>
#include <QRegularExpression>
#include <QSet>
#include <QUrl>
#include <QUuid>
#include <algorithm>
#include <zlib.h>

using namespace ModelDetail;

namespace {
const QStringList kSchemes = {"vless", "vmess", "trojan", "ss", "hysteria2", "hy2", "hysteria", "tuic", "wireguard", "wg", "socks", "socks4", "socks4a", "socks5", "socks5h", "http-proxy", "https-proxy", "anytls"};

QString scheme(const QString &line)
{
    const QString t = line.trimmed();
    const int i = t.indexOf(QStringLiteral("://"));
    return i < 0 ? QString() : t.left(i).toLower();
}

QStringList links(const QString &text)
{
    const QString t = text.trimmed();
    QString plain;
    if (t.contains(QStringLiteral("://"))) {
        plain = t;
    } else {
        QByteArray compact;
        for (const QChar c : t)
            if (!c.isSpace())
                compact += c.toLatin1();
        auto std = QByteArray::fromBase64Encoding(compact, QByteArray::Base64Encoding | QByteArray::AbortOnBase64DecodingErrors);
        if (std) {
            plain = QString::fromUtf8(std.decoded);
        } else {
            QByteArray trimmed = compact;
            while (trimmed.endsWith('='))
                trimmed.chop(1);
            auto url = QByteArray::fromBase64Encoding(trimmed, QByteArray::Base64UrlEncoding | QByteArray::AbortOnBase64DecodingErrors);
            if (url)
                plain = QString::fromUtf8(url.decoded);
        }
    }
    QStringList out;
    for (const QString &l : lines(plain)) {
        const QString x = l.trimmed();
        if (kSchemes.contains(scheme(x)))
            out << x;
    }
    return out;
}

QJsonArray parseEntries(const QString &text)
{
    const QString s = text.trimmed();
    if (s.startsWith(u'[') || s.startsWith(u'{')) {
        QJsonParseError err;
        const QJsonDocument doc = QJsonDocument::fromJson(s.toUtf8(), &err);
        if (err.error != QJsonParseError::NoError)
            throw Fail(t("Конфиг не читается: %1", "Can't read the config: %1").arg(err.errorString()));
        QJsonArray out;
        if (doc.isArray()) {
            for (const QJsonValue &v : doc.array())
                if (v.isString() || v.isObject())
                    out.append(v);
        } else if (doc.isObject()) {
            out.append(doc.object());
        }
        return out;
    }
    QJsonArray out;
    for (const QString &l : links(s))
        out.append(l);
    return out;
}

QString entryKey(const QJsonValue &v)
{
    if (v.isObject()) {
        QJsonObject o = v.toObject();
        o.remove("remarks");
        return QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact));
    }
    return substringBefore(v.toString(), QStringLiteral("#"));
}
}

QString Local::title() { return t("Свои серверы", "My servers"); }

bool Local::isLocal(const Sub &sub) { return sub.url == URL; }

QString Local::serverInput(const QString &text)
{
    if (text.trimmed().startsWith(u'{') || text.trimmed().startsWith(u'[')) return text;
    QStringList out;
    for (const QString &line : text.split(u'\n')) {
        const QString t = line.trimmed();
        const QString protocol = scheme(t);
        out << (protocol == u"http" || protocol == u"https" ? protocol + QStringLiteral("-proxy") + t.mid(t.indexOf(u':')) : line);
    }
    return out.join(u'\n');
}

bool Local::isServers(const QString &text)
{
    const QString t = text.trimmed();
    if (t.startsWith(u'{') || t.startsWith(u'['))
        return true;
    QString first;
    for (const QString &l : lines(t)) {
        const QString x = l.trimmed();
        if (!x.isEmpty() && !x.startsWith(u'#')) {
            first = x;
            break;
        }
    }
    if (first.isEmpty())
        return false;
    if (first.contains(QStringLiteral("://")))
        return kSchemes.contains(scheme(first));
    return !links(t).isEmpty();
}

QJsonArray Local::entries(const QString &body)
{
    const QJsonDocument doc = QJsonDocument::fromJson(body.toUtf8());
    return doc.isArray() ? doc.array() : QJsonArray();
}

QString Local::merge(const QString &body, const QString &text)
{
    const QJsonArray found = parseEntries(text);
    if (found.isEmpty())
        throw Fail(t("Не похоже на ссылку на сервер или конфиг", "This doesn't look like a server link or config"));
    QJsonArray all = entries(body);
    for (const QJsonValue &e : found) {
        const QString k = entryKey(e);
        int at = -1;
        for (int i = 0; i < all.size(); ++i)
            if (entryKey(all.at(i)) == k) {
                at = i;
                break;
            }
        if (at >= 0)
            all[at] = e;
        else
            all.append(e);
    }
    return QString::fromUtf8(QJsonDocument(all).toJson(QJsonDocument::Compact));
}

QStringList Local::preview(const QString &text)
{
    QStringList out;
    try {
        for (const QJsonValue &e : parseEntries(text)) {
            QString label = name(e).value_or(QString()).trimmed();
            if (label.isEmpty() && e.isString())
                label = QUrl(e.toString()).host();
            out << (label.isEmpty() ? t("сервер без названия", "unnamed server") : label);
        }
    } catch (const Fail &) {
    }
    return out;
}

std::optional<QString> Local::name(const QJsonValue &entry)
{
    if (entry.isObject()) {
        const QString r = entry.toObject().value("remarks").toString();
        if (r.isEmpty())
            return {};
        return r;
    }
    const QString link = entry.toString();
    if (link.startsWith(QStringLiteral("vmess://"), Qt::CaseInsensitive) && !link.contains(u'@')) {
        const auto raw = QByteArray::fromBase64Encoding(link.mid(8).trimmed().toLatin1(), QByteArray::AbortOnBase64DecodingErrors);
        if (!raw)
            return {};
        const QJsonDocument doc = QJsonDocument::fromJson(raw.decoded);
        if (!doc.isObject())
            return {};
        return doc.object().value("ps").toString();
    }
    const int hash = link.indexOf(u'#');
    if (hash < 0)
        return {};
    const QString frag = substringBefore(link.mid(hash + 1), QStringLiteral("?serverDescription="));
    if (frag.isEmpty())
        return {};
    return QUrl::fromPercentEncoding(frag.toUtf8());
}

std::optional<QString> Local::without(const QString &body, const QString &remarks)
{
    QJsonArray all = entries(body);
    int at = -1;
    for (int i = 0; i < all.size(); ++i) {
        const auto n = name(all.at(i));
        if (n && n->trimmed() == remarks.trimmed()) {
            at = i;
            break;
        }
    }
    if (at < 0)
        return body;
    all.removeAt(at);
    if (all.isEmpty())
        return {};
    return QString::fromUtf8(QJsonDocument(all).toJson(QJsonDocument::Compact));
}

QString Local::renamed(const QString &body, const QString &remarks, const QString &name)
{
    QJsonArray all = entries(body);
    for (int i = 0; i < all.size(); ++i) {
        const auto n = Local::name(all.at(i));
        if (!n || n->trimmed() != remarks.trimmed())
            continue;
        if (all.at(i).isObject()) {
            QJsonObject o = all.at(i).toObject();
            o["remarks"] = name;
            all[i] = o;
        } else {
            const QString link = all.at(i).toString();
            QString next;
            if (link.startsWith(QStringLiteral("vmess://"), Qt::CaseInsensitive) && !link.contains(u'@')) {
                const auto raw = QByteArray::fromBase64Encoding(link.mid(8).trimmed().toLatin1(), QByteArray::AbortOnBase64DecodingErrors);
                QJsonObject o = QJsonDocument::fromJson(raw ? raw.decoded : QByteArray()).object();
                o["ps"] = name;
                next = QStringLiteral("vmess://") + QString::fromLatin1(QJsonDocument(o).toJson(QJsonDocument::Compact).toBase64());
            } else {
                const qsizetype hash = link.indexOf(u'#');
                const QString frag = hash < 0 ? QString() : link.mid(hash + 1);
                const qsizetype desc = frag.indexOf(QStringLiteral("?serverDescription="));
                next = link.left(hash < 0 ? link.size() : hash) + u'#' + QString::fromUtf8(QUrl::toPercentEncoding(name))
                       + (desc >= 0 ? frag.mid(desc) : QString());
            }
            all[i] = next;
        }
        break;
    }
    return QString::fromUtf8(QJsonDocument(all).toJson(QJsonDocument::Compact));
}

Sub Local::sub()
{
    Sub s;
    s.id = ID;
    s.url = URL;
    s.title = title();
    s.updated = nowSeconds();
    return s;
}
