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

void App::loadServers(std::function<void()> then)
{
    const int generation = ++m_loading;
    const auto sub = current();
    if (!sub || sub->id != m_serversSub) {
        m_servers.clear();
        m_serversSub = sub ? sub->id : QString();
        emit serversChanged();
    }
    if (!sub)
        return;
    const auto body = Store::body(sub->id);
    if (!body)
        return;
    const QString id = sub->id;
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, generation, id, then](const QJsonObject &r) {
        const auto now = current();
        if (generation != m_loading || !now || now->id != id || m_closed)
            return;
        m_servers = parseServers(r, id);
        const Server *selected = byRemarks(m_servers, m_settings.profile.value_or(QString()));
        if (!selected || !selected->supported || selected->placeholder) {
            Settings next = m_settings;
            next.profile.reset();
            for (const Server &s : std::as_const(m_servers))
                if (s.supported && !s.placeholder) {
                    next.profile = s.remarks;
                    break;
                }
            changeSettings(next);
        }
        const QString err = r.value("error").toString();
        if (!err.trimmed().isEmpty())
            setNotice(err);
        emit serversChanged();
        if (m_startupPingPending) {
            m_startupPingPending = false;
            ping();
        }
        if (then)
            then();
    });
}

QJsonObject App::headers() const
{
    QJsonObject out;
    static const QRegularExpression token(QStringLiteral("^[!#$%&'*+.^_`|~0-9A-Za-z-]+$"));
    for (const QString &line : m_settings.subHeaders.split(u'\n')) {
        if (line.trimmed().isEmpty())
            continue;
        const int colon = line.indexOf(u':');
        if (colon < 0 || !token.match(line.left(colon).trimmed()).hasMatch())
            throw Fail(t("Заголовки: Имя: значение", "Headers: Name: value"));
        out[line.left(colon).trimmed()] = line.mid(colon + 1).trimmed();
    }
    return out;
}

void App::fetch(const QString &url, const std::optional<QString> &id, bool relink, const std::optional<QString> &name)
{
    if (url == Local::URL)
        return;
    QJsonObject hs;
    try {
        hs = headers();
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    const bool isNew = !id;
    setFetching(m_fetching + 1);
    const QString &ours = ownAgent();
    QJsonObject cmd{{"cmd", "fetch"},
                    {"url", url},
                    {"agent", m_settings.subClient == u"happ" ? QStringLiteral("Happ/4.3.0") : ours},
                    {"headers", hs},
                    {"sendHwid", m_settings.sendHwid},
                    {"hwid", m_settings.hwid},
                    {"happRetry", m_settings.subClient == u"rynet"}};
    if (id)
        cmd["sub"] = *id;
    m_core->request(cmd, [this, id, isNew, url, relink, name](const QJsonObject &r) {
        setFetching(m_fetching - 1);
        if (m_closed)
            return;
        const auto was = id ? std::find_if(m_subs.begin(), m_subs.end(), [&](const Sub &s) { return s.id == *id; }) : m_subs.end();
        if (id && was == m_subs.end())
            return;
        if (id && !relink && was->url != url)
            return;
        const QString err = r.value("error").toString();
        if (!err.trimmed().isEmpty()) {
            if (relink)
                emit subEdited(*id, err);
            else
                setNotice(err);
            if (isNew && r.value("needHwid").toBool() && !m_settings.sendHwid && m_hwidRetry != url) {
                m_hwidRetry = url;
                emit hwidRetryChanged();
            }
            return;
        }
        if (isNew && !m_hwidRetry.isEmpty()) {
            m_hwidRetry.clear();
            emit hwidRetryChanged();
        }
        try {
            Sub sub = Sub::from(r.value("sub").toObject());
            Store::saveBody(sub.id, r.value("body").toString());
            cancelAutoPick();
            if (m_pingSub == sub.id) {
                ++m_pingGeneration;
                m_core->send(QJsonObject{{"cmd", "pingCancel"}});
                m_pinging = false;
                m_waiting.clear();
                m_pingSub.reset();
                m_pingFlush.stop();
            }
            m_pingsBySub.remove(sub.id);
            if (m_settings.sub == sub.id) m_pings.clear();
            emit pingsChanged();
            bool found = false;
            for (Sub &s : m_subs)
                if (s.id == sub.id) {
                    sub.customName = relink ? name : s.customName;
                    s = sub;
                    found = true;
                }
            if (!found && name && sub.title.value_or(QString()).trimmed().isEmpty())
                sub.customName = name;
            if (!found)
                m_subs << sub;
            Settings next = m_settings;
            if ((isNew || !next.sub) && next.sub != sub.id) {
                if (next.sub) {
                    m_pingsBySub[*next.sub] = m_pings;
                    if (next.profile)
                        next.lastServers[*next.sub] = *next.profile;
                }
                next.sub = sub.id;
                next.profile = next.lastServers.contains(sub.id) ? std::optional<QString>(next.lastServers.value(sub.id)) : std::nullopt;
                m_pings = m_pingsBySub.value(sub.id);
                emit pingsChanged();
            }
            if (isNew)
                setAdding(false);
            changeSettings(next);
            emit subsChanged();
            loadServers();
            if (isNew)
                emit added(url);
            if (relink)
                emit subEdited(*id, QString());
            if (!isNew && !relink && m_settings.refreshNotices)
                refreshNotice(sub);
        } catch (const Fail &f) {
            if (relink)
                emit subEdited(*id, f.text());
            else
                setNotice(f.text());
        }
    });
}

void App::refreshNotice(const Sub &sub)
{
    const auto body = Store::body(sub.id);
    if (!body)
        return;
    const QString title = sub.title.value_or(QString()).trimmed();
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, title, id = sub.id](const QJsonObject &r) {
        int n = 0;
        for (const Server &p : parseServers(r, id))
            for (const Server &s : p.members.isEmpty() ? QList<Server>{p} : p.members)
                n += s.supported && !s.placeholder;
        const QString servers = plural(n, QStringLiteral("сервер"), QStringLiteral("сервера"), QStringLiteral("серверов"), QStringLiteral("server"));
        setNotice(t("%1 обновлена · %2", "%1 updated · %2").arg(title.isEmpty() ? t("Подписка", "Subscription") : title, servers));
    });
}

bool App::addServer(const QString &text)
{
    try {
        const QString input = Local::serverInput(text);
        Local::merge(Store::body(Local::ID).value_or(QString()), input);
        addLocal(input);
        return true;
    } catch (const Fail &f) {
        setNotice(f.text());
        return false;
    }
}

void App::add(const QString &text)
{
    if (text.trimmed().isEmpty())
        return;
    if (Local::isServers(text))
        addLocal(text);
    else
        fetch(text.trimmed(), std::nullopt);
}

void App::openLink(const QString &link)
{
    if (RuleSets::isLink(link)) {
        m_incoming = {{QStringLiteral("kind"), QStringLiteral("set")}, {QStringLiteral("text"), link.trimmed()}};
        emit incomingChanged();
        emit showRequested();
        return;
    }
    QVariantMap target = DeepLink::parse(link);
    if (target.value(QStringLiteral("kind")).toString() == u"servers") {
        const QStringList names = Local::preview(target.value(QStringLiteral("text")).toString());
        if (names.isEmpty()) {
            setNotice(t("Не похоже на ссылку на сервер или конфиг", "This doesn't look like a server link or config"));
        } else {
            target[QStringLiteral("count")] = int(names.size());
            target[QStringLiteral("names")] = names.size() > 5
                ? names.mid(0, 5).join(u'\n') + u'\n' + t("и ещё %1", "and %1 more").arg(names.size() - 5)
                : names.join(u'\n');
            m_incoming = target;
            emit incomingChanged();
        }
        emit showRequested();
        return;
    }
    if (target.isEmpty())
        return;
    const QUrl url(target.value(QStringLiteral("url")).toString());
    const auto same = [&url](const QString &other) {
        const QUrl o(other.trimmed());
        return o.host().compare(url.host(), Qt::CaseInsensitive) == 0 && o.port() == url.port()
            && o.path().chopped(o.path().endsWith(u'/') ? 1 : 0) == url.path().chopped(url.path().endsWith(u'/') ? 1 : 0)
            && o.query() == url.query();
    };
    for (const Sub &s : std::as_const(m_subs))
        if (s.id != Local::ID && same(s.url)) {
            target[QStringLiteral("have")] = s.id;
            target[QStringLiteral("title")] = s.name();
        }
    target[QStringLiteral("host")] = url.host();
    m_incoming = target;
    emit incomingChanged();
    emit showRequested();
}

void App::openPluginFile(const QString &path)
{
    m_incoming = {{QStringLiteral("kind"), QStringLiteral("plugin")}, {QStringLiteral("path"), path}};
    emit incomingChanged();
    emit showRequested();
}

QVariantMap App::takeIncoming()
{
    const QVariantMap link = std::exchange(m_incoming, {});
    if (!link.isEmpty())
        emit incomingChanged();
    return link;
}

void App::addLink(const QVariantMap &link)
{
    if (link.value(QStringLiteral("kind")).toString() == u"servers") {
        addLocal(link.value(QStringLiteral("text")).toString());
        return;
    }
    const QString have = link.value(QStringLiteral("have")).toString();
    for (const Sub &s : std::as_const(m_subs))
        if (!have.isEmpty() && s.id == have) {
            fetch(s.url, s.id);
            return;
        }
    const QString name = link.value(QStringLiteral("name")).toString().trimmed();
    fetch(link.value(QStringLiteral("url")).toString(), std::nullopt, false, name.isEmpty() ? std::nullopt : std::optional(name));
}

void App::retryWithHwid()
{
    const QString url = m_hwidRetry;
    if (url.isEmpty())
        return;
    m_hwidRetry.clear();
    emit hwidRetryChanged();
    Settings next = m_settings;
    next.sendHwid = true;
    changeSettings(next);
    fetch(url, std::nullopt);
}

void App::addLocal(const QString &text)
{
    try {
        Store::saveBody(Local::ID, Local::merge(Store::body(Local::ID).value_or(QString()), text));
        bool found = false;
        for (Sub &s : m_subs)
            if (s.id == Local::ID) {
                s.updated = nowSeconds();
                found = true;
            }
        if (!found)
            m_subs << Local::sub();
        setAdding(false);
        emit subsChanged();
        if (m_settings.sub != Local::ID) {
            selectSub(Local::ID);
        } else {
            save();
            loadServers();
        }
        setNotice(t("Сервер добавлен в «%1»", "Server added to \"%1\"").arg(Local::title()));
        emit added(text);
    } catch (const Fail &f) {
        setNotice(f.text());
    }
}

void App::removeLocal(const QString &remarks)
{
    if (std::none_of(m_subs.begin(), m_subs.end(), [](const Sub &s) { return s.id == Local::ID; }))
        return;
    if (m_connectedSub == Local::ID && m_connectedProfile == remarks)
        disconnectVpn();
    const auto next = Local::without(Store::body(Local::ID).value_or(QString()), remarks);
    if (!next) {
        remove(Local::ID);
        return;
    }
    Store::saveBody(Local::ID, *next);
    changeSettings(m_settings.pin(Local::ID, remarks, false));
    loadServers();
}

void App::renameLocal(const QString &remarks, const QString &name)
{
    const QString to = name.trimmed();
    if (to.isEmpty() || to == remarks || std::none_of(m_subs.begin(), m_subs.end(), [](const Sub &s) { return s.id == Local::ID; }))
        return;
    try {
        Store::saveBody(Local::ID, Local::renamed(Store::body(Local::ID).value_or(QString()), remarks, to));
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    Settings next = m_settings;
    if (next.pinned(Local::ID).contains(remarks))
        next = next.pin(Local::ID, remarks, false).pin(Local::ID, to, true);
    if (next.sub == Local::ID && next.profile == remarks)
        next.profile = to;
    if (next.lastServers.value(Local::ID) == remarks)
        next.lastServers[Local::ID] = to;
    if (next.chainSub == Local::ID && next.chainProfile == remarks)
        next.chainProfile = to;
    if (m_settings.sub == Local::ID && m_pings.contains(remarks))
        m_pings.insert(to, m_pings.take(remarks));
    if (m_connectedSub == Local::ID && m_connectedProfile == remarks)
        m_connectedProfile = to;
    changeSettings(next);
    loadServers();
}

void App::refresh(const QString &id)
{
    const auto cur = current();
    const QString want = id.isEmpty() && cur ? cur->id : id;
    for (const Sub &s : std::as_const(m_subs))
        if (s.id == want) {
            fetch(s.url, s.id);
            return;
        }
}

void App::refreshAll()
{
    const QList<Sub> all = m_subs;
    for (const Sub &s : all)
        fetch(s.url, s.id);
}

void App::remove(const QString &id)
{
    const auto it = std::find_if(m_subs.begin(), m_subs.end(), [&](const Sub &s) { return s.id == id; });
    if (it == m_subs.end())
        return;
    if (m_connectedSub == id)
        disconnectVpn();
    Store::removeBody(id);
    if (QJsonObject all = countriesFile(); all.contains(id)) {
        all.remove(id);
        Store::write(countriesPath(), QJsonDocument(all).toJson(QJsonDocument::Compact));
    }
    m_subs.erase(it);
    Settings next = m_settings;
    next.lastServers.remove(id);
    next.splitSubs.removeAll(id);
    if (next.chainSub == id) {
        next.chainSub.clear();
        next.chainProfile.clear();
        next.chain = false;
    }
    if (next.sub == id) {
        next.sub = m_subs.isEmpty() ? std::nullopt : std::optional<QString>(m_subs.first().id);
        next.profile = next.sub && next.lastServers.contains(*next.sub) ? std::optional<QString>(next.lastServers.value(*next.sub)) : std::nullopt;
        m_pings = next.sub ? m_pingsBySub.value(*next.sub) : QHash<QString, Ping>();
        emit pingsChanged();
    }
    m_pingsBySub.remove(id);
    changeSettings(next);
    emit subsChanged();
    loadServers();
}

bool App::validSubUrl(const QString &url) const
{
    const QUrl u(url.trimmed(), QUrl::StrictMode);
    return u.isValid() && (u.scheme() == u"https" || u.scheme() == u"http") && !u.host().isEmpty();
}

void App::editSub(const QString &id, const QString &name, const QString &url, bool split)
{
    const auto it = std::find_if(m_subs.begin(), m_subs.end(), [&](const Sub &s) { return s.id == id; });
    if (it == m_subs.end())
        return;
    if (split != m_settings.splitSubs.contains(id)) {
        Settings next = m_settings;
        if (split)
            next.splitSubs << id;
        else
            next.splitSubs.removeAll(id);
        changeSettings(next);
        emit subsChanged();
        if (id == currentSubId())
            loadServers();
        if (id == m_entrySub && id != currentSubId())
            showEntrySub(id);
    }
    std::optional<QString> custom = name.trimmed();
    if (custom->isEmpty() || *custom == it->title.value_or(QString()))
        custom.reset();
    const QString link = url.trimmed();
    if (Local::isLocal(*it) || link.isEmpty() || link == it->url) {
        if (custom != it->customName) {
            it->customName = custom;
            save();
            emit subsChanged();
        }
        emit subEdited(id, QString());
        return;
    }
    if (!validSubUrl(link)) {
        emit subEdited(id, t("Неверная ссылка подписки", "Invalid subscription link"));
        return;
    }
    fetch(link, id, true, custom);
}

void App::checkBalancers(const QString &id)
{
    const auto body = Store::body(id);
    if (!body) {
        QTimer::singleShot(0, this, [this, id] { emit balancersOf(id, false); });
        return;
    }
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, id](const QJsonObject &r) {
        const QJsonArray all = r.value("profiles").toArray();
        emit balancersOf(id, std::any_of(all.begin(), all.end(), [](const QJsonValue &v) { return !v.toObject().value("members").toArray().isEmpty(); }));
    });
}

void App::selectSub(const QString &id)
{
    if (m_settings.autoSelect) {
        Settings next = m_settings;
        next.autoSelect = false;
        changeSettings(next);
    }
    if (id == m_settings.sub)
        return;
    const bool reconnect = m_phase == 2 || m_blocked;
    if (m_phase == 1)
        disconnectVpn();
    Settings next = m_settings;
    if (m_settings.sub) {
        m_pingsBySub[*m_settings.sub] = m_pings;
        if (m_settings.profile)
            next.lastServers[*m_settings.sub] = *m_settings.profile;
    }
    next.sub = id;
    next.profile = next.lastServers.contains(id) ? std::optional<QString>(next.lastServers.value(id)) : std::nullopt;
    m_pings = m_pingsBySub.value(id);
    emit pingsChanged();
    changeSettings(next);
    emit subsChanged();
    loadServers([this, reconnect] {
        if (reconnect && m_settings.profile && (m_phase == 2 || m_blocked))
            connectVpn();
    });
}

void App::selectServer(const QString &remarks)
{
    const Server *s = byRemarks(m_servers, remarks);
    if (!s || !s->supported || s->placeholder)
        return;
    const bool connecting = m_phase == 1;
    if (connecting && m_settings.profile == remarks && !m_settings.autoSelect)
        return;
    if (connecting)
        disconnectVpn();
    Settings next = m_settings;
    next.profile = remarks;
    next.autoSelect = false;
    changeSettings(next);
    if (connecting || m_phase == 2 || m_phase == 3 || m_blocked)
        connectVpn();
}

void App::connectTo(const QString &remarks)
{
    selectServer(remarks);
    if (m_phase != 2)
        connectVpn();
}

QVariantMap App::server(const QString &remarks) const
{
    const Server *s = byRemarks(m_servers, remarks);
    return s ? s->toVariant() : QVariantMap();
}

QVariantMap App::entryServer(const QString &remarks) const
{
    const Server *s = byRemarks(m_entryServers, remarks);
    return s ? s->toVariant() : QVariantMap();
}

QVariantMap App::ping(const QString &remarks) const
{
    const auto it = m_pings.constFind(remarks);
    return it == m_pings.constEnd() ? QVariantMap() : pingVariant(*it);
}

QVariantMap App::pingIn(const QString &sub, const QString &remarks) const
{
    if (sub == currentSubId())
        return ping(remarks);
    const auto kept = m_pingsBySub.constFind(sub);
    if (kept == m_pingsBySub.constEnd())
        return {};
    const auto it = kept->constFind(remarks);
    return it == kept->constEnd() ? QVariantMap() : pingVariant(*it);
}

QString App::groupOf(const QString &remarks) const
{
    for (const Section &sec : sections(m_servers))
        for (const Server &p : sec.items)
            if (p.remarks == remarks)
                return sec.title;
    return {};
}

void App::loadServersOf(const QString &id)
{
    const auto cur = current();
    if (cur && id == cur->id && !m_servers.isEmpty()) {
        QTimer::singleShot(0, this, [this, id] { emit serversOfReady(id, serversVariant()); });
        return;
    }
    const auto body = Store::body(id);
    if (!body) {
        QTimer::singleShot(0, this, [this, id] { emit serversOfReady(id, {}); });
        return;
    }
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, id](const QJsonObject &r) {
        QVariantList out;
        for (const Server &s : parseServers(r, id))
            out << s.toVariant();
        emit serversOfReady(id, out);
    });
}

void App::details(const QString &remarks)
{
    const auto cur = current();
    const auto body = cur ? Store::body(cur->id) : std::nullopt;
    if (!body) {
        QTimer::singleShot(0, this, [this, remarks] { emit detailsReady(remarks, {{QStringLiteral("error"), t("Нет подписки", "No subscription")}}); });
        return;
    }
    m_core->request(QJsonObject{{"cmd", "details"}, {"text", *body}, {"profile", remarks}}, [this, remarks](const QJsonObject &r) {
        const QString err = r.value("error").toString();
        if (!err.isEmpty()) {
            emit detailsReady(remarks, {{QStringLiteral("error"), err}});
            return;
        }
        QVariantMap d = r.value("details").toObject().toVariantMap();
        const QStringList lines = d.value(QStringLiteral("json")).toString().split(u'\n');
        d[QStringLiteral("long")] = lines.size() > 10;
        d[QStringLiteral("short")] = lines.size() > 10 ? QStringList(lines.mid(0, 10)).join(u'\n') + QStringLiteral("\n…") : d.value(QStringLiteral("json"));
        emit detailsReady(remarks, d);
    });
}

QJsonObject App::options() const
{
    const auto sub = current();
    QJsonObject o = m_settings.toJson();
    o["serverDns"] = m_settings.serverDnsArg();
    o["userRules"] = m_settings.effectiveRules();
    o["dns"] = strings(m_settings.dnsFor(sub ? std::optional<QString>(sub->id) : std::nullopt, m_settings.profile));
    if (sub && sub->routing) {
        const QJsonDocument routing = QJsonDocument::fromJson(sub->routing->toUtf8());
        if (routing.isObject())
            o["subRouting"] = routing.object();
    }
    o.remove("chain");
    o.remove("fragment");
    if (const auto chain = m_settings.chainEntry()) {
        const auto body = Store::body(chain->first);
        if (!body)
            throw Fail(t("Вход цепочки был в удалённой подписке — выберите его заново", "The chain entry was in a removed subscription: pick it again"));
        o["chain"] = QJsonObject{{"config", *body}, {"profile", chain->second}};
    }
    if (m_settings.fragment)
        o["fragment"] = QJsonObject{{"length", m_settings.fragmentLength}, {"interval", m_settings.fragmentInterval}};
    QJsonArray servers;
    for (const auto &[key, tag] : Rules::servers(m_settings.rules, m_settings.sets)) {
        const auto body = Store::body(key.first);
        if (!body)
            continue;
        servers.append(QJsonObject{{"config", *body}, {"profile", key.second}, {"tag", tag}});
    }
    if (!servers.isEmpty())
        o["servers"] = servers;
    return o;
}
