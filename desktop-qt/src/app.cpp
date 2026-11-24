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

App *App::instance()
{
    static App *a = new App;
    return a;
}

App *App::create(QQmlEngine *, QJSEngine *)
{
    QJSEngine::setObjectOwnership(instance(), QJSEngine::CppOwnership);
    return instance();
}

App::App(QObject *parent)
    : QObject(parent)
{
    m_subs = Store::subs();
    m_settings = Store::settings();
    if (!m_settings.sub && !m_subs.isEmpty())
        m_settings.sub = m_subs.first().id;
    m_net = new QNetworkAccessManager(this);
    m_tray = new Tray(this);
    connect(m_tray, &Tray::availableChanged, this, &App::trayChanged);
    connect(m_tray, &Tray::event, this, [this](const QJsonObject &e) {
        const QString ev = e.value("ev").toString();
        if (ev == u"open") {
            emit showRequested();
        } else if (ev == u"toggle") {
            toggle();
        } else if (ev == u"quit") {
            quit();
        } else if (ev == u"server") {
            const QString name = e.value("name").toString();
            if (byRemarks(m_servers, name)) {
                selectServer(name);
                if (m_phase != 2 && m_phase != 1)
                    connectVpn();
            }
        }
    });
    for (auto sig : {&App::serversChanged, &App::pingsChanged, &App::subsChanged, &App::settingsChanged, &App::entryChanged})
        connect(this, sig, this, [this] {
            ++m_rev;
            emit revChanged();
        });
    connect(this, &App::connectionChanged, this, &App::updateTray);
    connect(this, &App::settingsChanged, this, &App::updateTray);
    connect(this, &App::serversChanged, this, &App::updateTray);

    m_pingFlush.setSingleShot(true);
    m_pingFlush.setInterval(50);
    connect(&m_pingFlush, &QTimer::timeout, this, &App::pingsChanged);
    connect(&m_health, &QTimer::timeout, this, [this] { checkHealth(); });
    m_startGuard.setSingleShot(true);
    m_startGuard.setInterval(90000);
    connect(&m_startGuard, &QTimer::timeout, this, [this] {
        if (m_phase != 1 || m_stopping || m_closed)
            return;
        const bool elevated = m_core && m_core->elevated();
        if (m_core) {
            m_core->close();
            m_core->deleteLater();
        }
        m_core = newCore(elevated);
        m_reconnectAfterReady = m_resumeAfterStop = false;
        onError(QJsonObject{{"message", t("Подключение не удалось: ядро не ответило", "Couldn't connect: the core did not respond")}});
    });
    m_auto.setSingleShot(true);
    connect(&m_auto, &QTimer::timeout, this, &App::autoTick);
    m_autoNet.setSingleShot(true);
    m_autoNet.setInterval(kNetMs);
    connect(&m_autoNet, &QTimer::timeout, this, [this] { autoPick(false); });
    m_autoInfo = readJson(autoFile()).toVariantMap();
    {
        const QJsonObject saved = readJson(updateFile());
        m_updateAt = qint64(saved.value("checkedAt").toDouble());
        if (const auto f = Update::Found::fromJson(saved.value("found").toObject()); f && Update::newer(Platform::version(), f->version))
            m_update = f;
    }
    m_autoInfo["running"] = false;
    m_autoInfo["next"] = 0;
    m_report.setInterval(60000);
    connect(&m_report, &QTimer::timeout, this, &App::reportTick);
    connect(&m_report, &QTimer::timeout, this, &App::clockTick);
    m_netChange.setSingleShot(true);
    m_netChange.setInterval(1500);
    connect(&m_netChange, &QTimer::timeout, this, &App::networkChanged);
    m_expiry.setInterval(6 * 3600 * 1000);
    connect(&m_expiry, &QTimer::timeout, this, &App::expiryAlerts);
    m_noticeTimer.setSingleShot(true);
    m_noticeTimer.setInterval(6000);
    connect(&m_noticeTimer, &QTimer::timeout, this, [this] { setNotice({}); });

    m_worker = new QThread(this);
    m_serial = new QObject;
    m_serial->moveToThread(m_worker);
    connect(m_worker, &QThread::finished, m_serial, &QObject::deleteLater);
    m_worker->start();
    serial([] { SysProxy::recover(); });

    Theme::instance()->setMode(m_settings.theme, m_settings.wallpaperColors, m_settings.seedColor);
}

App::~App() { shutdown(); }

void App::start(bool autostart)
{
    m_core = newCore(wantElevated());
    m_startupPingPending = m_settings.pingOnStart;
    if (QNetworkInformation::loadDefaultBackend()) {
        connect(QNetworkInformation::instance(), &QNetworkInformation::transportMediumChanged, this, [this] {
            if (m_settings.autoSelect && m_phase == 2)
                m_autoNet.start();
            m_netChange.start();
        });
        connect(QNetworkInformation::instance(), &QNetworkInformation::reachabilityChanged, this, [this](QNetworkInformation::Reachability r) {
            if (r == QNetworkInformation::Reachability::Online)
                m_netChange.start();
        });
    }
#ifdef RYNET_DBUS
    QDBusConnection::systemBus().connect(QStringLiteral("org.freedesktop.login1"), QStringLiteral("/org/freedesktop/login1"), QStringLiteral("org.freedesktop.login1.Manager"),
                                         QStringLiteral("PrepareForSleep"), this, SLOT(prepareForSleep(bool)));
#endif
    loadServers();
    autoUpdateSubs();
    m_tickAt = nowMillis();
    m_report.start();
    m_tray->start();
    updateTray();
    expiryAlerts();
    m_expiry.start();
    if (autostart)
        connectVpn();
}

void App::shutdown()
{
    if (m_closed)
        return;
    m_closed = true;
    m_health.stop();
    m_report.stop();
    cancelAutoPick();
    if (m_worker) {
        m_worker->quit();
        m_worker->wait(5000);
    }
    if (m_sysProxyOn) {
        SysProxy::disable();
        m_sysProxyOn = false;
    }
    delete m_core;
    m_core = nullptr;
    m_tray->close();
}

void App::serial(std::function<void()> work)
{
    if (m_serial && m_worker && m_worker->isRunning())
        QMetaObject::invokeMethod(m_serial, std::move(work), Qt::QueuedConnection);
    else
        work();
}

void App::setWindowVisible(bool on)
{
    m_visible = on;
    Theme::instance()->setWatching(on);
}

bool App::windows() const { return Platform::windows(); }
QString App::version() const { return Platform::version(); }
bool App::sysProxyAvailable() const { return SysProxy::available(); }
bool App::trayAvailable() const { return m_tray && m_tray->available(); }

std::optional<Sub> App::current() const
{
    if (m_settings.sub)
        for (const Sub &s : m_subs)
            if (s.id == *m_settings.sub)
                return s;
    if (!m_subs.isEmpty())
        return m_subs.first();
    return {};
}

std::optional<QString> App::currentBody() const
{
    const auto s = current();
    return s ? Store::body(s->id) : std::nullopt;
}

QVariantList App::subsVariant() const
{
    QVariantList out;
    for (const Sub &s : m_subs)
        out << subVariant(s);
    return out;
}

QVariantMap App::subVariant(const Sub &s) const
{
    QVariantMap m = s.toVariant();
    QStringList parts;
    const qint64 used = s.upload + s.download;
    if (s.total > 0)
        parts << t("%1 из %2", "%1 of %2").arg(bytes(double(used)), bytes(double(s.total)));
    else if (used > 0)
        parts << bytes(double(used));
    if (s.expire > 0) {
        const qint64 days = qint64(std::floor(double(s.expire - nowSeconds()) / 86400));
        parts << (days < 0 ? t("закончилась", "expired") : t("%1 дн.", "%1 d").arg(days));
    }
    m[QStringLiteral("line")] = parts.join(QStringLiteral(" · "));
    m[QStringLiteral("split")] = m_settings.splitSubs.contains(s.id);
    QStringList facts;
    if (s.total > 0)
        facts << t("%1 из %2", "%1 of %2").arg(bytes(double(used)), bytes(double(s.total)));
    if (s.expire > 0) {
        const qint64 days = qint64(std::floor(double(s.expire - nowSeconds()) / 86400));
        facts << (days < 0 ? t("Закончилась", "Expired") : t("Осталось %1 дн.", "%1 d left").arg(days));
    }
    if (s.updated > 0)
        facts << t("Обновлена ", "Updated ") + ago(double(nowSeconds() - s.updated));
    for (QString &f : facts)
        f.replace(QLatin1Char(' '), QChar(0x00a0));
    m[QStringLiteral("facts")] = facts.join(QStringLiteral("\u00a0· "));
    return m;
}

QString App::subName(const QString &id) const
{
    for (const Sub &s : m_subs)
        if (s.id == id) {
            const QVariantMap m = s.toVariant();
            const QString name = m.value(QStringLiteral("name")).toString();
            return !name.isEmpty() ? name : m.value(QStringLiteral("host")).toString();
        }
    return {};
}

QVariantMap App::subOr(const QString &id) const
{
    for (const Sub &s : m_subs)
        if (s.id == id)
            return subVariant(s);
    if (const auto s = current())
        return subVariant(*s);
    return m_subs.isEmpty() ? QVariantMap() : subVariant(m_subs.first());
}

QString App::currentSubId() const
{
    const auto s = current();
    return s ? s->id : QString();
}

QVariantMap App::currentVariant() const
{
    const auto s = current();
    return s ? s->toVariant() : QVariantMap();
}

QString App::expiryAlert() const
{
    const auto s = current();
    if (!s || s->expire <= 0)
        return {};
    const qint64 left = s->expire - nowSeconds();
    if (left < 0)
        return t("Подписка закончилась", "Subscription expired");
    const qint64 days = left / 86400;
    if (days > 3)
        return {};
    if (days == 0)
        return t("Подписка закончится через день", "Subscription expires in a day");
    return Lang::ru() ? QStringLiteral("Подписка закончится через %1 дн.").arg(days)
                      : (days <= 1 ? QStringLiteral("Subscription expires in a day") : QStringLiteral("Subscription expires in %1 days").arg(days));
}

QVariantMap App::settingsVariant() const
{
    QVariantMap m = m_settings.toJson().toVariantMap();
    const auto chain = m_settings.chainEntry();
    m["chainActive"] = chain.has_value();
    m["fragmentPreset"] = Fragment::of(m_settings.fragmentLength, m_settings.fragmentInterval);
    m["ruleCount"] = int(m_settings.rules.size() + m_settings.sets.size()) + extraRuleLines();
    return m;
}

QVariantList App::serversVariant() const
{
    QVariantList out;
    for (const Server &s : m_servers)
        out << s.toVariant();
    return out;
}

int App::supportedCount() const
{
    int n = 0;
    for (const Server &s : m_servers)
        if (s.supported && !s.placeholder)
            ++n;
    return n;
}

QVariantMap App::pingsVariant() const
{
    QVariantMap m;
    for (auto it = m_pings.begin(); it != m_pings.end(); ++it)
        m[it.key()] = pingVariant(it.value());
    return m;
}

int App::status() const
{
    if (m_blocked)
        return 3;
    switch (m_phase) {
    case 2:
        return 2;
    case 1:
        return 1;
    case 3:
        return 3;
    default:
        return 0;
    }
}

QString App::statusTitle() const
{
    if (m_blocked)
        return t("Интернет заблокирован", "Internet blocked");
    switch (m_phase) {
    case 2:
        return t("Защищено", "Protected");
    case 1:
        return m_autoPicking ? t("Выбираю сервер…", "Choosing a server…") : t("Подключение…", "Connecting…");
    case 3:
        return t("Не удалось", "Failed");
    default:
        return t("Не подключено", "Not connected");
    }
}

void App::setNotice(const QString &n)
{
    if (!n.isEmpty())
        m_noticeTimer.start();
    if (n == m_notice)
        return;
    m_notice = n;
    emit noticeChanged();
}

void App::setAdding(bool on)
{
    if (on == m_adding)
        return;
    m_adding = on;
    emit addingChanged();
}

void App::setNav(const QString &n)
{
    if (m_entryPick) {
        m_entryPick = false;
        emit entryChanged();
    }
    if (n == m_nav)
        return;
    m_nav = n;
    emit navChanged();
}

void App::setSettingsPage(const QString &p)
{
    if (p == m_settingsPage)
        return;
    m_settingsPage = p;
    emit settingsPageChanged();
}

void App::setEntryPick(bool on)
{
    if (on == m_entryPick)
        return;
    m_entryPick = on;
    emit entryChanged();
}

QVariantList App::entryServersVariant() const
{
    QVariantList out;
    for (const Server &s : m_entryServers)
        out << s.toVariant();
    return out;
}

QVariantList App::speedHistory() const
{
    QFile f(speedFile());
    if (!f.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(f.readAll()).array().toVariantList();
}

void App::setTunCapable(bool on)
{
    if (on == m_tunCapable)
        return;
    m_tunCapable = on;
    emit tunCapableChanged();
}

void App::setPhase(int p)
{
    m_phase = p;
    if (p != 1)
        m_startGuard.stop();
    emit connectionChanged();
}

void App::setFetching(int n)
{
    const bool was = busy();
    m_fetching = std::max(0, n);
    if (was != busy())
        emit busyChanged();
}

Core *App::newCore(bool elevated)
{
    auto *c = new Core(m_settings.debugLog, elevated, this);
    connect(c, &Core::event, this, [this, c](const QJsonObject &e) {
        if (!m_closed && c == m_core)
            onEvent(e);
    });
    return c;
}

bool App::wantElevated()
{
    if (!Platform::windows() || m_settings.connMode != u"tun")
        return false;
    if (!m_winTun)
        m_winTun = Platform::checkWinTun();
    return m_winTun;
}

void App::restartCore() { restartCore(wantElevated()); }

void App::restartCore(bool elevated)
{
    if (m_phase == 2 || m_phase == 1 || m_blocked)
        disconnectVpn();
    if (m_core) {
        m_core->close();
        m_core->deleteLater();
    }
    m_core = newCore(elevated);
    m_stopping = m_resumeAfterStop = false;
}

void App::save() { Store::save(m_subs, m_settings); }

void App::changeSettings(const Settings &next)
{
    const bool theme = next.theme != m_settings.theme || next.wallpaperColors != m_settings.wallpaperColors || next.seedColor != m_settings.seedColor;
    const bool autoChanged = next.autoSelect != m_settings.autoSelect || next.autoInterval != m_settings.autoInterval
        || next.autoWhitelist != m_settings.autoWhitelist || next.reserveOnlyOnWhitelists != m_settings.reserveOnlyOnWhitelists;
    const bool healthChanged = next.healthCheck != m_settings.healthCheck || next.healthSec != m_settings.healthSec || next.autoSelect != m_settings.autoSelect;
    m_settings = next;
    if (autoChanged) {
        cancelAutoPick();
        autoSchedule();
    }
    if (healthChanged && m_phase == 2) {
        m_health.stop();
        if (next.healthCheck || next.autoSelect)
            m_health.start(std::clamp(next.healthSec, Settings::kHealthMinSec, 3600) * 1000);
    }
    save();
    if (theme)
        Theme::instance()->setMode(m_settings.theme, m_settings.wallpaperColors, m_settings.seedColor);
    emit settingsChanged();
}

void App::apply(const Settings &next, bool reconnect)
{
    if (reconnect)
        cancelAutoPick();
    const bool modeChanged = next.connMode != m_settings.connMode;
    changeSettings(next);
    if (Platform::windows() && modeChanged) {
        const bool resume = reconnect && (m_phase == 2 || m_phase == 1);
        restartCore();
        if (resume) {
            m_reconnectAfterReady = true;
            setPhase(1);
            m_startGuard.start();
        }
        return;
    }
    if (reconnect && m_phase == 2)
        connectVpn();
}

void App::onEvent(const QJsonObject &e)
{
    const QString ev = e.value("ev").toString();
    if (ev == u"ready") {
        const bool caps = e.value("tunCapable").toBool();
        setTunCapable(caps);
        if (caps && !Platform::windows())
            Platform::background<bool>(this, [] { return Platform::firewallReady(); }, [this](bool ok) { setTunCapable(ok); });
        m_coreVersion = e.value("version").toString();
        emit coreVersionChanged();
        m_core->sendQuiet(QJsonObject{{"cmd", "stats"}, {"on", m_statsOn}});
        m_core->request(QJsonObject{{"cmd", "caps"}}, [this](const QJsonObject &r) {
            m_hwidDefault = r.value("hwid").toString();
            emit hwidDefaultChanged();
        });
        m_core->request(QJsonObject{{"cmd", "smartRules"}}, [this](const QJsonObject &r) { m_smartRules = r.value("rules").toObject(); });
        if (std::exchange(m_reconnectAfterReady, false)) {
            m_phase = 0;
            connectVpn();
        }
        if (e.value("blocked").toBool() && m_phase != 1 && m_phase != 2) {
            m_blocked = true;
            setPhase(3);
        }
    } else if (ev == u"caps") {
        setTunCapable(e.value("tun").toBool());
    } else if (ev == u"failing") {
        onFailing();
    } else if (ev == u"started") {
        onStarted(e);
    } else if (ev == u"stopped") {
        onStopped();
    } else if (ev == u"error") {
        onError(e);
    } else if (ev == u"stats") {
        const qint64 up = qint64(e.value("up").toDouble()), down = qint64(e.value("down").toDouble());
        m_skipSpeed = false;
        m_upload = up;
        m_download = down;
        emit statsChanged();
    } else if (ev == u"ping") {
        if (!m_pinging || quint64(e.value("generation").toDouble()) != m_pingGeneration) return;
        const QString name = e.value("name").toString();
        Ping p;
        if (e.contains("ms"))
            p.ms = e.value("ms").toInt();
        p.error = e.value("error").toString().trimmed();
        if (m_pingSub && m_pingSub != m_settings.sub)
            m_pingsBySub[*m_pingSub][name] = p;
        else
            m_pings[name] = p;
        m_waiting.removeAll(name);
        if (!m_pingFlush.isActive())
            m_pingFlush.start();
    } else if (ev == u"pingDone") {
        if (!m_pinging || quint64(e.value("generation").toDouble()) != m_pingGeneration) return;
        m_pingFlush.stop();
        m_pinging = false;
        m_waiting.clear();
        emit pingsChanged();
        if (m_settings.exitFlags && m_pingSub)
            askCountries(*m_pingSub);
    } else if (ev == u"speed") {
        if (m_speed.isEmpty())
            return;
        const int phase = e.value("phase").toInt();
        const double v = e.value("value").toDouble(), f = e.value("fraction").toDouble();
        const qint64 now = nowMillis();
        if (phase == m_speed.value("phase").toInt() && f < 1.0 && now - m_speedShown < 250)
            return;
        m_speedShown = now;
        m_speed["phase"] = phase;
        m_speed["value"] = v;
        m_speed["fraction"] = f;
        if (phase == 1 && v > 0)
            m_speed["ping"] = v;
        if (phase == 2 && f >= 1.0)
            m_speed["download"] = v;
        if (phase == 3 && f >= 1.0)
            m_speed["upload"] = v;
        emit speedChanged();
    } else if (ev == u"speedServer") {
        if (m_speed.isEmpty())
            return;
        const QJsonObject s = e.value("server").toObject();
        QStringList parts;
        for (const QString &k : {QStringLiteral("sponsor"), QStringLiteral("name")})
            if (!s.value(k).toString().trimmed().isEmpty())
                parts << s.value(k).toString();
        m_speed["target"] = parts.join(QStringLiteral(" · "));
        emit speedChanged();
    } else if (ev == u"speedDone") {
        if (m_speed.isEmpty())
            return;
        m_speed["running"] = false;
        const QJsonObject rep = e.value("report").toObject();
        if (rep.isEmpty()) {
            const QString err = e.value("error").toString().trimmed();
            m_speed["error"] = err.isEmpty() ? t("Не удалось замерить", "Couldn't measure") : err;
        } else {
            m_speed["phase"] = 4;
            m_speed["ping"] = rep.value("ping").toDouble();
            m_speed["jitter"] = rep.value("jitter").toDouble();
            m_speed["download"] = rep.value("download").toDouble();
            m_speed["upload"] = rep.value("upload").toDouble();
            addSpeedHistory();
        }
        emit speedChanged();
    } else if (ev == u"probe") {
        setNotice(e.contains("ms") ? t("Проверка: %1 мс", "Check: %1 ms").arg(e.value("ms").toInt()) : e.value("error").toString());
    } else if (ev == u"exited") {
        const bool reconnect = m_phase == 2 || m_phase == 1;
        cancelAutoPick();
        m_health.stop();
        ++m_epoch;
        m_pinging = false;
        m_waiting.clear();
        emit pingsChanged();
        proxyOff();
        const qint64 now = nowMillis();
        m_respawns.removeIf([now](qint64 at) { return now - at > 60000; });
        if (m_respawns.size() < 3) {
            m_respawns << now;
            m_reconnectAfterReady = reconnect;
            m_stopping = m_resumeAfterStop = m_restartAfterStop = false;
            const bool elevated = m_core->elevated();
            m_core->close();
            m_core->deleteLater();
            m_core = newCore(elevated);
            setNotice(t("Ядро перезапущено", "Core restarted"));
            if (m_reconnectAfterReady)
                setPhase(1);
            return;
        }
        m_failure = t("Ядро остановилось", "Core stopped");
        setPhase(3);
    }
}

void App::onStarted(const QJsonObject &e)
{
    if (m_stopping)
        return;
    const bool switched = m_phase == 2;
    m_failure.clear();
    m_blocked = false;
    m_connectedSub = m_pendingSub;
    m_connectedProfile = m_pendingProfile;
    m_connectedName = m_pendingProfile.value_or(QString());
    m_connectedServer = m_pendingServer;
    if (!switched || m_since == 0)
        m_since = nowMillis();
    ++m_epoch;
    const bool proxy = e.value("mode").toString() == u"proxy";
    const int port = e.value("port").toInt();
    m_activeOptions = m_pendingOptions;
    m_checkingHealth = false;
    m_health.stop();
    if (m_settings.healthCheck || m_settings.autoSelect) {
        m_health.setInterval(std::clamp(m_settings.healthSec, Settings::kHealthMinSec, 3600) * 1000);
        m_health.start();
    }
    setPhase(2);
    autoSchedule();
    lookupExit(proxy, port);
    proxyOff();
    if (proxy && m_settings.systemProxy && SysProxy::available())
        proxyOn(port);
}

void App::onStopped()
{
    cancelAutoPick();
    const bool restart = m_restartAfterStop;
    if (restart) {
        m_restartAfterStop = false;
        QTimer::singleShot(0, this, [this] { restartCore(); });
    }
    ++m_epoch;
    m_health.stop();
    m_stopping = false;
    m_blocked = false;
    clearConnection();
    proxyOff();
    setPhase(0);
    if (m_resumeAfterStop) {
        m_resumeAfterStop = false;
        if (!restart)
            connectVpn();
    }
}

void App::clearConnection()
{
    m_connectedSub.reset();
    m_connectedProfile.reset();
    m_connectedName.clear();
    m_connectedServer.clear();
    m_since = 0;
    if (!m_exitIp.isEmpty() || !m_exitFlag.isEmpty()) {
        m_exitIp.clear();
        m_exitFlag.clear();
        emit exitChanged();
    }
}

void App::onError(const QJsonObject &e)
{
    const QString message = e.value("message").toString();
    if (e.value("elevatedFailed").toBool()) {
        setTunCapable(false);
        setNotice(message);
        m_winTun = false;
        QTimer::singleShot(0, this, [this] {
            if (m_core) {
                m_core->close();
                m_core->deleteLater();
            }
            m_core = newCore(false);
        });
        return;
    }
    if (m_stopping)
        return;
    m_blocked = e.value("blocked").toBool();
    if (e.value("kept").toBool()) {
        if (m_connectedSub) {
            Settings next = m_settings;
            next.sub = m_connectedSub;
            next.profile = m_connectedProfile;
            changeSettings(next);
            loadServers();
        }
        setNotice(message);
        setPhase(2);
        autoSchedule();
        return;
    }
    m_health.stop();
    ++m_epoch;
    cancelAutoPick();
    m_failure = message;
    clearConnection();
    proxyOff();
    setPhase(3);
    if (m_settings.notifications && !m_focused)
        notify(QStringLiteral("ryNet"), message);
}

QList<Server> App::parseServers(const QJsonObject &reply, const QString &sub) const
{
    QList<Server> out;
    for (const QJsonValue &v : reply.value("profiles").toArray())
        out << Server::from(v.toObject());
    if (m_settings.exitFlags)
        flagByCountry(out, countriesOf(countriesFile(), sub, endpointsOf(out)));
    return m_settings.splitSubs.contains(sub) ? split(out) : out;
}

void App::askCountries(const QString &sub)
{
    const auto body = Store::body(sub);
    if (!body)
        return;
    QJsonObject opts;
    try {
        opts = options();
    } catch (const Fail &) {
        return;
    }
    opts.remove("servers");
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, sub, text = *body, opts](const QJsonObject &d) {
        QList<Server> list;
        for (const QJsonValue &v : d.value("profiles").toArray())
            list << Server::from(v.toObject());
        const QHash<QString, QString> endpoints = endpointsOf(list);
        const QHash<QString, QString> known = countriesOf(countriesFile(), sub, endpoints);
        const QHash<QString, Ping> &pings = sub == m_settings.sub.value_or(QString()) ? m_pings : m_pingsBySub[sub];
        QStringList names;
        for (auto it = pings.cbegin(); it != pings.cend(); ++it)
            if (it->ms && !known.contains(it.key()) && endpoints.contains(it.key()))
                names << it.key();
        if (names.isEmpty())
            return;
        m_core->request(QJsonObject{{"cmd", "countries"},
                                    {"text", text},
                                    {"names", strings(names)},
                                    {"parallel", m_settings.pingParallel},
                                    {"timeoutMs", m_settings.pingTimeout * 1000},
                                    {"tune", opts}},
                        [this, sub, endpoints](const QJsonObject &r) {
                            QHash<QString, QString> found;
                            const QJsonObject c = r.value("countries").toObject();
                            for (auto it = c.begin(); it != c.end(); ++it)
                                found.insert(it.key(), it.value().toString().toUpper());
                            putCountries(sub, found, endpoints);
                        });
    });
}

void App::putCountries(const QString &sub, const QHash<QString, QString> &found, const QHash<QString, QString> &endpoints)
{
    if (found.isEmpty() || !m_settings.exitFlags)
        return;
    QJsonObject all = countriesFile();
    QJsonObject o = all.value(sub).toObject();
    const qint64 now = nowMillis();
    QHash<QString, QString> stored;
    for (auto it = found.cbegin(); it != found.cend(); ++it) {
        const auto ep = endpoints.constFind(it.key());
        if (ep == endpoints.constEnd())
            continue;
        o[it.key()] = QJsonObject{{"c", it.value()}, {"e", *ep}, {"at", double(now)}};
        stored.insert(it.key(), it.value());
    }
    if (stored.isEmpty())
        return;
    for (const QString &k : o.keys()) {
        const qint64 age = now - qint64(o.value(k).toObject().value("at").toDouble());
        if (age < 0 || age >= kCountryTtl)
            o.remove(k);
    }
    all[sub] = o;
    Store::write(countriesPath(), QJsonDocument(all).toJson(QJsonDocument::Compact));
    if (sub == currentSubId()) {
        flagByCountry(m_servers, stored);
        emit serversChanged();
    }
}

void App::setStats(bool on)
{
    if (on == m_statsOn)
        return;
    m_statsOn = on;
    m_core->sendQuiet(QJsonObject{{"cmd", "stats"}, {"on", on}});
}
