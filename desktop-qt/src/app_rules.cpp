#include "endpoints.h"
#include "app.h"

#include "core.h"
#include "filepick.h"
#include "platform.h"
#include "reports.h"
#include "store.h"
#include "theme.h"
#include "tray.h"

#include <cmath>
#include <QClipboard>
#include <QCryptographicHash>
#include <memory>
#include <QDate>
#include <QDir>
#include <QFile>
#include <QDateTime>
#ifdef RYNET_DBUS
#include <QDBusConnection>
#endif
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLocale>
#include <QNetworkAccessManager>
#include <QNetworkInformation>
#include <QNetworkProxy>
#include <QNetworkReply>
#include <QRegularExpression>
#include <QSet>
#include <QSysInfo>
#include <QThread>
#include <QUrl>
#include <tuple>
#include "app_detail.h"

using namespace AppDetail;

void App::set(const QString &key, const QVariant &value, bool reconnect)
{
    QJsonObject j = m_settings.toJson();
    j[key] = QJsonValue::fromVariant(value);
    Settings next = Settings::from(j);
    const Settings &was = m_settings;
    const auto bad = [](int value, int old, int low, int high) { return value != old && (value < low || value > high); };
    if (bad(next.port, was.port, 1, 65535) || bad(next.pingParallel, was.pingParallel, 1, 64)
        || bad(next.pingTimeout, was.pingTimeout, 1, 60)) {
        setNotice(t("Неверный порт или параметры пинга", "Invalid port or ping settings"));
        return;
    }
    if (next.mtu != 0 && bad(next.mtu, was.mtu, 1280, 9000)) {
        setNotice(QStringLiteral("MTU: 1280–9000"));
        return;
    }
    if (next.fragment && !was.fragment
        && !(Fragment::validRange(next.fragmentLength, 65535) && Fragment::validRange(next.fragmentInterval, 1000))) {
        const Settings d;
        next.fragmentLength = d.fragmentLength;
        next.fragmentInterval = d.fragmentInterval;
    }
    const bool badLength = next.fragmentLength != was.fragmentLength && !Fragment::validRange(next.fragmentLength, 65535);
    const bool badInterval = next.fragmentInterval != was.fragmentInterval && !Fragment::validRange(next.fragmentInterval, 1000);
    if (next.fragment && (badLength || badInterval)) {
        setNotice(t("Неверный диапазон фрагментации", "Invalid fragmentation range"));
        return;
    }
    const bool hadFlags = m_settings.exitFlags;
    apply(next, reconnect);
    if (key == u"autoSelect" && next.autoSelect && m_phase == 2)
        autoPick(false);
    if (key == u"exitFlags" && next.exitFlags != hadFlags) {
        if (!next.exitFlags)
            QFile::remove(countriesPath());
        loadServers();
    }
}

void App::setRouting(const QString &mode)
{
    Settings next = m_settings;
    next.routing = mode;
    changeSettings(next);
    updateGeo();
    setNotice(t("Настройки сохранены", "Settings saved"));
    if (m_phase == 2)
        connectVpn();
}

void App::setSystemProxy(bool on)
{
    Settings next = m_settings;
    next.systemProxy = on;
    changeSettings(next);
    if (m_phase == 2 && m_settings.connMode == u"proxy") {
        if (on && SysProxy::available())
            proxyOn(m_settings.port);
        else
            proxyOff();
    }
}

void App::setDebugLog(bool on)
{
    Settings next = m_settings;
    next.debugLog = on;
    changeSettings(next);
    if (m_phase == 2 || m_phase == 1) {
        setNotice(t("Журнал станет подробным после переподключения", "The log becomes verbose after reconnecting"));
        m_restartAfterStop = true;
        return;
    }
    restartCore(m_core && m_core->elevated());
}

void App::setSendLogs(bool on)
{
    if (on)
        Reports::skipToEnd(Store::dir());
    Settings next = m_settings;
    next.sendLogs = on;
    changeSettings(next);
    if (on && !m_settings.debugLog)
        setDebugLog(true);
}

void App::setSeedColor(const QString &color)
{
    static const QRegularExpression rgbHex(QStringLiteral("^#[0-9a-fA-F]{6}$"));
    Settings next = m_settings;
    next.seedColor = rgbHex.match(color).hasMatch() ? color.toLower() : QString();
    next.wallpaperColors = false;
    changeSettings(next);
}

void App::setAutostart(bool on)
{
    try {
        Platform::autostart(on);
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    Settings next = m_settings;
    next.autostart = on;
    changeSettings(next);
}

void App::pin(const QString &remarks, bool on)
{
    const auto sub = current();
    if (sub)
        apply(m_settings.pin(sub->id, remarks, on));
}

bool App::pinned(const QString &remarks) const
{
    const auto sub = current();
    return sub && m_settings.pinned(sub->id).contains(remarks);
}

QString App::serverDns(const QString &remarks) const
{
    const auto sub = current();
    return sub ? m_settings.perServerDns.value(sub->id).value(remarks) : QString();
}

void App::setServerDns(const QString &remarks, const QString &dns)
{
    const auto sub = current();
    if (sub)
        apply(m_settings.serverDnsFor(sub->id, remarks, dns), remarks == m_settings.profile);
}

void App::setChainEntry(const QString &remarks, const QString &subId)
{
    const auto sub = current();
    if (!sub)
        return;
    if (!subId.isEmpty() && std::none_of(m_subs.begin(), m_subs.end(), [&](const Sub &s) { return s.id == subId; }))
        return;
    Settings next = m_settings;
    if (remarks.isEmpty()) {
        next.chain = false;
    } else {
        next.chain = true;
        next.chainSub = subId.isEmpty() ? sub->id : subId;
        next.chainProfile = remarks;
    }
    changeSettings(next);
    if (m_phase == 2)
        connectVpn();
}

void App::openEntryPicker()
{
    const auto chain = m_settings.chainEntry();
    QString id = currentSubId();
    if (chain && std::any_of(m_subs.begin(), m_subs.end(), [&](const Sub &s) { return s.id == chain->first; }))
        id = chain->first;
    showEntrySub(id);
    setNav(QStringLiteral("servers"));
    setEntryPick(true);
}

void App::showEntrySub(const QString &id)
{
    m_entrySub = id;
    m_entryServers = id == currentSubId() ? m_servers : QList<Server>();
    emit entryChanged();
    if (id.isEmpty() || id == currentSubId())
        return;
    const auto body = Store::body(id);
    if (!body)
        return;
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, id](const QJsonObject &r) {
        if (m_entrySub != id)
            return;
        m_entryServers = parseServers(r, id);
        emit entryChanged();
    });
}

void App::pickEntry(const QString &remarks)
{
    const QString id = m_entrySub;
    if (id.isEmpty())
        return;
    const Server *s = byRemarks(m_entryServers, remarks);
    if (!s || !s->supported || s->placeholder || (id == currentSubId() && m_settings.profile == remarks))
        return;
    Settings next = m_settings;
    next.chain = true;
    next.chainSub = id;
    next.chainProfile = remarks;
    changeSettings(next);
    setEntryPick(false);
    setNav(QStringLiteral("home"));
    if (m_phase == 2 || m_phase == 3 || m_blocked)
        connectVpn();
}

QVariantMap App::chainServer() const
{
    const auto chain = m_settings.chainEntry();
    if (!chain)
        return {};
    const auto &[subId, name] = *chain;
    if (subId == currentSubId())
        for (const Server &s : m_servers)
            if (s.remarks == name)
                return s.toVariant();
    if (subId == m_entrySub)
        for (const Server &s : m_entryServers)
            if (s.remarks == name)
                return s.toVariant();
    const auto [flag, rest] = Server::splitFlag(name);
    return {{"remarks", name}, {"name", rest.isEmpty() ? name : rest}, {"flag", flag}, {"supported", true}};
}

void App::updateGeo()
{
    if (m_core)
        m_core->send(QJsonObject{{"cmd", "geo"}, {"text", m_settings.effectiveRules()}});
}

bool App::validDns(const QString &text) const
{
    static const QRegularExpression sep(QStringLiteral("[\\s,]+"));
    const QStringList parts = text.split(sep, Qt::SkipEmptyParts);
    return !parts.isEmpty() && std::all_of(parts.begin(), parts.end(), [](const QString &a) { return ::validDns(a); });
}

bool App::validRange(const QString &v, int max) const { return Fragment::validRange(v, max); }

QVariantList App::rules() const
{
    QVariantList out;
    for (const Rule &r : m_settings.rules)
        out << QVariantMap{{"kind", r.kind}, {"value", r.value}, {"action", r.action},
                           {"sub", r.server ? r.server->first : QString()}, {"profile", r.server ? r.server->second : QString()}};
    return out;
}

static Rule ruleOf(const QVariantMap &m)
{
    Rule r;
    r.kind = m.value("kind").toString();
    r.value = m.value("value").toString().trimmed();
    r.action = m.value("action").toString();
    const QString sub = m.value("sub").toString(), profile = m.value("profile").toString();
    if (r.action == u"server" && !sub.isEmpty() && !profile.isEmpty())
        r.server = QPair<QString, QString>(sub, profile);
    return r;
}

void App::saveRule(int index, const QVariantMap &rule)
{
    Settings next = m_settings;
    const Rule r = ruleOf(rule);
    if (index >= 0 && index < next.rules.size())
        next.rules[index] = r;
    else
        next.rules << r;
    apply(next, true);
}

void App::deleteRule(int index)
{
    if (index < 0 || index >= m_settings.rules.size())
        return;
    Settings next = m_settings;
    next.rules.removeAt(index);
    apply(next, true);
}

bool App::ruleValid(const QVariantMap &rule) const { return ruleOf(rule).valid(); }

QVariantMap App::subscriptionRules() const
{
    if (m_settings.routing != u"subscription")
        return {};
    const auto sub = current();
    const QVariantMap happ = sub ? SubscriptionRules::parse(sub->routing) : QVariantMap();
    const QString title = happ.value("name").toString().isEmpty() ? t("Правила подписки", "Subscription rules") : happ.value("name").toString();
    if (sub) {
        const QVariantMap own = ModeRules::fromConfig(Store::body(sub->id).value_or(QString()), m_settings.profile.value_or(QString()));
        if (!own.isEmpty())
            return {{"name", title}, {"entries", own.value("entries")}, {"rest", own.value("rest")}, {"active", true}};
    }
    return {{"name", title}, {"entries", happ.isEmpty() ? QVariantList() : ModeRules::subscription(happ)},
            {"rest", happ.isEmpty() ? QStringLiteral("proxy") : happ.value("rest")}, {"active", true}};
}

QVariantMap App::modeRules(const QString &which) const
{
    if (which == u"smart") {
        QStringList nets;
        for (const QJsonValue &v : m_smartRules.value("private").toArray())
            nets << v.toString();
        return {{"title", t("Режим Умный", "Smart mode")}, {"entries", ModeRules::fromXray(m_smartRules)}, {"rest", QString()}, {"private", nets}};
    }
    const QVariantMap sub = subscriptionRules();
    return {{"title", sub.value("name")}, {"entries", sub.value("entries")}, {"rest", sub.value("rest")}, {"private", QStringList()}};
}

void App::geoCounts(const QVariantList &items, const QJSValue &done)
{
    QJSValue callback = done;
    QPointer<QJSEngine> engine = qjsEngine(this);
    m_core->request(QJsonObject{{"cmd", "geoCounts"}, {"items", QJsonArray::fromVariantList(items)}}, [callback, engine](const QJsonObject &r) mutable {
        if (engine && callback.isCallable())
            callback.call({engine->toScriptValue(r.value("counts").toArray().toVariantList())});
    });
}

void App::importRules(const QString &text, const QJSValue &done)
{
    QJSValue callback = done;
    QPointer<QJSEngine> engine = qjsEngine(this);
    m_core->request(QJsonObject{{"cmd", "importRules"}, {"text", text}}, [callback, engine](const QJsonObject &r) mutable {
        QVariantMap out{{"rules", r.value("rules").toString()}, {"skipped", int(r.value("skipped").toArray().size())}};
        if (r.contains("error"))
            out = {{"error", r.value("error").toString()}};
        if (engine && callback.isCallable())
            callback.call({engine->toScriptValue(out)});
    });
}

void App::coverWarnings(const QString &kind, const QString &value, const QString &action, const QJSValue &done)
{
    QString v = value.trimmed();
    if (kind == u"domain")
        v = Rules::domain(v).value_or(QString());
    else if (kind == u"ip")
        v = Rules::cidr(v) ? Rules::cidr(v)->first : QString();
    struct Source { QVariantMap entry; QString origin; bool user; };
    QList<Source> sources;
    const QString mine = t("ваши правила", "your rules");
    for (const Rule &r : m_settings.rules)
        sources.append({ModeRules::entry(r.kind == u"domain" ? "suffix" : r.kind == u"ip" ? "cidr" : "app", r.value, r.action), mine, true});
    for (const RuleSet &s : m_settings.sets) {
        if (!s.enabled)
            continue;
        const QString name = s.name.trimmed().isEmpty() ? t("Набор", "Set") : s.name.trimmed();
        const QString o = t("набор «%1»", "set \"%1\"").arg(name);
        for (const QString &d : s.domains)
            sources.append({ModeRules::entry("suffix", d, s.action), o, true});
        for (const QString &i : s.ips)
            sources.append({ModeRules::entry("cidr", i, s.action), o, true});
        for (const QString &g : s.geosite)
            sources.append({ModeRules::entry("geosite", g, s.action), o, true});
        for (const QString &p : s.procs)
            sources.append({ModeRules::entry("app", p, s.action), o, true});
    }
    if (m_settings.routing == u"smart") {
        for (const QVariant &e : ModeRules::fromXray(m_smartRules))
            sources.append({e.toMap(), t("режим Умный", "Smart mode"), false});
    } else if (m_settings.routing == u"subscription" && sources.isEmpty() && m_settings.userRules.trimmed().isEmpty()) {
        for (const QVariant &e : subscriptionRules().value("entries").toList())
            sources.append({e.toMap(), t("режим Подписка", "Subscription mode"), false});
    }
    QJSValue callback = done;
    QPointer<QJSEngine> engine = qjsEngine(this);
    if (v.isEmpty() || sources.isEmpty()) {
        QTimer::singleShot(0, this, [callback, engine]() mutable {
            if (engine && callback.isCallable())
                callback.call({engine->toScriptValue(QStringList())});
        });
        return;
    }
    QJsonArray list;
    for (const Source &s : sources)
        list.append(QJsonObject{{"k", s.entry.value("kind").toString()}, {"v", s.entry.value("value").toString()}});
    m_core->request(QJsonObject{{"cmd", "cover"}, {"kind", kind}, {"values", QJsonArray{v}}, {"sources", list}},
                    [callback, engine, sources, v, action](const QJsonObject &r) mutable {
        QStringList out;
        const auto does = [](const QString &a) {
            return a == u"direct" ? t("идёт напрямую", "goes direct") : a == u"block" ? t("блокируется", "is blocked")
                 : a == u"server" ? t("идёт через сервер", "goes through a server") : t("идёт через VPN", "goes through the VPN");
        };
        const int i = r.value("found").toArray().at(0).toInt(-1);
        if (i >= 0 && i < sources.size()) {
            const Source &src = sources[i];
            const QString rule = src.entry.value("text").toString() + " (" + src.origin + ")";
            const QString now = src.entry.value("action").toString();
            if (now == action)
                out << t("%1 уже %2 по правилу %3", "%1 already %2 by the rule %3").arg(v, does(action), rule);
            else if (src.user)
                out << t("Конфликт: %1 %2 по правилу %3, оно сработает раньше нового", "Conflict: %1 %2 by the rule %3, which is applied before the new one").arg(v, does(now), rule);
            else
                out << t("%1 %2 по правилу %3, но новое правило сработает раньше", "%1 %2 by the rule %3, but the new rule is applied first").arg(v, does(now), rule);
        }
        if (engine && callback.isCallable())
            callback.call({engine->toScriptValue(out)});
    });
}

QVariantList App::sets() const
{
    QVariantList out;
    for (const RuleSet &s : m_settings.sets)
        out << RuleSets::toVariant(s);
    return out;
}

QVariantMap App::ruleSet(const QString &id) const
{
    for (const RuleSet &s : m_settings.sets)
        if (s.id == id)
            return RuleSets::toVariant(s);
    return {};
}

QVariantMap App::newSet() const
{
    RuleSet s;
    s.id = RuleSets::newId();
    return RuleSets::toVariant(s);
}

void App::saveSet(const QVariantMap &set)
{
    const RuleSet s = RuleSets::fromVariant(set);
    Settings next = m_settings;
    const auto it = std::find_if(next.sets.begin(), next.sets.end(), [&](const RuleSet &x) { return x.id == s.id; });
    if (it != next.sets.end())
        *it = s;
    else
        next.sets << s;
    const bool rules = next.effectiveRules() != m_settings.effectiveRules()
        || Rules::servers(next.rules, next.sets) != Rules::servers(m_settings.rules, m_settings.sets);
    apply(next, rules);
    if (rules)
        updateGeo();
}

void App::deleteSet(const QString &id)
{
    Settings next = m_settings;
    next.sets.removeIf([&](const RuleSet &s) { return s.id == id; });
    apply(next, true);
}

void App::toggleSet(const QString &id)
{
    const auto it = std::find_if(m_settings.sets.begin(), m_settings.sets.end(), [&](const RuleSet &s) { return s.id == id; });
    if (it == m_settings.sets.end())
        return;
    RuleSet target = *it;
    QList<RuleSet> on, off;
    for (const RuleSet &s : m_settings.sets) {
        if (s.id == id)
            continue;
        (s.enabled ? on : off) << s;
    }
    target.enabled = !target.enabled;
    Settings next = m_settings;
    next.sets = on;
    next.sets << target;
    next.sets << off;
    apply(next, true);
}

void App::orderSets(const QStringList &order, int enabled)
{
    QList<RuleSet> next;
    for (int i = 0; i < order.size(); ++i)
        for (const RuleSet &s : m_settings.sets)
            if (s.id == order[i]) {
                RuleSet x = s;
                x.enabled = i < enabled;
                next << x;
            }
    if (next.size() != m_settings.sets.size() || next == m_settings.sets)
        return;
    Settings s = m_settings;
    s.sets = next;
    apply(s, true);
}

QString App::setLink(const QVariantMap &set) const { return RuleSets::link(RuleSets::fromVariant(set)); }

QStringList App::setParts(const QString &link) const { return RuleSets::parts(link); }

QVariantMap App::parseSet(const QString &text) const
{
    const auto s = RuleSets::parse(text);
    return s ? RuleSets::toVariant(*s) : QVariantMap();
}

QVariantMap App::normalizeEntries(const QString &kind, const QString &text) const
{
    const QStringList parts = RuleSets::split(text, kind == u"procs");
    QStringList good;
    for (const QString &p : parts) {
        std::optional<QString> v;
        if (kind == u"domains") {
            v = Rules::domain(p);
        } else if (kind == u"geosite") {
            v = RuleSets::geosite(p);
        } else if (kind == u"ips") {
            if (const auto c = Rules::cidr(p))
                v = c->first;
        } else if (Rules::validProcess(p.trimmed())) {
            v = p.trimmed();
        }
        if (v)
            good << *v;
    }
    const bool ok = !good.isEmpty() && good.size() == parts.size();
    good.removeDuplicates();
    return {{"good", good}, {"total", int(parts.size())}, {"ok", ok}};
}
