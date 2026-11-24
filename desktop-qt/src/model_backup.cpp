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
const QList<QPair<QString, QStringList>> kBackupKeys = {
    {"routing", {"routingMode", "rules", "sets", "userRules", "fakeDns"}},
    {"network", {"ipv4Only", "chain", "chainSub", "chainProfile", "fragment", "fragmentLength", "fragmentInterval", "mtu", "killSwitch", "processes", "processesMode", "directPorts", "serverDns", "serverDnsCustom", "dnsServers", "perServerDns"}},
    {"ping", {"pingOnStart", "pingMode", "pingParallel", "pingTimeout", "pingUrl", "ipUrl", "healthCheck", "healthSec", "healthInterval", "healthNotify", "autoSwitch", "autoSelect", "autoFeature", "autoInterval",
              "autoSkipSubs", "autoSkip", "autoWlSubs", "autoWl", "autoPins", "autoTransports", "autoWhitelist", "autoHome", "autoFree",
              "autoMargin", "autoSpeed", "autoSaveData", "autoNotify", "reserveOnlyOnWhitelists"}},
    {"subSettings", {"autoUpdate", "sendHwid", "subClient", "subHeaders", "hwid", "refreshNotices"}},
    {"app", {"theme", "font", "wallpaperColors", "seedColor", "sectionRadius", "sectionDivider", "sort", "view", "grid", "serverInfo", "exitFlags", "pins", "autostart", "debugLog", "sendLogs", "updateCheck"}},
    {"desktop", {"connMode", "port", "systemProxy"}},
};
constexpr qsizetype kBackupLimit = 32 << 20;
const QByteArray kMagic = "RYNET";
}

const QList<Backup::Part> &Backup::parts()
{
    static const QList<Part> all = {
        {"subs", "Подписки и выбранный сервер", "Subscriptions and selected server"},
        {"routing", "Маршрутизация и свои правила", "Routing and custom rules"},
        {"network", "Сеть: цепочка, фрагментация, MTU", "Network: chain, fragmentation, MTU"},
        {"ping", "Пинг, проверка сервера и автовыбор", "Ping, server check and auto select"},
        {"subSettings", "Обновление подписок и HWID", "Subscription updates and HWID"},
        {"app", "Внешний вид и приложение", "Appearance and app"},
        {"desktop", "Режим подключения и порт", "Connection mode and port"},
    };
    return all;
}

QByteArray Backup::encode(const Settings &settings, const QList<Sub> &subs, const std::function<std::optional<QString>(const QString &)> &body,
                          const QStringList &chosen)
{
    QJsonObject parts;
    const QJsonObject all = settings.toJson();
    for (const auto &[part, names] : kBackupKeys) {
        if (!chosen.contains(part))
            continue;
        QJsonObject o;
        for (const QString &n : names)
            if (all.contains(n))
                o[n] = all.value(n);
        parts[part] = o;
    }
    QJsonArray list;
    for (const Sub &sub : subs) {
        const auto text = body(sub.id);
        if (!text)
            continue;
        QJsonObject info = sub.toJson();
        info["updatedAt"] = sub.updated * 1000;
        if (sub.intervalHours)
            info["updateIntervalHours"] = *sub.intervalHours;
        list.append(QJsonObject{{"id", sub.id}, {"info", info}, {"body", *text}});
    }
    QJsonObject selection;
    if (settings.sub)
        selection["sub"] = *settings.sub;
    if (settings.profile)
        selection["profile"] = *settings.profile;
    if (chosen.contains(QStringLiteral("subs")))
        parts["subs"] = QJsonObject{{"list", list}, {"selection", selection}};
    const QJsonObject root{{"app", QStringLiteral("ryNet Desktop")}, {"created", nowMillis()}, {"parts", parts}};
    const QByteArray text = QJsonDocument(root).toJson(QJsonDocument::Compact);
    if (text.size() > kBackupLimit)
        throw Fail(t("Резервная копия слишком большая", "The backup is too large"));
    return kMagic + char(1) + Zlib::gzip(text);
}

namespace {
QJsonObject backupParts(const QByteArray &bytes)
{
    if (bytes.size() <= 6 || !bytes.startsWith(kMagic))
        throw Fail(t("Это не резервная копия ryNet", "This is not a ryNet backup"));
    if (bytes[5] != 1)
        throw Fail(t("Неизвестная версия резервной копии", "Unknown backup version"));
    bool tooLarge = false;
    const QByteArray json = Zlib::gunzip(bytes.mid(6), kBackupLimit);
    if (json.isNull())
        tooLarge = true;
    if (tooLarge)
        throw Fail(t("Резервная копия слишком большая или повреждена", "The backup is too large or damaged"));
    const QJsonDocument doc = QJsonDocument::fromJson(json);
    if (!doc.isObject() || !doc.object().value("parts").isObject())
        throw Fail(t("Это не резервная копия ryNet", "This is not a ryNet backup"));
    return doc.object().value("parts").toObject();
}
}

QStringList Backup::partsIn(const QByteArray &bytes, int *subs)
{
    const QJsonObject parts = backupParts(bytes);
    QStringList out;
    for (const Part &p : Backup::parts())
        if (parts.value(p.key).isObject())
            out << p.key;
    if (subs)
        *subs = int(parts.value("subs").toObject().value("list").toArray().size());
    return out;
}

Backup::Snapshot Backup::decode(const QByteArray &bytes, const Settings &current, const QStringList &chosen)
{
    QJsonObject parts = backupParts(bytes);
    for (const QString &k : parts.keys())
        if (!chosen.contains(k))
            parts.remove(k);
    QJsonObject merged = current.toJson();
    for (const auto &[part, names] : kBackupKeys) {
        const QJsonObject src = parts.value(part).toObject();
        for (const QString &n : names)
            if (src.contains(n))
                merged[n] = src.value(n);
        if (src.contains("healthInterval") && !src.contains("healthSec"))
            merged.remove("healthSec");
    }
    const QJsonObject subPart = parts.value("subs").toObject();
    Snapshot out;
    static const QRegularExpression idRe(QStringLiteral("^[A-Za-z0-9-]{1,64}$"));
    QSet<QString> ids;
    for (const QJsonValue &v : subPart.value("list").toArray()) {
        const QJsonObject entry = v.toObject();
        const QString id = entry.value("id").toString();
        if (!idRe.match(id).hasMatch())
            throw Fail(t("Неверный идентификатор подписки", "Invalid subscription ID"));
        QJsonObject info = entry.value("info").toObject();
        info["id"] = id;
        info["updated"] = qint64(info.value("updatedAt").toDouble() / 1000);
        if (info.contains("updateIntervalHours"))
            info["intervalHours"] = info.value("updateIntervalHours");
        else
            info.remove("intervalHours");
        Sub sub;
        try {
            sub = Sub::from(info);
        } catch (const Fail &) {
            throw Fail(t("Неверная ссылка подписки", "Invalid subscription link"));
        }
        const QUrl url(sub.url, QUrl::StrictMode);
        if (!url.isValid() || (url.scheme() != u"https" && url.scheme() != u"http") || url.host().isEmpty())
            throw Fail(t("Неверная ссылка подписки", "Invalid subscription link"));
        if (!entry.value("body").isString())
            throw Fail(t("Это не резервная копия ryNet", "This is not a ryNet backup"));
        if (ids.contains(id))
            throw Fail(t("Повтор идентификатора подписки", "Duplicate subscription ID"));
        ids.insert(id);
        out.subs.append({sub, entry.value("body").toString()});
    }
    const QJsonObject sel = subPart.value("selection").toObject();
    if (sel.contains("sub") && ids.contains(sel.value("sub").toString())) {
        merged["sub"] = sel.value("sub").toString();
        merged["profile"] = sel.value("profile").toString();
    }
    out.settings = Settings::from(merged);
    const Settings &s = out.settings;
    if (s.port < 1 || s.port > 65535 || s.pingParallel < 1 || s.pingParallel > 64 || s.pingTimeout < 1 || s.pingTimeout > 60)
        throw Fail(t("Неверные настройки резервной копии", "Invalid backup settings"));
    if (s.mtu != 0 && (s.mtu < 1280 || s.mtu > 9000))
        throw Fail(t("Неверный MTU", "Invalid MTU"));
    if (s.fragment && !(Fragment::validRange(s.fragmentLength, 65535) && Fragment::validRange(s.fragmentInterval, 1000)))
        throw Fail(t("Неверная фрагментация", "Invalid fragmentation"));
    return out;
}

static QByteArray deflateWith(const QByteArray &data, int level, int windowBits)
{
    z_stream z{};
    if (deflateInit2(&z, level, Z_DEFLATED, windowBits, 8, Z_DEFAULT_STRATEGY) != Z_OK)
        return {};
    QByteArray out;
    out.resize(qsizetype(deflateBound(&z, uLong(data.size()))) + 64);
    z.next_in = reinterpret_cast<Bytef *>(const_cast<char *>(data.constData()));
    z.avail_in = uInt(data.size());
    z.next_out = reinterpret_cast<Bytef *>(out.data());
    z.avail_out = uInt(out.size());
    const int rc = deflate(&z, Z_FINISH);
    const qsizetype n = qsizetype(z.total_out);
    deflateEnd(&z);
    if (rc != Z_STREAM_END)
        return {};
    out.resize(n);
    return out;
}

static std::optional<QByteArray> inflateWith(const QByteArray &data, int windowBits, qsizetype limit)
{
    z_stream z{};
    if (inflateInit2(&z, windowBits) != Z_OK)
        return {};
    QByteArray out;
    char buf[16384];
    z.next_in = reinterpret_cast<Bytef *>(const_cast<char *>(data.constData()));
    z.avail_in = uInt(data.size());
    int rc = Z_OK;
    while (rc != Z_STREAM_END) {
        z.next_out = reinterpret_cast<Bytef *>(buf);
        z.avail_out = sizeof(buf);
        rc = inflate(&z, Z_NO_FLUSH);
        if (rc != Z_OK && rc != Z_STREAM_END) {
            inflateEnd(&z);
            return {};
        }
        out.append(buf, qsizetype(sizeof(buf) - z.avail_out));
        if (out.size() > limit) {
            inflateEnd(&z);
            return {};
        }
        if (rc == Z_OK && z.avail_in == 0 && z.avail_out != 0)
            break;
    }
    inflateEnd(&z);
    if (rc != Z_STREAM_END)
        return {};
    return out;
}

QByteArray Zlib::gzip(const QByteArray &data) { return deflateWith(data, Z_DEFAULT_COMPRESSION, 15 + 16); }

QByteArray Zlib::gunzip(const QByteArray &data, qsizetype limit)
{
    const auto r = inflateWith(data, 15 + 32, limit);
    return r ? *r : QByteArray();
}

QByteArray Zlib::deflateRaw(const QByteArray &data) { return deflateWith(data, 9, -15); }

std::optional<QByteArray> Zlib::inflateRaw(const QByteArray &data, qsizetype limit) { return inflateWith(data, -15, limit); }

quint32 Zlib::crc32(const QByteArray &data)
{
    return quint32(::crc32(0L, reinterpret_cast<const Bytef *>(data.constData()), uInt(data.size())));
}

QVariantMap DeepLink::parse(const QString &link)
{
    const QString raw = link.trimmed();
    if (!raw.startsWith(u"rynet://", Qt::CaseInsensitive))
        return {};
    const QString rest = raw.mid(8);
    const QString host = rest.section(u'/', 0, 0).section(u'?', 0, 0).toLower();
    static const QStringList hosts{QStringLiteral("add"), QStringLiteral("import"), QStringLiteral("install-config"), QStringLiteral("install-sub"), QStringLiteral("sub"), QStringLiteral("subscription")};
    if (!hosts.contains(host))
        return {};
    QString after = rest.mid(host.size());
    if (after.startsWith(u'/'))
        after.remove(0, 1);
    QHash<QString, QString> params;
    if (after.startsWith(u'?'))
        for (const QString &kv : after.mid(1).split(u'&')) {
            const qsizetype eq = kv.indexOf(u'=');
            if (eq > 0)
                params.insert(kv.left(eq).toLower(), QUrl::fromPercentEncoding(kv.mid(eq + 1).toUtf8()));
        }
    const QString name = params.value(QStringLiteral("name"), params.value(QStringLiteral("title"))).trimmed();
    QString payload = params.value(QStringLiteral("url"), params.value(QStringLiteral("link"), params.value(QStringLiteral("config"))));
    if (payload.isEmpty() && !after.trimmed().isEmpty() && !after.startsWith(u'?'))
        payload = after.contains(u"://") ? after : QUrl::fromPercentEncoding(after.toUtf8());
    payload = payload.trimmed();
    if (payload.isEmpty())
        return {};
    QString value = payload;
    if (!value.contains(u"://")) {
        QString t = value;
        t.replace(u'-', u'+').replace(u'_', u'/');
        const QString decoded = QString::fromUtf8(QByteArray::fromBase64(t.toLatin1()));
        if (decoded.contains(u"://"))
            value = decoded;
    }
    if (Local::isServers(value))
        return {{QStringLiteral("kind"), QStringLiteral("servers")}, {QStringLiteral("text"), value}};
    if (!value.startsWith(u"http://", Qt::CaseInsensitive) && !value.startsWith(u"https://", Qt::CaseInsensitive))
        return {};
    const qsizetype hash = value.indexOf(u'#');
    const QString fragment = hash < 0 ? QString() : QUrl::fromPercentEncoding(value.mid(hash + 1).toUtf8());
    return {{QStringLiteral("kind"), QStringLiteral("sub")}, {QStringLiteral("url"), hash < 0 ? value : value.left(hash)}, {QStringLiteral("name"), name.isEmpty() ? fragment : name}};
}
