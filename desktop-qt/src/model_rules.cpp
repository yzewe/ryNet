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

QString Rules::actionTarget(const QString &action)
{
    if (action == u"proxy")
        return QStringLiteral("PROXY");
    if (action == u"direct")
        return QStringLiteral("DIRECT");
    if (action == u"block")
        return QStringLiteral("REJECT");
    return {};
}

std::optional<QString> Rules::domain(const QString &input)
{
    QString d = input.trimmed().toLower();
    d = substringAfter(d, QStringLiteral("://"));
    d = substringBefore(d, QStringLiteral("/"));
    d = substringBefore(d, QStringLiteral(":"));
    if (d.startsWith(u"*."))
        d = d.mid(2);
    if (d.startsWith(u"+."))
        d = d.mid(2);
    if (d.startsWith(u'.'))
        d = d.mid(1);
    static const QRegularExpression re(QStringLiteral("^([a-z0-9-]+\\.)*[a-z0-9-]+$"));
    if (!re.match(d).hasMatch())
        return {};
    return d;
}

std::optional<QPair<QString, bool>> Rules::cidr(const QString &input)
{
    const QString in = input.trimmed();
    const int slash = in.indexOf(u'/');
    const QString addr = slash < 0 ? in : in.left(slash);
    const bool v6 = addr.contains(u':');
    bool ok = false;
    if (v6) {
        static const QRegularExpression hex(QStringLiteral("^[0-9a-fA-F:.]+$"));
        ok = hex.match(addr).hasMatch() && QHostAddress(addr).protocol() == QAbstractSocket::IPv6Protocol;
    } else {
        const QStringList p = addr.split(u'.');
        ok = p.size() == 4 && std::all_of(p.begin(), p.end(), [](const QString &x) {
            const auto v = toIntStrict(x);
            return v && *v >= 0 && *v <= 255;
        });
    }
    if (!ok)
        return {};
    const int max = v6 ? 128 : 32;
    int m = max;
    if (slash >= 0) {
        const auto v = toIntStrict(in.mid(slash + 1));
        if (!v)
            return {};
        m = *v;
    }
    if (m < 0 || m > max)
        return {};
    return QPair<QString, bool>(addr + u'/' + QString::number(m), v6);
}

bool Rules::validProcess(const QString &s)
{
    if (s.isEmpty() || s.size() > 128)
        return false;
    for (const QChar c : s)
        if (c == u',' || c == u'\n' || c == u'\r' || c == u'#')
            return false;
    return true;
}

std::optional<QString> Rule::mihomo(const std::optional<QString> &target) const
{
    QString to;
    if (action == u"server") {
        if (!target)
            return {};
        to = *target;
    } else {
        to = Rules::actionTarget(action);
    }
    if (kind == u"domain") {
        const auto d = Rules::domain(value);
        if (!d)
            return {};
        return QStringLiteral("DOMAIN-SUFFIX,%1,%2").arg(*d, to);
    }
    if (kind == u"ip") {
        const auto c = Rules::cidr(value);
        if (!c)
            return {};
        return QStringLiteral("%1,%2,%3,no-resolve").arg(c->second ? QStringLiteral("IP-CIDR6") : QStringLiteral("IP-CIDR"), c->first, to);
    }
    if (kind == u"app") {
        const QString v = value.trimmed();
        if (!Rules::validProcess(v))
            return {};
        return QStringLiteral("PROCESS-NAME,%1,%2").arg(v, to);
    }
    return {};
}

bool Rule::valid() const
{
    return mihomo(action == u"server" && server ? std::optional<QString>(QStringLiteral("rynet-app0")) : std::nullopt).has_value();
}

QStringList RuleSet::mihomo(const std::optional<QString> &target) const
{
    QString to;
    if (action == u"server") {
        if (!target)
            return {};
        to = *target;
    } else {
        to = Rules::actionTarget(action);
    }
    QStringList out;
    auto add = [&](const QString &kind, const QStringList &values) {
        for (const QString &v : values) {
            Rule r{kind, v, action, server};
            if (const auto m = r.mihomo(target))
                out << *m;
        }
    };
    add(QStringLiteral("domain"), domains);
    add(QStringLiteral("app"), procs);
    add(QStringLiteral("ip"), ips);
    for (const QString &g : geosite)
        out << QStringLiteral("GEOSITE,%1,%2").arg(g, to);
    return out;
}

bool RuleSet::operator==(const RuleSet &o) const
{
    return id == o.id && name == o.name && shape == o.shape && icon == o.icon && action == o.action && server == o.server
        && enabled == o.enabled && domains == o.domains && procs == o.procs && ips == o.ips && geosite == o.geosite;
}

QList<QPair<QPair<QString, QString>, QString>> Rules::servers(const QList<Rule> &rules, const QList<RuleSet> &sets)
{
    QList<QPair<QString, QString>> keys;
    for (const Rule &r : rules)
        if (r.action == u"server" && r.server && !keys.contains(*r.server))
            keys << *r.server;
    for (const RuleSet &s : sets)
        if (s.enabled && s.action == u"server" && s.server && !keys.contains(*s.server))
            keys << *s.server;
    QList<QPair<QPair<QString, QString>, QString>> out;
    for (int i = 0; i < keys.size(); ++i)
        out.append({keys[i], QStringLiteral("rynet-app%1").arg(i)});
    return out;
}

QString Rules::mihomo(const QList<Rule> &rules, const QString &extra, const QList<RuleSet> &sets)
{
    QList<RuleSet> on;
    for (const RuleSet &s : sets)
        if (s.enabled)
            on << s;
    const auto tags = servers(rules, on);
    auto tagOf = [&](const Target &s) -> std::optional<QString> {
        if (!s)
            return {};
        for (const auto &kv : tags)
            if (kv.first == *s)
                return kv.second;
        return {};
    };
    QStringList out;
    for (const Rule &r : rules)
        if (const auto m = r.mihomo(tagOf(r.server)))
            out << *m;
    for (const RuleSet &s : on)
        out << s.mihomo(tagOf(s.server));
    for (const QString &l : lines(extra)) {
        const QString t = l.trimmed();
        if (!t.isEmpty())
            out << t;
    }
    return out.join(u'\n');
}

QJsonArray Rules::toJson(const QList<Rule> &rules)
{
    QJsonArray a;
    for (const Rule &r : rules) {
        QJsonObject o{{"kind", r.kind}, {"value", r.value}, {"action", r.action}};
        if (r.server) {
            o["sub"] = r.server->first;
            o["profile"] = r.server->second;
        }
        a.append(o);
    }
    return a;
}

QList<Rule> Rules::fromJson(const QJsonArray &a)
{
    QList<Rule> out;
    for (const QJsonValue &v : a) {
        if (!v.isObject())
            continue;
        const QJsonObject o = v.toObject();
        Rule r;
        r.kind = str(o, "kind");
        if (!kRuleKinds.contains(r.kind))
            continue;
        r.value = str(o, "value");
        if (r.value.trimmed().isEmpty())
            continue;
        r.action = keyOf(o, "action", kActions, "proxy");
        const QString sub = str(o, "sub"), profile = str(o, "profile");
        if (!sub.isEmpty() && !profile.isEmpty())
            r.server = QPair<QString, QString>(sub, profile);
        if (r.action == u"server" && !r.server)
            r.action = "proxy";
        out << r;
    }
    return out;
}

namespace {
constexpr int kSetMaxBytes = 512 * 1024;
constexpr int kSetMaxItems = 20000;
const QString kSetLink = QStringLiteral("rynet://set/");

QJsonObject setBody(const RuleSet &s)
{
    return QJsonObject{{"name", s.name},        {"shape", s.shape},       {"icon", s.icon},       {"action", s.action},
                       {"domains", arr(s.domains)}, {"procs", arr(s.procs)}, {"ips", arr(s.ips)}, {"geosite", arr(s.geosite)}};
}

RuleSet readSet(const QJsonObject &o)
{
    auto list = [&](const char *key, const std::function<std::optional<QString>(const QString &)> &norm) {
        QStringList out;
        const QJsonArray a = o.value(QLatin1String(key)).toArray();
        const int n = std::min<int>(a.size(), kSetMaxItems);
        for (int i = 0; i < n; ++i) {
            const QJsonValue v = a.at(i);
            const QString s = v.isString() ? v.toString() : v.isDouble() ? QString::number(v.toDouble()) : QString();
            if (const auto x = norm(s))
                out << *x;
        }
        return uniq(out);
    };
    RuleSet s;
    s.name = str(o, "name").trimmed().left(40);
    s.shape = std::max(0, intOf(o, "shape", 0));
    s.icon = std::max(0, intOf(o, "icon", 0));
    s.action = keyOf(o, "action", kActions, "proxy");
    s.domains = list("domains", [](const QString &v) { return Rules::domain(v); });
    s.procs = list("procs", [](const QString &v) -> std::optional<QString> {
        const QString t = v.trimmed();
        return Rules::validProcess(t) ? std::optional<QString>(t) : std::nullopt;
    });
    s.ips = list("ips", [](const QString &v) -> std::optional<QString> {
        const auto c = Rules::cidr(v);
        return c ? std::optional<QString>(c->first) : std::nullopt;
    });
    s.geosite = list("geosite", [](const QString &v) { return RuleSets::geosite(v); });
    return s;
}

qint32 javaHash(const QString &s)
{
    quint32 h = 0;
    for (const QChar c : s)
        h = 31 * h + c.unicode();
    return qint32(h);
}
}

QVariantMap SubscriptionRules::parse(const std::optional<QString> &raw)
{
    if (!raw)
        return {};
    const QJsonDocument doc = QJsonDocument::fromJson(raw->toUtf8());
    if (!doc.isObject())
        return {};
    const QJsonObject o = doc.object();
    auto field = [&](const QString &key) -> QJsonValue {
        if (o.contains(key))
            return o.value(key);
        for (auto it = o.begin(); it != o.end(); ++it)
            if (it.key().compare(key, Qt::CaseInsensitive) == 0)
                return it.value();
        return QJsonValue();
    };
    auto list = [&](const QString &key) {
        QStringList out;
        for (const QJsonValue &v : field(key).toArray())
            if (v.isString() && !v.toString().trimmed().isEmpty())
                out << v.toString().trimmed();
        return out;
    };
    const QStringList defaults{QStringLiteral("block"), QStringLiteral("proxy"), QStringLiteral("direct")};
    QStringList order;
    for (const QString &key : field(QStringLiteral("RouteOrder")).toString().trimmed().toLower().split(QRegularExpression(QStringLiteral("[-, ]"))))
        if (defaults.contains(key) && !order.contains(key))
            order << key;
    for (const QString &key : defaults)
        if (!order.contains(key))
            order << key;
    QVariantList groups;
    int count = 0;
    for (const QString &key : order) {
        QString prefix = key;
        prefix[0] = prefix[0].toUpper();
        const QStringList domains = list(prefix + QStringLiteral("Sites")), ips = list(prefix + QStringLiteral("Ip"));
        const int n = int(domains.size() + ips.size());
        if (n) {
            groups << QVariantMap{{"action", key}, {"domains", domains}, {"ips", ips}, {"size", n}};
            count += n;
        }
    }
    const QJsonValue gp = field(QStringLiteral("GlobalProxy"));
    const bool global = gp.isBool() ? gp.toBool() : !gp.isString() || gp.toString().trimmed().compare(QStringLiteral("false"), Qt::CaseInsensitive) != 0;
    return {{"name", field(QStringLiteral("Name")).toString().trimmed()}, {"groups", groups}, {"size", count},
            {"rest", global ? QStringLiteral("proxy") : QStringLiteral("direct")}, {"raw", QString::fromUtf8(doc.toJson(QJsonDocument::Indented))}};
}

QVariantMap ModeRules::entry(const QString &kind, const QString &value, const QString &action)
{
    static const QHash<QString, QString> prefix{{"suffix", "domain:"}, {"full", "full:"}, {"keyword", "keyword:"}, {"regex", "regexp:"},
                                                {"geosite", "geosite:"}, {"geoip", "geoip:"}, {"protocol", "protocol:"}};
    return {{"kind", kind}, {"value", value}, {"action", action}, {"text", prefix.value(kind) + value}};
}

QVariantMap ModeRules::domain(const QString &raw, const QString &action)
{
    const QString v = raw.trimmed();
    const qsizetype colon = v.indexOf(u':');
    const QString p = colon >= 0 ? v.left(colon).toLower() : QString();
    const QString rest = colon >= 0 ? v.mid(colon + 1) : v;
    if (p == u"geosite")
        return entry("geosite", rest.toLower(), action);
    if (p == u"full")
        return entry("full", rest.toLower(), action);
    if (p == u"keyword")
        return entry("keyword", rest.toLower(), action);
    if (p == u"regexp")
        return entry("regex", rest, action);
    return entry("suffix", (p == u"domain" ? rest : v).toLower(), action);
}

QVariantMap ModeRules::ip(const QString &raw, const QString &action)
{
    const QString v = raw.trimmed();
    if (v.startsWith(QStringLiteral("geoip:"), Qt::CaseInsensitive)) {
        QString name = v.mid(6).toLower();
        if (name.startsWith(u'!'))
            name.remove(0, 1);
        return entry("geoip", name, action);
    }
    return entry("cidr", v, action);
}

QVariantList ModeRules::fromXray(const QJsonObject &o)
{
    QVariantList out;
    for (const QJsonValue &rv : o.value("rules").toArray()) {
        const QJsonObject r = rv.toObject();
        const QString tag = r.value("outboundTag").toString();
        const QString action = tag == u"direct" || tag == u"block" ? tag : QStringLiteral("proxy");
        for (const QJsonValue &v : r.value("protocol").toArray())
            out << entry("protocol", v.toString(), action);
        for (const QJsonValue &v : r.value("ip").toArray())
            out << ip(v.toString(), action);
        for (const QJsonValue &v : r.value("domain").toArray())
            out << domain(v.toString(), action);
    }
    return out;
}

QVariantMap ModeRules::fromConfig(const QString &body, const QString &remarks)
{
    const QByteArray text = body.trimmed().toUtf8();
    if (!text.startsWith('[') && !text.startsWith('{'))
        return {};
    const QJsonDocument doc = QJsonDocument::fromJson(text);
    QList<QJsonObject> configs;
    if (doc.isArray())
        for (const QJsonValue &v : doc.array())
            configs << v.toObject();
    else if (doc.isObject())
        configs << doc.object();
    QJsonObject config;
    bool found = false;
    for (const QJsonObject &c : configs)
        if (remarks.isEmpty() || c.value("remarks").toString() == remarks) {
            config = c;
            found = true;
            break;
        }
    const QJsonArray rules = config.value("routing").toObject().value("rules").toArray();
    if (!found || rules.isEmpty())
        return {};
    QHash<QString, QString> kinds;
    for (const QJsonValue &o : config.value("outbounds").toArray())
        kinds.insert(o.toObject().value("tag").toString(), o.toObject().value("protocol").toString());
    auto list = [](const QJsonValue &v) {
        QStringList out;
        if (v.isArray())
            for (const QJsonValue &x : v.toArray())
                out << x.toString();
        else if (v.isString())
            for (const QString &x : v.toString().split(u',', Qt::SkipEmptyParts))
                out << x.trimmed();
        return out;
    };
    QVariantList entries;
    QString rest;
    for (const QJsonValue &rv : rules) {
        const QJsonObject r = rv.toObject();
        if (r.contains("inboundTag") && !r.contains("domain") && !r.contains("ip"))
            continue;
        const QString tag = r.value("outboundTag").toString();
        const QString kind = kinds.value(tag);
        const QString action = kind == u"freedom" ? QStringLiteral("direct") : kind == u"blackhole" ? QStringLiteral("block")
            : kind.isEmpty() && (tag == u"direct" || tag == u"block") ? tag : QStringLiteral("proxy");
        const QStringList protocols = list(r.value("protocol")), ips = list(r.value("ip")), domains = list(r.value("domain"));
        for (const QString &v : protocols)
            entries << entry("protocol", v, action);
        for (const QString &v : ips)
            entries << ip(v, action);
        for (const QString &v : domains)
            entries << domain(v, action);
        if (protocols.isEmpty() && ips.isEmpty() && domains.isEmpty() && !r.contains("port") && rest.isEmpty())
            rest = action;
    }
    return {{"entries", entries}, {"rest", rest}};
}

QVariantList ModeRules::subscription(const QVariantMap &parsed)
{
    QVariantList out;
    for (const QVariant &gv : parsed.value("groups").toList()) {
        const QVariantMap g = gv.toMap();
        const QString action = g.value("action").toString();
        for (const QString &d : g.value("domains").toStringList())
            out << domain(d, action);
        for (const QString &i : g.value("ips").toStringList())
            out << ip(i, action);
    }
    return out;
}

QString RuleSets::newId() { return QUuid::createUuid().toString(QUuid::WithoutBraces).left(8); }

QJsonArray RuleSets::toJson(const QList<RuleSet> &sets)
{
    QJsonArray a;
    for (const RuleSet &s : sets) {
        QJsonObject o = setBody(s);
        o["id"] = s.id;
        o["enabled"] = s.enabled;
        if (s.server) {
            o["sub"] = s.server->first;
            o["profile"] = s.server->second;
        }
        a.append(o);
    }
    return a;
}

QList<RuleSet> RuleSets::fromJson(const QJsonArray &a)
{
    QList<RuleSet> out;
    for (const QJsonValue &v : a) {
        if (!v.isObject())
            continue;
        const QJsonObject o = v.toObject();
        RuleSet s = readSet(o);
        s.id = str(o, "id");
        if (s.id.isEmpty())
            s.id = newId();
        s.enabled = boolOf(o, "enabled", true);
        const QString sub = str(o, "sub"), profile = str(o, "profile");
        if (!sub.isEmpty() && !profile.isEmpty())
            s.server = QPair<QString, QString>(sub, profile);
        if (s.action == u"server" && !s.server)
            s.action = "proxy";
        out << s;
    }
    return out;
}

QString RuleSets::link(const RuleSet &s)
{
    QJsonObject o = setBody(s);
    o["v"] = 1;
    const QByteArray packed = Zlib::deflateRaw(QJsonDocument(o).toJson(QJsonDocument::Compact));
    return kSetLink + QString::fromLatin1(packed.toBase64(QByteArray::Base64UrlEncoding | QByteArray::OmitTrailingEquals));
}

bool RuleSets::isLink(const QString &text) { return text.trimmed().startsWith(kSetLink, Qt::CaseInsensitive); }

QStringList RuleSets::parts(const QString &link, int size)
{
    QString body = link.trimmed();
    if (body.startsWith(kSetLink))
        body = body.mid(kSetLink.size());
    QStringList chunks;
    for (int i = 0; i < body.size(); i += size)
        chunks << body.mid(i, size);
    const QString tag = QString::number(quint32(javaHash(body)), 36);
    QStringList out;
    for (int i = 0; i < chunks.size(); ++i)
        out << QStringLiteral("%1p/%2/%3/%4/%5").arg(kSetLink, tag).arg(i + 1).arg(chunks.size()).arg(chunks[i]);
    return out;
}

std::optional<RuleSet> RuleSets::parse(const QString &text)
{
    const QString t = text.trimmed();
    if (!isLink(t))
        return {};
    QString payload = t.mid(kSetLink.size());
    payload = substringBefore(payload, QStringLiteral("#"));
    payload = substringBefore(payload, QStringLiteral("?"));
    const auto decoded = QByteArray::fromBase64Encoding(payload.toLatin1(), QByteArray::Base64UrlEncoding | QByteArray::AbortOnBase64DecodingErrors);
    if (!decoded || decoded.decoded.isEmpty())
        return {};
    const auto raw = Zlib::inflateRaw(decoded.decoded, kSetMaxBytes);
    if (!raw)
        return {};
    QJsonParseError err;
    const QJsonDocument doc = QJsonDocument::fromJson(*raw, &err);
    if (err.error != QJsonParseError::NoError || !doc.isObject())
        return {};
    RuleSet s = readSet(doc.object());
    s.id = newId();
    if (s.action == u"server")
        s.action = "proxy";
    return s;
}

std::optional<QString> RuleSets::geosite(const QString &input)
{
    QString g = input.trimmed().toLower();
    if (g.startsWith(u"geosite:"))
        g = g.mid(8);
    static const QRegularExpression re(QStringLiteral("^[a-z0-9!@._-]+$"));
    if (!re.match(g).hasMatch())
        return {};
    return g;
}

QStringList RuleSets::split(const QString &text, bool byLines)
{
    static const QRegularExpression lineRe(QStringLiteral("[\\r\\n]+"));
    static const QRegularExpression wordRe(QStringLiteral("[\\s,;]+"));
    QStringList out;
    for (const QString &p : text.split(byLines ? lineRe : wordRe)) {
        const QString t = p.trimmed();
        if (!t.isEmpty())
            out << t;
    }
    return out;
}

QVariantMap RuleSets::toVariant(const RuleSet &s)
{
    return {{"id", s.id},
            {"name", s.name},
            {"shape", s.shape},
            {"icon", s.icon},
            {"action", s.action},
            {"sub", s.server ? s.server->first : QString()},
            {"profile", s.server ? s.server->second : QString()},
            {"enabled", s.enabled},
            {"domains", s.domains},
            {"procs", s.procs},
            {"ips", s.ips},
            {"geosite", s.geosite},
            {"size", s.size()}};
}

RuleSet RuleSets::fromVariant(const QVariantMap &m)
{
    RuleSet s;
    s.id = m.value("id").toString();
    s.name = m.value("name").toString().trimmed().left(40);
    s.shape = std::max(0, m.value("shape").toInt());
    s.icon = std::max(0, m.value("icon").toInt());
    s.action = kActions.contains(m.value("action").toString()) ? m.value("action").toString() : QStringLiteral("proxy");
    const QString sub = m.value("sub").toString(), profile = m.value("profile").toString();
    if (!sub.isEmpty() && !profile.isEmpty())
        s.server = QPair<QString, QString>(sub, profile);
    if (s.action == u"server" && !s.server)
        s.action = "proxy";
    s.enabled = m.value("enabled", true).toBool();
    s.domains = m.value("domains").toStringList();
    s.procs = m.value("procs").toStringList();
    s.ips = m.value("ips").toStringList();
    s.geosite = m.value("geosite").toStringList();
    return s;
}
