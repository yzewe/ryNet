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
bool g_ru = true;
}

bool Lang::ru() { return g_ru; }
void Lang::setRu(bool on) { g_ru = on; }

QString plural(int n, const char *one, const char *few, const char *many, const char *en, const char *enMany)
{
    if (!Lang::ru()) {
        const QString e = QString::fromUtf8(en);
        return QStringLiteral("%1 %2").arg(n).arg(n == 1 ? e : (enMany ? QString::fromUtf8(enMany) : e + u's'));
    }
    const int m10 = n % 10, m100 = n % 100;
    const char *w = (m10 == 1 && m100 != 11) ? one : (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) ? few : many;
    return QStringLiteral("%1 %2").arg(n).arg(QString::fromUtf8(w));
}

qint64 nowSeconds() { return QDateTime::currentSecsSinceEpoch(); }
qint64 nowMillis() { return QDateTime::currentMSecsSinceEpoch(); }

const Fragment::Preset Fragment::presets[3] = {
    {"Лёгкая", "Light", "100-200", "10-20"},
    {"Средняя", "Medium", "30-60", "10-30"},
    {"Сильная", "Strong", "5-20", "20-50"},
};

int Fragment::of(const QString &length, const QString &interval)
{
    for (int i = 0; i < 3; ++i)
        if (length == QLatin1String(presets[i].length) && interval == QLatin1String(presets[i].interval))
            return i;
    return -1;
}

bool Fragment::validRange(const QString &v, int max)
{
    const QStringList parts = v.trimmed().split(u'-');
    if (parts.isEmpty() || parts.size() > 2)
        return false;
    QList<int> n;
    for (const QString &p : parts) {
        const auto x = toIntStrict(p.trimmed());
        if (!x)
            return false;
        n << *x;
    }
    const int a = n.first(), b = n.last();
    return a >= 1 && a <= max && b >= a && b <= max;
}

bool validDns(const QString &address)
{
    static const QRegularExpression re(
        QStringLiteral("^(localhost|fakedns|[0-9a-f.:\\[\\]]+|(udp|tcp|tls|https|quic)(\\+local)?://[^\\s/]+(/\\S*)?)$"),
        QRegularExpression::CaseInsensitiveOption);
    return re.match(address).hasMatch();
}

Target Settings::chainEntry() const
{
    if (chain && !chainSub.isEmpty() && !chainProfile.isEmpty())
        return QPair<QString, QString>(chainSub, chainProfile);
    return {};
}

static QStringList splitDns(const QString &s)
{
    static const QRegularExpression re(QStringLiteral("[\\s,]+"));
    QStringList out;
    for (const QString &p : s.split(re))
        if (!p.isEmpty())
            out << p;
    return out;
}

QStringList Settings::ownDns() const { return splitDns(dnsServers); }

QString Settings::serverDnsArg() const
{
    if (serverDns != u"custom")
        return serverDns;
    const QStringList list = splitDns(serverDnsCustom);
    return list.isEmpty() ? QStringLiteral("auto") : u"custom:" + list.join(u' ');
}

QStringList Settings::dnsFor(const std::optional<QString> &s, const std::optional<QString> &p) const
{
    if (s && p) {
        const auto it = perServerDns.constFind(*s);
        if (it != perServerDns.constEnd()) {
            const QStringList own = splitDns(it->value(*p));
            if (!own.isEmpty())
                return own;
        }
    }
    return ownDns();
}

QStringList Settings::pinned(const std::optional<QString> &s) const { return s ? pins.value(*s) : QStringList(); }

Settings Settings::pin(const QString &s, const QString &remarks, bool on) const
{
    Settings next = *this;
    QStringList now = pinned(s);
    now.removeAll(remarks);
    if (on)
        now << remarks;
    if (now.isEmpty())
        next.pins.remove(s);
    else
        next.pins[s] = now;
    return next;
}

bool Settings::autoWlMarked() const
{
    return !autoWlSubs.isEmpty() || std::any_of(autoWl.begin(), autoWl.end(), [](const QStringList &l) { return !l.isEmpty(); });
}

bool Settings::isAutoWl(const QString &s, const QString &remarks) const
{
    return autoWlSubs.contains(s) || autoWl.value(s).contains(remarks);
}

Settings Settings::serverDnsFor(const QString &s, const QString &remarks, const QString &dns) const
{
    Settings next = *this;
    QMap<QString, QString> now = perServerDns.value(s);
    if (dns.trimmed().isEmpty())
        now.remove(remarks);
    else
        now[remarks] = dns.trimmed();
    if (now.isEmpty())
        next.perServerDns.remove(s);
    else
        next.perServerDns[s] = now;
    return next;
}

QJsonObject Settings::toJson() const
{
    QJsonObject o;
    o["routingMode"] = routing;
    o["pingMode"] = pingMode;
    o["pingOnStart"] = pingOnStart;
    o["sort"] = sort;
    o["view"] = view;
    o["grid"] = grid;
    o["serverInfo"] = serverInfo;
    o["exitFlags"] = exitFlags;
    o["pingParallel"] = pingParallel;
    o["pingTimeout"] = pingTimeout;
    o["pingUrl"] = pingUrl;
    o["fakeDns"] = fakeDns;
    o["ipv4Only"] = ipv4Only;
    o["ipUrl"] = ipUrl;
    o["serverDns"] = serverDns;
    o["serverDnsCustom"] = serverDnsCustom;
    o["dnsServers"] = dnsServers;
    o["hwid"] = hwid;
    QJsonObject p;
    for (auto it = pins.begin(); it != pins.end(); ++it)
        p[it.key()] = arr(it.value());
    o["pins"] = p;
    QJsonObject d;
    for (auto it = perServerDns.begin(); it != perServerDns.end(); ++it) {
        QJsonObject m;
        for (auto j = it->begin(); j != it->end(); ++j)
            m[j.key()] = j.value();
        d[it.key()] = m;
    }
    o["perServerDns"] = d;
    QJsonObject last;
    for (auto it = lastServers.begin(); it != lastServers.end(); ++it)
        last[it.key()] = it.value();
    o["lastServers"] = last;
    o["sendHwid"] = sendHwid;
    o["refreshNotices"] = refreshNotices;
    o["autoUpdate"] = autoUpdate;
    o["subHeaders"] = subHeaders;
    o["subClient"] = subClient;
    o["userRules"] = userRules;
    o["rules"] = Rules::toJson(rules);
    o["sets"] = RuleSets::toJson(sets);
    o["autostart"] = autostart;
    o["theme"] = theme;
    o["font"] = font;
    o["wallpaperColors"] = wallpaperColors;
    o["seedColor"] = seedColor;
    o["debugLog"] = debugLog;
    o["healthCheck"] = healthCheck;
    o["healthSec"] = healthSec;
    o["healthNotify"] = healthNotify;
    o["autoSwitch"] = autoSwitch;
    o["autoSelect"] = autoSelect;
    o["autoFeature"] = autoFeature;
    o["splitSubs"] = arr(splitSubs);
    o["dismissedNotices"] = arr(dismissedNotices);
    o["autoSkipSubs"] = arr(autoSkipSubs);
    o["autoSkip"] = listMap(autoSkip);
    o["autoWlSubs"] = arr(autoWlSubs);
    o["autoWl"] = listMap(autoWl);
    o["autoPins"] = autoPins;
    o["autoTransports"] = arr(autoTransports);
    o["autoWhitelist"] = autoWhitelist;
    o["autoHome"] = autoHome;
    o["autoFree"] = autoFree;
    o["autoInterval"] = autoInterval;
    o["autoMargin"] = autoMargin;
    o["autoSpeed"] = autoSpeed;
    o["autoSaveData"] = autoSaveData;
    o["autoNotify"] = autoNotify;
    o["reserveOnlyOnWhitelists"] = reserveOnlyOnWhitelists;
    o["sendLogs"] = sendLogs;
    o["updateCheck"] = updateCheck;
    o["killSwitch"] = killSwitch;
    o["processes"] = processes;
    o["processesMode"] = processesMode;
    o["directPorts"] = directPorts;
    o["notifications"] = notifications;
    o["mtu"] = mtu;
    o["fragment"] = fragment;
    o["fragmentLength"] = fragmentLength;
    o["fragmentInterval"] = fragmentInterval;
    o["chain"] = chain;
    o["chainSub"] = chainSub;
    o["chainProfile"] = chainProfile;
    o["connMode"] = connMode;
    o["port"] = port;
    o["systemProxy"] = systemProxy;
    o["uiScale"] = uiScale;
    o["sectionRadius"] = sectionRadius;
    o["sectionDivider"] = sectionDivider;
    if (sub)
        o["sub"] = *sub;
    if (profile)
        o["profile"] = *profile;
    return o;
}

Settings Settings::from(const QJsonObject &o)
{
    const Settings d;
    Settings s;
    s.routing = keyOf(o, o.contains("routingMode") ? "routingMode" : "routing", {"smart", "subscription", "global", "direct"}, d.routing);
    s.pingMode = keyOf(o, "pingMode", {"tcp", "icmp", "proxy", "proxy2"}, d.pingMode);
    s.pingOnStart = o.value("pingOnStart").toBool(false);
    s.sort = keyOf(o, "sort", {"default", "ping", "name"}, d.sort);
    s.view = keyOf(o, "view", {"groups", "single"}, d.view);
    s.grid = keyOf(o, "grid", {"2x1", "1x1"}, d.grid);
    s.serverInfo = keyOf(o, "serverInfo", {"description", "protocol", "none"}, d.serverInfo);
    s.exitFlags = boolOf(o, "exitFlags", d.exitFlags);
    s.pingParallel = intOf(o, "pingParallel", d.pingParallel);
    s.pingTimeout = intOf(o, "pingTimeout", d.pingTimeout);
    s.pingUrl = str(o, "pingUrl");
    s.fakeDns = boolOf(o, "fakeDns", d.fakeDns);
    s.ipv4Only = boolOf(o, "ipv4Only", d.ipv4Only);
    s.ipUrl = str(o, "ipUrl").trimmed();
    s.serverDns = keyOf(o, "serverDns", {"auto", "cloudflare", "google", "system", "custom"}, d.serverDns);
    s.serverDnsCustom = str(o, "serverDnsCustom").trimmed();
    s.dnsServers = str(o, "dnsServers");
    const QJsonObject pins = o.value("pins").toObject();
    for (auto it = pins.begin(); it != pins.end(); ++it) {
        QStringList l;
        for (const QJsonValue &v : it.value().toArray())
            if (v.isString() && !v.toString().isEmpty())
                l << v.toString();
        if (!l.isEmpty())
            s.pins[it.key()] = l;
    }
    const QJsonObject dns = o.value("perServerDns").toObject();
    for (auto it = dns.begin(); it != dns.end(); ++it) {
        QMap<QString, QString> m;
        const QJsonObject inner = it.value().toObject();
        for (auto j = inner.begin(); j != inner.end(); ++j)
            if (j.value().isString() && !j.value().toString().trimmed().isEmpty())
                m[j.key()] = j.value().toString();
        if (!m.isEmpty())
            s.perServerDns[it.key()] = m;
    }
    const QJsonObject last = o.value("lastServers").toObject();
    for (auto it = last.begin(); it != last.end(); ++it)
        if (it.value().isString() && !it.value().toString().isEmpty())
            s.lastServers[it.key()] = it.value().toString();
    s.hwid = str(o, "hwid");
    s.autoUpdate = boolOf(o, "autoUpdate", d.autoUpdate);
    s.sendHwid = boolOf(o, "sendHwid", d.sendHwid);
    s.refreshNotices = boolOf(o, "refreshNotices", d.refreshNotices);
    s.subClient = keyOf(o, "subClient", {"rynet", "happ", "custom"}, d.subClient);
    s.subHeaders = str(o, "subHeaders");
    s.rules = Rules::fromJson(o.value("rules").toArray());
    for (const QJsonValue &v : o.value("appServers").toArray()) {
        const QJsonObject a = v.toObject();
        Rule r{QStringLiteral("app"), str(a, "app"), QStringLiteral("server"), QPair<QString, QString>(str(a, "sub"), str(a, "profile"))};
        if (r.value.isEmpty() || r.server->first.isEmpty() || r.server->second.isEmpty())
            continue;
        const bool dup = std::any_of(s.rules.begin(), s.rules.end(), [&](const Rule &x) { return x.kind == u"app" && x.value == r.value; });
        if (!dup)
            s.rules << r;
    }
    s.sets = RuleSets::fromJson(o.value("sets").toArray());
    s.userRules = str(o, "userRules");
    s.autostart = boolOf(o, "autostart", d.autostart);
    s.theme = keyOf(o, "theme", {"system", "light", "dark", "amoled"}, d.theme);
    s.font = keyOf(o, "font", {"roboto", "system"}, d.font);
    s.wallpaperColors = boolOf(o, "wallpaperColors", d.wallpaperColors);
    static const QRegularExpression rgbHex(QStringLiteral("^#[0-9a-fA-F]{6}$"));
    if (const QString seed = str(o, "seedColor"); rgbHex.match(seed).hasMatch())
        s.seedColor = seed.toLower();
    s.debugLog = boolOf(o, "debugLog", d.debugLog);
    s.healthCheck = boolOf(o, "healthCheck", d.healthCheck);
    const int health = intOf(o, "healthSec", intOf(o, "healthInterval", d.healthSec / 60) * 60);
    s.healthSec = *std::min_element(std::begin(kHealthSteps), std::end(kHealthSteps), [health](int a, int b) { return std::abs(a - health) < std::abs(b - health); });
    s.healthNotify = boolOf(o, "healthNotify", d.healthNotify);
    s.autoSwitch = boolOf(o, "autoSwitch", d.autoSwitch);
    s.autoSelect = boolOf(o, "autoSelect", d.autoSelect);
    s.autoFeature = boolOf(o, "autoFeature", s.autoSelect);
    s.splitSubs = strings(o.value("splitSubs"));
    s.dismissedNotices = strings(o.value("dismissedNotices"));
    s.autoSkipSubs = strings(o.value("autoSkipSubs"));
    s.autoSkip = listMapOf(o.value("autoSkip"));
    s.autoWlSubs = strings(o.value("autoWlSubs"));
    s.autoWl = listMapOf(o.value("autoWl"));
    s.autoPins = boolOf(o, "autoPins", d.autoPins);
    s.autoTransports = strings(o.value("autoTransports"));
    s.autoWhitelist = keyOf(o, "autoWhitelist", {"auto", "always", "never"}, d.autoWhitelist);
    s.autoHome = str(o, "autoHome").trimmed().isEmpty() ? d.autoHome : str(o, "autoHome");
    s.autoFree = str(o, "autoFree").trimmed().isEmpty() ? d.autoFree : str(o, "autoFree");
    const int interval = intOf(o, "autoInterval", d.autoInterval);
    s.autoInterval = interval < 0 ? d.autoInterval : std::min(interval, 60);
    s.autoMargin = std::clamp(intOf(o, "autoMargin", d.autoMargin), 0, 1000);
    s.autoSpeed = boolOf(o, "autoSpeed", d.autoSpeed);
    s.autoSaveData = boolOf(o, "autoSaveData", d.autoSaveData);
    s.autoNotify = boolOf(o, "autoNotify", d.autoNotify);
    s.reserveOnlyOnWhitelists = boolOf(o, "reserveOnlyOnWhitelists", d.reserveOnlyOnWhitelists);
    s.sendLogs = boolOf(o, "sendLogs", d.sendLogs);
    s.updateCheck = boolOf(o, "updateCheck", d.updateCheck);
    s.killSwitch = boolOf(o, "killSwitch", d.killSwitch);
    s.processes = str(o, "processes");
    s.processesMode = keyOf(o, "processesMode", {"exclude", "include"}, d.processesMode);
    s.directPorts = str(o, "directPorts");
    s.notifications = boolOf(o, "notifications", d.notifications);
    s.mtu = intOf(o, "mtu", d.mtu);
    s.fragment = boolOf(o, "fragment", d.fragment);
    s.fragmentLength = o.contains("fragmentLength") ? str(o, "fragmentLength") : d.fragmentLength;
    s.fragmentInterval = o.contains("fragmentInterval") ? str(o, "fragmentInterval") : d.fragmentInterval;
    s.chain = boolOf(o, "chain", d.chain);
    s.chainSub = str(o, "chainSub");
    s.chainProfile = str(o, "chainProfile");
    s.connMode = keyOf(o, "connMode", {"tun", "proxy"}, d.connMode);
    s.port = intOf(o, "port", d.port);
    s.systemProxy = boolOf(o, "systemProxy", d.systemProxy);
    s.uiScale = intOf(o, "uiScale", d.uiScale);
    if (s.uiScale == 100 || s.uiScale < 50)
        s.uiScale = 0;
    s.uiScale = std::min(s.uiScale, 200);
    s.sectionRadius = std::clamp(intOf(o, "sectionRadius", d.sectionRadius), 0, 32);
    s.sectionDivider = keyOf(o, "sectionDivider", {"gap", "none"}, d.sectionDivider);
    if (const auto v = text(o, "sub"))
        s.sub = *v;
    if (const auto v = text(o, "profile"))
        s.profile = *v;
    return s;
}

QString Sub::name() const
{
    if (customName)
        return *customName;
    if (title)
        return *title;
    return substringBefore(substringAfter(url, QStringLiteral("://")), QStringLiteral("/"));
}

QJsonObject Sub::toJson() const
{
    QJsonObject o{{"id", id}, {"url", url}, {"upload", upload}, {"download", download}, {"total", total}, {"expire", expire}, {"updated", updated}};
    if (title)
        o["title"] = *title;
    if (customName)
        o["customName"] = *customName;
    if (intervalHours)
        o["intervalHours"] = *intervalHours;
    if (announce)
        o["announce"] = *announce;
    if (routing)
        o["routing"] = *routing;
    if (supportUrl)
        o["supportUrl"] = *supportUrl;
    if (webPage)
        o["webPage"] = *webPage;
    return o;
}

Sub Sub::from(const QJsonObject &o)
{
    if (!o.value("id").isString() || !o.value("url").isString())
        throw Fail(QStringLiteral("subscription without id or url"));
    Sub s;
    s.id = o.value("id").toString();
    s.url = o.value("url").toString();
    s.title = text(o, "title");
    if (const auto custom = text(o, "customName"))
        s.customName = custom->trimmed();
    s.upload = longOf(o, "upload");
    s.download = longOf(o, "download");
    s.total = longOf(o, "total");
    s.expire = longOf(o, "expire");
    s.updated = longOf(o, "updated");
    if (o.contains("intervalHours") && !o.value("intervalHours").isNull())
        s.intervalHours = intOf(o, "intervalHours", 0);
    s.announce = text(o, "announce");
    s.routing = text(o, "routing");
    const QString support = str(o, "supportUrl"), web = str(o, "webPage");
    if (support.startsWith(u"https://") || support.startsWith(u"http://"))
        s.supportUrl = support;
    if (web.startsWith(u"https://") || web.startsWith(u"http://"))
        s.webPage = web;
    return s;
}

QVariantMap Sub::toVariant() const
{
    QString letter = QStringLiteral("•");
    for (const QChar c : name())
        if (c.isLetterOrNumber()) {
            letter = c.toUpper();
            break;
        }
    const QString host = substringBefore(substringAfter(url, QStringLiteral("://")), QStringLiteral("/"));
    return {{"id", id},
            {"url", url},
            {"host", host},
            {"name", name()},
            {"letter", letter},
            {"title", title.value_or(QString())},
            {"customName", customName.value_or(QString())},
            {"panelName", title.value_or(host)},
            {"upload", upload},
            {"download", download},
            {"used", upload + download},
            {"total", total},
            {"expire", expire},
            {"updated", updated},
            {"intervalHours", intervalHours ? QVariant(*intervalHours) : QVariant()},
            {"announce", announce.value_or(QString())},
            {"supportUrl", supportUrl.value_or(QString())},
            {"webPage", webPage.value_or(QString())},
            {"local", Local::isLocal(*this)}};
}

QPair<QString, QString> Server::splitFlag(const QString &s)
{
    const QString t = s.trimmed();
    const QList<uint> cps = t.toUcs4();
    auto ri = [](uint c) { return c >= 0x1F1E6 && c <= 0x1F1FF; };
    if (cps.size() >= 2 && ri(cps[0]) && ri(cps[1])) {
        const QString flag = QString::fromUcs4(reinterpret_cast<const char32_t *>(cps.constData()), 2);
        return {flag, t.mid(flag.size()).trimmed()};
    }
    return {QString(), t};
}

Server Server::from(const QJsonObject &o)
{
    Server p;
    p.remarks = str(o, "remarks");
    const auto [flag, full] = splitFlag(p.remarks);
    if (p.remarks.contains(u"Автовыбор"))
        p.group = 0;
    else if (p.remarks.contains(u"Для игр"))
        p.group = 3;
    else if (p.remarks.contains(u"Белые списки"))
        p.group = 4;
    else if (p.remarks.contains(u"Каскад"))
        p.group = 5;
    else if (p.remarks.contains(u"Резерв"))
        p.group = 2;
    else
        p.group = 1;
    static const QRegularExpression tag(QStringLiteral("\\s*\\[(Резерв|Для игр|Каскад)]"));
    p.name = QString(full).replace(tag, QString()).trimmed();
    if (p.name.isEmpty())
        p.name = p.remarks.trimmed();
    p.flag = flag;
    p.kind = str(o, "kind");
    p.supported = boolOf(o, "supported", false);
    p.description = text(o, "description").value_or(QString());
    p.placeholder = boolOf(o, "placeholder", false);
    p.reason = text(o, "reason").value_or(QString());
    p.server = text(o, "server").value_or(QString());
    p.sni = text(o, "sni").value_or(QString());
    p.fingerprint = text(o, "fp").value_or(QString());
    p.link = text(o, "link").value_or(QString());
    for (const QJsonValue &v : o.value("members").toArray()) {
        const QJsonObject m = v.toObject();
        const QString memberTag = str(m, "tag");
        const auto [mflag, mname] = splitFlag(memberTag);
        Server s;
        s.remarks = str(m, "name");
        s.name = mname.isEmpty() ? memberTag : mname;
        s.flag = mflag.isEmpty() ? p.flag : mflag;
        s.group = p.group;
        s.kind = str(m, "kind");
        s.supported = boolOf(m, "supported", false);
        s.reason = text(m, "reason").value_or(QString());
        s.server = text(m, "server").value_or(QString());
        s.sni = text(m, "sni").value_or(QString());
        s.fingerprint = text(m, "fp").value_or(QString());
        s.link = text(m, "link").value_or(QString());
        p.members << s;
    }
    return p;
}

QVariantMap Server::toVariant() const
{
    QVariantList list;
    for (const Server &m : members)
        list << m.toVariant();
    return {{"remarks", remarks},     {"name", name},         {"flag", flag},           {"group", group},
            {"kind", kind},           {"supported", supported}, {"description", description}, {"placeholder", placeholder},
            {"reason", reason},       {"server", server},     {"sni", sni},             {"fingerprint", fingerprint},
            {"link", link},           {"members", list}};
}

QString groupTitle(int group, bool russian)
{
    static const char *ru[] = {"Автовыбор", "Основные", "Резерв", "Для игр", "Белые списки", "Каскады"};
    static const char *en[] = {"Auto select", "Main", "Backup", "Gaming", "Whitelists", "Cascades"};
    group = std::clamp(group, 0, 5);
    return QString::fromUtf8(russian ? ru[group] : en[group]);
}

QString groupTitle(int group) { return groupTitle(group, Lang::ru()); }

static QString cleanTitle(const QString &s)
{
    QString t = Server::splitFlag(s.trimmed()).second.trimmed();
    static const QString trim = QStringLiteral("━─—-=⬇️↓ ");
    int a = 0, b = t.size();
    while (a < b && trim.contains(t[a]))
        ++a;
    while (b > a && trim.contains(t[b - 1]))
        --b;
    return t.mid(a, b - a).trimmed();
}

QList<Section> sections(const QList<Server> &all)
{
    const bool anyPlaceholder = std::any_of(all.begin(), all.end(), [](const Server &p) { return p.placeholder; });
    QList<Section> out;
    if (!anyPlaceholder) {
        QMap<int, QList<Server>> groups;
        for (const Server &p : all)
            groups[p.group] << p;
        int i = 0;
        for (auto it = groups.begin(); it != groups.end(); ++it, ++i) {
            Section s;
            const bool lone = i == 0 && it->size() == 1
                && (it->first().name == groupTitle(it.key(), true) || it->first().name == groupTitle(it.key()));
            s.title = lone ? QString() : groupTitle(it.key());
            s.items = it.value();
            out << s;
        }
        return out;
    }
    const Server *head = nullptr;
    QList<Server> items;
    auto flush = [&] {
        if (items.isEmpty() && !head)
            return;
        Section s;
        s.title = head ? cleanTitle(head->remarks) : QString();
        s.items = items;
        if (head && !head->description.isEmpty()) {
            s.note = head->description;
            s.hasNote = true;
        }
        out << s;
    };
    for (const Server &p : all) {
        if (p.placeholder) {
            flush();
            head = &p;
            items.clear();
        } else {
            items << p;
        }
    }
    flush();
    return out;
}

QList<Section> sorted(QList<Section> list, const QString &sort, const QHash<QString, Ping> &pings)
{
    if (sort == u"ping") {
        auto key = [&](const Server &p) -> qint64 {
            const auto it = pings.constFind(p.remarks);
            if (it == pings.constEnd())
                return qint64(INT_MAX) - 1;
            return it->ms ? *it->ms : qint64(INT_MAX);
        };
        for (Section &s : list)
            std::stable_sort(s.items.begin(), s.items.end(), [&](const Server &a, const Server &b) {
                const bool la = a.members.isEmpty(), lb = b.members.isEmpty();
                if (la != lb)
                    return !la;
                return key(a) < key(b);
            });
    } else if (sort == u"name") {
        QCollator c{QLocale(QLocale::Russian)};
        for (Section &s : list)
            std::stable_sort(s.items.begin(), s.items.end(), [&](const Server &a, const Server &b) {
                const bool la = a.members.isEmpty(), lb = b.members.isEmpty();
                if (la != lb)
                    return !la;
                return c.compare(a.name, b.name) < 0;
            });
    }
    return list;
}

QList<Server> split(const QList<Server> &all)
{
    QList<Server> out;
    for (const Server &p : all) {
        if (p.members.isEmpty()) {
            out << p;
            continue;
        }
        for (Server m : p.members) {
            m.name = QStringLiteral("%1 [%2]").arg(p.name, m.name);
            out << m;
        }
    }
    return out;
}

const Server *byRemarks(const QList<Server> &list, const QString &remarks)
{
    if (remarks.isEmpty())
        return nullptr;
    for (const Server &p : list)
        if (p.remarks == remarks)
            return &p;
    for (const Server &p : list)
        for (const Server &m : p.members)
            if (m.remarks == remarks)
                return &m;
    return nullptr;
}

QString shortKind(const QString &kind)
{
    const QStringList parts = kind.split(QStringLiteral(" · "));
    return parts.size() > 1 ? parts.mid(1).join(QStringLiteral(" · ")) : kind;
}
