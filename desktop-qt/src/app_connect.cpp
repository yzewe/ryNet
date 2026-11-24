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

void App::connectVpn()
{
    if (m_phase == 1)
        return;
    if (m_settings.autoSelect && !m_stopping && m_core->running())
        autoPick(true);
    else
        connectSelected();
}

void App::connectSelected()
{
    if (m_phase == 1)
        return;
    if (m_stopping) {
        m_resumeAfterStop = true;
        setPhase(1);
        return;
    }
    if (!m_core->running()) {
        m_respawns.clear();
        const bool elevated = m_core->elevated();
        m_core->close();
        m_core->deleteLater();
        m_core = newCore(elevated);
    }
    const auto sub = current();
    if (!sub)
        return;
    if (!m_settings.profile) {
        setNotice(t("Выберите сервер", "Choose a server"));
        return;
    }
    const auto body = Store::body(sub->id);
    if (!body)
        return;
    if (m_settings.connMode == u"tun" && !m_tunCapable) {
        setNotice(t("Разрешите режим «Весь трафик» в настройках", "Allow \"All traffic\" mode in settings"));
        setNav(QStringLiteral("settings"));
        setSettingsPage(QStringLiteral("Network"));
        return;
    }
    QJsonObject opts;
    try {
        opts = options();
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    m_pendingSub = sub->id;
    m_pendingProfile = m_settings.profile;
    const Server *chosen = m_serversSub == sub->id ? byRemarks(m_servers, *m_settings.profile) : nullptr;
    const auto [flag, rest] = Server::splitFlag(*m_settings.profile);
    m_pendingServer = chosen ? chosen->toVariant()
                             : QVariantMap{{"remarks", *m_settings.profile}, {"name", rest.isEmpty() ? *m_settings.profile : rest}, {"flag", flag}};
    m_pendingServer["sub"] = sub->id;
    m_pendingOptions = opts;
    setPhase(1);
    m_core->send(QJsonObject{{"cmd", "start"},
                             {"text", *body},
                             {"profile", *m_settings.profile},
                             {"options", opts},
                             {"tun", m_settings.connMode == u"tun"},
                             {"port", m_settings.port},
                             {"killSwitch", m_settings.killSwitch},
                             {"mtu", m_settings.tunMtu()}});
    m_startGuard.start();
}

void App::disconnectVpn()
{
    cancelAutoPick();
    m_resumeAfterStop = false;
    if (m_stopping) {
        setPhase(0);
        return;
    }
    ++m_epoch;
    m_health.stop();
    m_stopping = true;
    m_core->send(QJsonObject{{"cmd", "stop"}});
    proxyOff();
    m_blocked = false;
    setPhase(0);
}

void App::toggle()
{
    if (m_phase == 2 || m_phase == 1 || m_blocked)
        disconnectVpn();
    else
        connectVpn();
}

void App::grantTun()
{
    if (m_granting)
        return;
    m_granting = true;
    Platform::background<QString>(this, [] { return Platform::grantTun(); }, [this](QString error) {
        m_granting = false;
        if (!error.isEmpty()) {
            setNotice(error);
            return;
        }
        if (m_phase == 2 || m_phase == 1)
            disconnectVpn();
        m_winTun = false;
        restartCore();
        setNotice(t("Режим «Весь трафик» разрешён", "\"All traffic\" mode allowed"));
    });
}

void App::proxyOff()
{
    if (!m_sysProxyOn)
        return;
    m_sysProxyOn = false;
    emit sysProxyOnChanged();
    serial([] { SysProxy::disable(); });
}

void App::proxyOn(int port)
{
    QPointer<App> self(this);
    serial([self, port] {
        const bool ok = SysProxy::enable(port);
        QMetaObject::invokeMethod(qApp, [self, ok] {
            if (!self)
                return;
            if (!ok) {
                self->setNotice(t("Не удалось включить системный прокси", "Couldn't turn on the system proxy"));
                return;
            }
            if (self->m_phase != 2) {
                self->serial([] { SysProxy::disable(); });
                return;
            }
            self->m_sysProxyOn = true;
            emit self->sysProxyOnChanged();
        });
    });
}

void App::lookupExit(bool proxy, int port)
{
    const int epoch = m_epoch;
    auto *nam = new QNetworkAccessManager(this);
    nam->setProxy(proxy ? QNetworkProxy(QNetworkProxy::Socks5Proxy, QStringLiteral("127.0.0.1"), quint16(port)) : QNetworkProxy(QNetworkProxy::NoProxy));
    auto attempt = std::make_shared<std::function<void(int)>>();
    std::weak_ptr<std::function<void(int)>> weak = attempt;
    *attempt = [this, nam, epoch, weak](int n) {
        const auto attempt = weak.lock();
        if (!attempt || epoch != m_epoch || n >= 3) {
            nam->deleteLater();
            return;
        }
        auto finish = [this, nam, epoch](const QString &ip, const QString &country) {
            nam->deleteLater();
            if (epoch != m_epoch || m_phase != 2)
                return;
            m_exitIp = ip;
            m_exitFlag = flagOf(country);
            emit exitChanged();
            if (m_settings.exitFlags && !m_settings.chainEntry() && m_connectedSub && m_connectedProfile && !m_exitFlag.isEmpty() && *m_connectedSub == currentSubId()) {
                const Server *p = byRemarks(m_servers, *m_connectedProfile);
                if (p && p->members.isEmpty())
                    putCountries(*m_connectedSub, {{*m_connectedProfile, country.toUpper()}}, {{p->remarks, endpointOf(*p)}});
            }
        };
        QNetworkRequest who(QUrl(m_settings.ipUrl.isEmpty() ? ExitIp::DEFAULT_URL : m_settings.ipUrl));
        who.setRawHeader("Accept", "application/json, text/plain");
        who.setTransferTimeout(8000);
        QNetworkReply *r = nam->get(who);
        connect(r, &QNetworkReply::finished, this, [this, r, nam, n, finish, attempt] {
            r->deleteLater();
            const auto got = r->error() == QNetworkReply::NoError ? ExitIp::parse(r->read(16 * 1024)) : std::nullopt;
            if (got) {
                finish(got->ip, got->country);
                return;
            }
            QNetworkRequest trace(QUrl(QStringLiteral("https://1.1.1.1/cdn-cgi/trace")));
            trace.setTransferTimeout(8000);
            QNetworkReply *r2 = nam->get(trace);
            connect(r2, &QNetworkReply::finished, this, [r2, n, finish, attempt] {
                r2->deleteLater();
                QString ip, loc;
                const QByteArray body = r2->error() == QNetworkReply::NoError ? r2->readAll() : QByteArray();
                for (const QByteArray &line : body.split('\n')) {
                    if (line.startsWith("ip="))
                        ip = QString::fromLatin1(line.mid(3)).trimmed();
                    if (line.startsWith("loc="))
                        loc = QString::fromLatin1(line.mid(4)).trimmed();
                }
                if (!ip.isEmpty()) {
                    finish(ip, loc == u"XX" ? QString() : loc);
                    return;
                }
                QTimer::singleShot(2000 * (n + 1), [attempt, n] { (*attempt)(n + 1); });
            });
        });
    };
    if (!m_exitIp.isEmpty()) {
        m_exitIp.clear();
        m_exitFlag.clear();
        emit exitChanged();
    }
    (*attempt)(0);
}

void App::ping(const QStringList &only)
{
    if (m_pinging)
        return;
    const auto body = currentBody();
    if (!body)
        return;
    QJsonObject opts;
    try {
        opts = options();
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    opts.remove("servers");
    QStringList names;
    for (const Server &s : std::as_const(m_servers))
        if (s.supported && !s.placeholder && (only.isEmpty() || only.contains(s.remarks)))
            names << s.remarks;
    if (only.isEmpty())
        m_pings.clear();
    else
        for (const QString &name : names)
            m_pings.remove(name);
    m_pinging = true;
    ++m_pingGeneration;
    m_pingSub = m_settings.sub;
    m_waiting = names;
    emit pingsChanged();
    m_core->send(QJsonObject{{"cmd", "ping"}, {"generation", double(m_pingGeneration)},
                             {"text", *body},
                             {"names", strings(names)},
                             {"mode", pingCode(m_settings.pingMode)},
                             {"parallel", m_settings.pingParallel},
                             {"timeoutMs", m_settings.pingTimeout * 1000},
                             {"url", m_settings.pingUrl},
                             {"tune", opts}});
}

void App::stopPing()
{
    if (m_pinging)
        m_core->send(QJsonObject{{"cmd", "pingCancel"}});
}

void App::pingSome(const QStringList &remarks, const QString &sub)
{
    if (m_pinging) return;
    const QString id = sub.isEmpty() ? m_settings.sub.value_or(QString()) : sub;
    const auto body = Store::body(id);
    if (!body)
        return;
    QJsonObject opts;
    try {
        opts = options();
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    opts.remove("servers");
    opts["dns"] = strings(m_settings.dnsFor(id, remarks.isEmpty() ? std::nullopt : std::optional<QString>(remarks.first())));
    QStringList names;
    for (const QString &r : remarks)
        if (id != m_settings.sub || (byRemarks(m_servers, r) && byRemarks(m_servers, r)->supported))
            names << r;
    if (names.isEmpty())
        return;
    auto &results = id == m_settings.sub ? m_pings : m_pingsBySub[id];
    ++m_pingGeneration;
    m_pingSub = id;
    m_pinging = true;
    for (const QString &n : names) {
        results.remove(n);
        if (!m_waiting.contains(n))
            m_waiting << n;
    }
    emit pingsChanged();
    m_core->send(QJsonObject{{"cmd", "ping"}, {"generation", double(m_pingGeneration)},
                             {"text", *body},
                             {"names", strings(names)},
                             {"mode", pingCode(m_settings.pingMode)},
                             {"parallel", std::min<int>(m_settings.pingParallel, int(names.size()))},
                             {"timeoutMs", m_settings.pingTimeout * 1000},
                             {"url", m_settings.pingUrl},
                             {"tune", opts}});
}

void App::probe()
{
    const auto body = currentBody();
    if (!body || !m_settings.profile)
        return;
    try {
        m_core->send(QJsonObject{{"cmd", "probe"}, {"text", *body}, {"profile", *m_settings.profile}, {"options", options()}});
    } catch (const Fail &f) {
        setNotice(f.text());
    }
}

void App::checkHealth(bool again)
{
    if (m_phase != 2 || (again && m_checkingHealth) || m_stopping)
        return;
    const int epoch = m_epoch;
    if (!m_connectedSub || !m_connectedProfile)
        return;
    const auto body = Store::body(*m_connectedSub);
    if (!body)
        return;
    const QString profile = *m_connectedProfile;
    const QString text = *body;
    m_checkingHealth = true;
    m_core->request(QJsonObject{{"cmd", "probe"}, {"text", text}, {"profile", profile}, {"options", m_activeOptions}},
                    [this, epoch, text, profile, again](const QJsonObject &r) {
                        if (m_closed || epoch != m_epoch || m_phase != 2) {
                            m_checkingHealth = false;
                            return;
                        }
                        if (r.contains("ms")) {
                            m_checkingHealth = false;
                            return;
                        }
                        if (again) {
                            QTimer::singleShot(5000, this, [this] { checkHealth(false); });
                            return;
                        }
                        m_checkingHealth = false;
                        m_health.stop();
                        QString reason = r.value("error").toString().trimmed();
                        if (reason.isEmpty())
                            reason = t("Сервер перестал отвечать", "Server stopped responding");
                        if (m_settings.autoSelect)
                            autoPick(false, true);
                        else if (m_settings.autoSwitch)
                            switchAway(epoch, text, profile, reason);
                        else
                            reconnectAfter(reason);
                    });
}

void App::onFailing()
{
    if (m_phase != 2 || m_stopping || (!m_settings.healthCheck && !m_settings.autoSelect) || m_checkingHealth || m_autoPicking)
        return;
    const qint64 now = nowMillis();
    if (now - m_lastFailing < 60000)
        return;
    m_lastFailing = now;
    checkHealth();
}

void App::reconnectAfter(const QString &reason)
{
    setNotice(t("Сервер перестал отвечать: %1", "Server stopped responding: %1").arg(reason));
    if (m_settings.connMode == u"tun" && m_settings.killSwitch)
        m_core->send(QJsonObject{{"cmd", "block"}});
    else
        disconnectVpn();
    connectVpn();
}

void App::switchAway(int epoch, const QString &body, const QString &from, const QString &reason)
{
    const auto cur = current();
    if (!cur || m_connectedSub != cur->id) {
        reconnectAfter(reason);
        return;
    }
    QList<Server> candidates;
    for (const Server &s : std::as_const(m_servers))
        if (s.supported && !s.placeholder && s.remarks != from)
            candidates << s;
    std::stable_sort(candidates.begin(), candidates.end(), [this](const Server &a, const Server &b) {
        const auto pa = m_pings.value(a.remarks).ms, pb = m_pings.value(b.remarks).ms;
        return pa.value_or(INT_MAX) < pb.value_or(INT_MAX);
    });
    candidates = candidates.mid(0, 5);
    if (candidates.isEmpty()) {
        reconnectAfter(reason);
        return;
    }
    auto results = std::make_shared<QHash<QString, std::optional<int>>>();
    for (const Server &c : std::as_const(candidates)) {
        m_core->request(QJsonObject{{"cmd", "probe"}, {"text", body}, {"profile", c.remarks}, {"options", m_activeOptions}},
                        [this, results, candidates, c, epoch, from, reason](const QJsonObject &r) {
                            (*results)[c.remarks] = r.contains("ms") ? std::optional<int>(r.value("ms").toInt()) : std::nullopt;
                            if (results->size() < candidates.size() || m_closed || epoch != m_epoch)
                                return;
                            const Server *best = nullptr;
                            for (const Server &s : candidates)
                                if (const auto ms = results->value(s.remarks); ms && (!best || *ms < *results->value(best->remarks)))
                                    best = &s;
                            if (!best) {
                                reconnectAfter(reason);
                                return;
                            }
                            setNotice(t("«%1» не отвечает, переключаюсь на «%2»", "\"%1\" isn't responding, switching to \"%2\"").arg(from, best->name));
                            Settings next = m_settings;
                            next.profile = best->remarks;
                            changeSettings(next);
                            connectVpn();
                        });
    }
}
void App::cancelAutoPick()
{
    m_auto.stop();
    m_autoNet.stop();
    m_autoState.reset();
    const bool choosing = std::exchange(m_autoPicking, false);
    if (m_autoInfo.value("running").toBool()) {
        m_autoInfo["running"] = false;
        emit autoChanged();
    }
    if (choosing && m_phase == 1)
        setPhase(0);
}

void App::publishAuto(const QVariantMap &info)
{
    m_autoInfo = info;
    Store::write(autoFile(), QJsonDocument(QJsonObject::fromVariantMap(info)).toJson(QJsonDocument::Compact));
    emit autoChanged();
}
