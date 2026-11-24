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

QStringList App::processes() const { return Platform::processes(); }

int App::extraRuleLines() const
{
    int n = 0;
    for (const QString &l : m_settings.userRules.split(u'\n'))
        if (!l.trimmed().isEmpty() && !l.trimmed().startsWith(u'#'))
            ++n;
    return n;
}

void App::openSpeed(const QString &remarks)
{
    const Server *s = byRemarks(m_servers, remarks);
    if (!s)
        return;
    m_speed = {{"profile", remarks}, {"name", s->name}, {"kind", s->kind}, {"running", false}, {"phase", 0}, {"value", 0.0}, {"fraction", 0.0}};
    emit speedChanged();
}

void App::startSpeed()
{
    if (m_speed.isEmpty())
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
    const QString profile = m_speed.value("profile").toString();
    m_speed = {{"profile", profile}, {"name", m_speed.value("name")}, {"kind", m_speed.value("kind")}, {"running", true}, {"phase", 0}, {"value", 0.0}, {"fraction", 0.0}};
    emit speedChanged();
    m_core->send(QJsonObject{{"cmd", "speed"}, {"text", *body}, {"profile", profile}, {"options", opts}});
}

void App::stopSpeed()
{
    if (m_core)
        m_core->send(QJsonObject{{"cmd", "speedCancel"}});
}

void App::closeSpeed()
{
    if (m_speed.value("running").toBool())
        stopSpeed();
    m_speed.clear();
    emit speedChanged();
}

void App::clearSpeedHistory()
{
    QFile::remove(speedFile());
    closeSpeed();
}

void App::addSpeedHistory()
{
    QJsonArray all = QJsonArray::fromVariantList(speedHistory());
    all.prepend(QJsonObject{{"at", nowMillis()},
                            {"name", m_speed.value("name").toString()},
                            {"ping", m_speed.value("ping").toDouble()},
                            {"download", m_speed.value("download").toDouble()},
                            {"upload", m_speed.value("upload").toDouble()}});
    while (all.size() > 20)
        all.removeLast();
    Store::write(speedFile(), QJsonDocument(all).toJson(QJsonDocument::Compact));
}

void App::pickFile(bool save, const QString &name, const QString &title, const QString &ext, const QJSValue &done)
{
    QJSValue callback = done;
    QPointer<QJSEngine> engine = qjsEngine(this);
    FilePick::pick(this, save, name, title, ext, [callback, engine](const QString &path) mutable {
        if (engine && callback.isCallable())
            callback.call({QJSValue(path)});
    });
}

void App::plugins(const QString &op, const QVariantMap &arg, const QString &file, const QJSValue &done)
{
    QJSValue callback = done;
    QPointer<QJSEngine> engine = qjsEngine(this);
    auto answer = [callback, engine](const QVariantMap &r) mutable {
        if (engine && callback.isCallable())
            callback.call({engine->toScriptValue(r)});
    };
    QJsonObject cmd{{"cmd", "plugins"}, {"op", op}, {"arg", QJsonObject::fromVariantMap(arg)}};
    if (!file.isEmpty()) {
        QFile f(file);
        if (!f.open(QIODevice::ReadOnly) || f.size() > (16 << 20)) {
            QTimer::singleShot(0, this, [answer, err = t("Файл не читается или слишком большой", "The file can't be read or is too large")]() mutable {
                answer({{QStringLiteral("error"), err}});
            });
            return;
        }
        cmd.insert("data", QString::fromLatin1(f.readAll().toBase64()));
    }
    m_core->request(cmd, [answer](const QJsonObject &r) mutable { answer(r.toVariantMap()); });
}

QVariantList App::backupParts() const
{
    QVariantList out;
    for (const Backup::Part &p : Backup::parts())
        if (p.key != u"subs" || !m_subs.isEmpty())
            out << QVariantMap{{"key", p.key}, {"title", Lang::ru() ? p.ru : p.en}};
    return out;
}

QVariantList App::backupFileParts(const QString &file)
{
    QVariantList out;
    try {
        QFile f(file);
        if (f.size() > 32LL * 1024 * 1024)
            throw Fail(t("Резервная копия слишком большая", "The backup is too large"));
        if (!f.open(QIODevice::ReadOnly))
            throw Fail(t("Не удалось открыть файл", "Couldn't open the file"));
        int subs = 0;
        const QStringList keys = Backup::partsIn(f.readAll(), &subs);
        for (const Backup::Part &p : Backup::parts())
            if (keys.contains(p.key))
                out << QVariantMap{{"key", p.key},
                                   {"title", Lang::ru() ? p.ru : p.en},
                                   {"summary", p.key == u"subs" ? plural(subs, "подписка", "подписки", "подписок", "subscription") : QString()}};
        if (out.isEmpty())
            throw Fail(t("В резервной копии ничего нет", "The backup is empty"));
    } catch (const Fail &f) {
        setNotice(f.text());
        out.clear();
    }
    return out;
}

void App::exportBackup(const QString &file, const QStringList &parts)
{
    if (file.isEmpty() || parts.isEmpty())
        return;
    try {
        const QByteArray bytes = Backup::encode(m_settings, m_subs, [](const QString &id) { return Store::body(id); }, parts);
        if (!Store::write(file, bytes))
            throw Fail(t("Не удалось записать файл", "Couldn't write the file"));
        setNotice(t("Резервная копия сохранена", "Backup saved"));
    } catch (const Fail &f) {
        setNotice(f.text());
    }
}

bool App::canImport()
{
    if (m_phase == 2 || m_phase == 1 || m_stopping || m_blocked || busy()) {
        setNotice(t("Сначала отключите VPN и дождитесь обновления подписок", "Turn off the VPN first and wait for subscriptions to update"));
        return false;
    }
    return true;
}

void App::importBackup(const QString &file, const QStringList &parts)
{
    if (file.isEmpty() || parts.isEmpty() || !canImport())
        return;
    try {
        QFile f(file);
        if (f.size() > 32LL * 1024 * 1024)
            throw Fail(t("Резервная копия слишком большая", "The backup is too large"));
        if (!f.open(QIODevice::ReadOnly))
            throw Fail(t("Не удалось открыть файл", "Couldn't open the file"));
        Backup::Snapshot snap = Backup::decode(f.readAll(), m_settings, parts);
        for (const auto &[sub, body] : std::as_const(snap.subs))
            Store::saveBody(sub.id, body);
        QList<Sub> subs;
        for (const Sub &old : std::as_const(m_subs))
            if (std::none_of(snap.subs.begin(), snap.subs.end(), [&](const auto &p) { return p.first.id == old.id; }))
                subs << old;
        for (const auto &p : std::as_const(snap.subs))
            subs << p.first;
        m_subs = subs;
        if (Platform::windows() && snap.settings.pingMode == u"icmp")
            snap.settings.pingMode = "tcp";
        if (parts.contains(QStringLiteral("subs"))) {
            m_pings.clear();
            emit pingsChanged();
        }
        changeSettings(snap.settings);
        emit subsChanged();
        loadServers();
        setNotice(t("Резервная копия загружена", "Backup restored"));
    } catch (const Fail &f) {
        setNotice(f.text());
    }
}

void App::copy(const QString &text) { QGuiApplication::clipboard()->setText(text); }

QString App::clipboard() const { return QGuiApplication::clipboard()->text(); }

void App::open(const QString &target) { Platform::open(target); }

QString App::dataDir() const { return Store::dir(); }

QString App::logFile() const { return Store::path(QStringLiteral("core.log")); }

QString App::diagName() const
{
    return QStringLiteral("rynet-diag-%1.zip").arg(QDateTime::currentDateTime().toString(QStringLiteral("yyyy-MM-dd-HHmm")));
}

void App::exportDiag(const QString &file)
{
    if (file.isEmpty())
        return;
    QFile log(logFile());
    const QByteArray text = log.open(QIODevice::ReadOnly) ? log.readAll() : QByteArray();
    const Server *s = byRemarks(m_servers, m_settings.profile.value_or(QString()));
    const QJsonObject info{{"app", Platform::version()},
                           {"device", QStringLiteral("%1 %2 %3").arg(QSysInfo::prettyProductName(), QSysInfo::kernelVersion(), QSysInfo::currentCpuArchitecture())},
                           {"connMode", m_settings.connMode},
                           {"routing", m_settings.routing},
                           {"pingMode", m_settings.pingMode},
                           {"userRules", m_settings.effectiveRules()},
                           {"profile", m_settings.profile.value_or(QString())},
                           {"kind", s ? s->kind : QString()},
                           {"transport", s ? s->description : QString()},
                           {"subscriptions", int(m_subs.size())}};
    const QByteArray zip = Reports::zip({{QStringLiteral("core.log"), text}, {QStringLiteral("info.json"), QJsonDocument(info).toJson(QJsonDocument::Indented)}});
    if (Store::write(file, zip))
        setNotice(t("Журнал сохранён: %1", "Log saved: %1").arg(QFileInfo(file).fileName()));
    else
        setNotice(t("Не удалось записать файл", "Couldn't write the file"));
}

void App::writeText(const QString &file, const QString &text, const QString &doneNotice)
{
    if (file.isEmpty())
        return;
    if (Store::write(file, text.toUtf8()))
        setNotice(doneNotice);
    else
        setNotice(t("Не удалось записать файл", "Couldn't write the file"));
}

QString App::readText(const QString &file, int maxBytes) const
{
    QFile f(file);
    if (f.size() > maxBytes || !f.open(QIODevice::ReadOnly))
        return {};
    return QString::fromUtf8(f.readAll()).trimmed();
}

QString App::uptime(double sinceMs) const
{
    const qint64 s = std::max<qint64>(0, (nowMillis() - qint64(sinceMs)) / 1000);
    if (s >= 86400)
        return Lang::ru() ? QStringLiteral("%1 д %2 ч").arg(s / 86400).arg(s / 3600 % 24) : QStringLiteral("%1 d %2 h").arg(s / 86400).arg(s / 3600 % 24);
    if (s >= 3600)
        return QStringLiteral("%1:%2:%3").arg(s / 3600).arg(s / 60 % 60, 2, 10, QChar(u'0')).arg(s % 60, 2, 10, QChar(u'0'));
    return QStringLiteral("%1:%2").arg(s / 60).arg(s % 60, 2, 10, QChar(u'0'));
}

QString App::bytes(double b) const
{
    if (b < 1024)
        return QStringLiteral("%1 %2").arg(qint64(b)).arg(t("Б", "B"));
    if (b < 1024 * 1024)
        return QStringLiteral("%1 %2").arg(decimal(b / 1024.0, 1), t("КБ", "KB"));
    if (b < 1024.0 * 1024 * 1024)
        return QStringLiteral("%1 %2").arg(decimal(b / 1048576.0, 1), t("МБ", "MB"));
    return QStringLiteral("%1 %2").arg(decimal(b / 1073741824.0, 2), t("ГБ", "GB"));
}

QString App::ago(double seconds) const
{
    const qint64 s = qint64(seconds);
    if (s < 60)
        return t("только что", "just now");
    if (s < 3600)
        return t("%1 мин назад", "%1 min ago").arg(s / 60);
    if (s < 86400)
        return t("%1 ч назад", "%1 h ago").arg(s / 3600);
    return t("%1 дн. назад", "%1 d ago").arg(s / 86400);
}

QString App::mbps(double bps) const
{
    const double m = bps / 1'000'000;
    return m >= 100 ? QString::number(qRound64(m)) : decimal(m, 1);
}

QString App::plural(int n, const QString &one, const QString &few, const QString &many, const QString &en, const QString &enMany) const
{
    const QByteArray a = one.toUtf8(), b = few.toUtf8(), c = many.toUtf8(), d = en.toUtf8(), e = enMany.toUtf8();
    return ::plural(n, a.constData(), b.constData(), c.constData(), d.constData(), enMany.isEmpty() ? nullptr : e.constData());
}

QString App::backupName() const { return QStringLiteral("ryNet-%1.rynet").arg(QDate::currentDate().toString(Qt::ISODate)); }

void App::quit() { emit quitRequested(); }

void App::notify(const QString &title, const QString &text) { Platform::notify(title, text); }

void App::updateTray()
{
    if (!m_tray)
        return;
    QStringList servers;
    for (const Server &s : std::as_const(m_servers))
        if (s.supported && !s.placeholder)
            servers << s.remarks;
    m_tray->update(m_phase == 2 && !m_blocked, m_phase == 1, statusTitle(), m_connectedName.isEmpty() ? m_settings.profile.value_or(QString()) : m_connectedName,
                   servers, m_settings.profile.value_or(QString()));
}

void App::expiryAlerts()
{
    if (!m_settings.notifications)
        return;
    const qint64 now = nowSeconds();
    for (const Sub &s : std::as_const(m_subs)) {
        if (s.expire <= now)
            continue;
        const qint64 days = (s.expire - now) / 86400;
        if (days > 3)
            continue;
        const QString when = Lang::ru() ? (days == 0 ? QStringLiteral("день") : QStringLiteral("%1 дн.").arg(days))
                                        : (days <= 1 ? QStringLiteral("a day") : QStringLiteral("%1 days").arg(days));
        Platform::notifyOnce(QStringLiteral("expire-%1-%2").arg(s.id).arg(days), QStringLiteral("ryNet"),
                             t("Подписка «%1» закончится через %2", "Subscription \"%1\" expires in %2").arg(s.name(), when));
    }
}

void App::clockTick()
{
    const qint64 now = nowMillis();
    if (now - m_tickAt > m_report.interval() + kSleptMs)
        m_netChange.start();
    m_tickAt = now;
    autoUpdateSubs();
    updateTick();
}

void App::prepareForSleep(bool sleeping)
{
    if (!sleeping)
        m_netChange.start();
}

void App::networkChanged()
{
    if (m_core && m_phase == 2)
        m_core->sendQuiet(QJsonObject{{"cmd", "network"}});
    autoUpdateSubs();
}

void App::autoUpdateSubs()
{
    if (!m_settings.autoUpdate || m_closed)
        return;
    const QNetworkInformation *n = QNetworkInformation::instance();
    if (n && n->reachability() == QNetworkInformation::Reachability::Disconnected)
        return;
    const qint64 now = nowSeconds();
    for (const Sub &s : std::as_const(m_subs)) {
        if (now - s.updated <= qint64(std::max(1, s.intervalHours.value_or(24))) * 3600 || now - m_subTried.value(s.id) < kSubRetrySec)
            continue;
        m_subTried[s.id] = now;
        refresh(s.id);
    }
}

void App::reportTick()
{
    if (m_phase != 2 || !m_settings.sendLogs || !m_settings.debugLog || m_reporting)
        return;
    const bool due = Reports::worked(60000, Store::dir());
    const qint64 now = nowMillis();
    if (now < m_reportRetryAt || !Reports::allowed(Reports::state(Store::dir()), now))
        return;
    if (!due && Reports::unsent(Store::dir()) < Reports::SOON_BYTES)
        return;
    m_reporting = true;
    QString kind;
    if (const Server *s = byRemarks(m_servers, m_connectedProfile.value_or(QString())))
        kind = s->kind;
    const QJsonObject info{{"app", Platform::version()},
                           {"device", QStringLiteral("%1 %2 %3").arg(QSysInfo::prettyProductName(), QSysInfo::kernelVersion(), QSysInfo::currentCpuArchitecture())},
                           {"routing", m_settings.routing},
                           {"kind", kind},
                           {"subscriptions", int(m_subs.size())}};
    Reports::send(m_net, info, Store::dir(), Endpoints::Api + QStringLiteral("/v1/reports"), [this](bool ok) {
        m_reporting = false;
        if (!ok)
            m_reportRetryAt = nowMillis() + Reports::RETRY_MS;
    });
}

QVariantMap App::updateVariant() const
{
    QVariantMap m{{"state", m_updateChecking ? "checking" : m_update ? "found" : m_updateNone ? "none" : "idle"}};
    if (m_update) {
        m["version"] = m_update->version;
        m["changelog"] = m_update->changelog;
        m["post"] = m_update->post;
        m["file"] = m_update->file;
        m["sha256"] = m_update->sha256;
    }
    return m;
}

void App::getLimited(const QString &url, qsizetype limit, std::function<void(std::optional<QByteArray>)> done)
{
    QNetworkRequest req{QUrl(url)};
    req.setHeader(QNetworkRequest::UserAgentHeader, ownAgent());
    req.setRawHeader("Accept", "application/json");
    req.setAttribute(QNetworkRequest::CookieLoadControlAttribute, QNetworkRequest::Manual);
    req.setAttribute(QNetworkRequest::CookieSaveControlAttribute, QNetworkRequest::Manual);
    req.setAttribute(QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::NoLessSafeRedirectPolicy);
    req.setTransferTimeout(20000);
    QNetworkReply *reply = m_net->get(req);
    connect(reply, &QNetworkReply::downloadProgress, reply, [reply, limit](qint64 got, qint64) {
        if (got > limit)
            reply->abort();
    });
    connect(reply, &QNetworkReply::finished, this, [reply, limit, done = std::move(done)] {
        reply->deleteLater();
        const int code = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
        if (reply->error() != QNetworkReply::NoError || code != 200) {
            done({});
            return;
        }
        const QByteArray body = reply->readAll();
        done(body.size() <= limit ? std::optional<QByteArray>(body) : std::nullopt);
    });
}

void App::updateTick()
{
    if (!m_settings.updateCheck || m_updateChecking || m_closed)
        return;
    const qint64 now = nowMillis();
    if ((now >= m_updateAt && now - m_updateAt < Update::EVERY_MS) || now < m_updateRetryAt)
        return;
    const QNetworkInformation *n = QNetworkInformation::instance();
    if (n && n->reachability() == QNetworkInformation::Reachability::Disconnected)
        return;
    updateCheck(false);
}

void App::checkUpdate() { updateCheck(true); }

void App::updateCheck(bool manual)
{
    if (m_updateChecking || m_closed)
        return;
    m_updateChecking = true;
    m_updateNone = false;
    emit updateChanged();
    getLimited(Update::releasesUrl(), Update::MAX_RELEASES, [this, manual](std::optional<QByteArray> body) {
        if (m_closed)
            return;
        if (!body) {
            updateOutcome(manual, true);
            return;
        }
        m_updateAt = nowMillis();
        const auto release = Update::parseRelease(*body);
        if (!release || !Update::newer(Platform::version(), release->version)) {
            m_update.reset();
            m_updateNone = true;
            updateOutcome(manual, false);
            return;
        }
        const auto show = [this, manual, release](const std::optional<Update::Download> &file) {
            const bool fresh = !m_update || m_update->version != release->version;
            Update::Found f{release->version, release->changelog, release->url, {}, Update::pageUrl(), {}};
            if (file) {
                f.file = file->file;
                f.link = file->link;
                f.sha256 = file->sha256;
            }
            m_update = f;
            updateOutcome(manual, false);
            if (fresh && !manual && m_settings.notifications)
                Platform::notifyOnce(QStringLiteral("update-%1").arg(f.version), QStringLiteral("ryNet"), t("Доступна версия %1", "Version %1 is available").arg(f.version));
        };
        const auto withInstall = [this, release, show](Update::Install install) {
            if (Update::alias(install).isEmpty()) {
                show({});
                return;
            }
            getLimited(Update::downloadsUrl(), Update::MAX_DOWNLOADS, [this, release, show, install](std::optional<QByteArray> list) {
                if (m_closed)
                    return;
                show(list ? Update::parseDownload(*list, install, release->version) : std::nullopt);
            });
        };
        if (m_install) {
            withInstall(*m_install);
            return;
        }
        Platform::background<int>(
            this, [] { return int(Update::detect()); },
            [this, withInstall](int install) {
                m_install = Update::Install(install);
                withInstall(*m_install);
            });
    });
}

void App::updateOutcome(bool manual, bool failed)
{
    m_updateChecking = false;
    if (failed) {
        m_updateRetryAt = nowMillis() + Update::RETRY_MS;
    } else {
        m_updateRetryAt = 0;
        QJsonObject o{{"checkedAt", double(m_updateAt)}};
        o["found"] = m_update ? m_update->toJson() : QJsonObject();
        Store::write(updateFile(), QJsonDocument(o).toJson(QJsonDocument::Compact));
    }
    emit updateChanged();
    if (manual && failed)
        setNotice(t("Не удалось проверить обновления", "Couldn't check for updates"));
    else if (manual && !m_update)
        setNotice(t("Установлена последняя версия", "You have the latest version"));
}

void App::openUpdate()
{
    if (m_update)
        Platform::open(m_update->link);
}

void App::openUpdatePost()
{
    if (m_update && !m_update->post.isEmpty())
        Platform::open(m_update->post);
}

void App::copyLinks(const QString &id)
{
    const auto put = [this](const QList<Server> &list) {
        QStringList links;
        const std::function<void(const Server &)> take = [&](const Server &s) {
            if (!s.link.isEmpty() && !links.contains(s.link))
                links << s.link;
            for (const Server &m : s.members)
                take(m);
        };
        for (const Server &s : list)
            take(s);
        if (links.isEmpty()) {
            setNotice(t("Нет серверов для копирования", "No servers to copy"));
            return;
        }
        copy(links.join(QLatin1Char('\n')));
        setNotice(t("Скопировано серверов: %1", "Servers copied: %1").arg(links.size()));
    };
    const auto cur = current();
    if (cur && cur->id == id && !m_servers.isEmpty()) {
        put(m_servers);
        return;
    }
    const auto body = Store::body(id);
    if (!body || !m_core) {
        put({});
        return;
    }
    m_core->request(QJsonObject{{"cmd", "describe"}, {"text", *body}}, [this, id, put](const QJsonObject &r) { put(parseServers(r, id)); });
}

void App::removeDead()
{
    const auto body = Store::body(Local::ID);
    if (!body)
        return;
    const QHash<QString, Ping> &results = m_settings.sub == Local::ID ? m_pings : m_pingsBySub.value(Local::ID);
    QStringList dead;
    for (auto it = results.begin(); it != results.end(); ++it)
        if (!it->ms || *it->ms < 0)
            dead << it.key();
    QString next = *body;
    int gone = 0;
    for (const QString &r : std::as_const(dead))
        if (const auto less = Local::without(next, r); less && *less != next) {
            next = *less;
            ++gone;
        } else if (!less) {
            ++gone;
            next.clear();
            break;
        }
    if (gone == 0) {
        setNotice(t("Нерабочих серверов нет", "No non-working servers"));
        return;
    }
    if (m_connectedSub == Local::ID && m_connectedProfile && dead.contains(*m_connectedProfile))
        disconnectVpn();
    if (next.isEmpty()) {
        remove(Local::ID);
    } else {
        Store::saveBody(Local::ID, next);
        Settings s = m_settings;
        for (const QString &r : std::as_const(dead))
            s = s.pin(Local::ID, r, false);
        changeSettings(s);
        if (m_settings.sub == Local::ID)
            loadServers();
    }
    setNotice(t("Удалено серверов: %1", "Servers deleted: %1").arg(gone));
}

namespace {
QString noticeKey(const QString &sub, const QString &text)
{
    return sub + QLatin1Char(':') + QString::fromLatin1(QCryptographicHash::hash(text.trimmed().toUtf8(), QCryptographicHash::Sha1).left(8).toHex());
}
}

bool App::noticeHidden(const QString &sub, const QString &text) const { return m_settings.dismissedNotices.contains(noticeKey(sub, text)); }

void App::dismissNotice(const QString &sub, const QString &text)
{
    const QString key = noticeKey(sub, text);
    if (m_settings.dismissedNotices.contains(key))
        return;
    Settings next = m_settings;
    next.dismissedNotices << key;
    while (next.dismissedNotices.size() > 200)
        next.dismissedNotices.removeFirst();
    changeSettings(next);
}

bool App::geoMissing() const
{
    if (m_settings.routing != u"smart")
        return false;
    QString dir = Store::portableDir();
    if (dir.isEmpty()) {
#ifdef Q_OS_WIN
        dir = qEnvironmentVariable("LOCALAPPDATA");
#else
        dir = qEnvironmentVariable("XDG_CACHE_HOME");
        if (dir.isEmpty())
            dir = QDir::homePath() + QStringLiteral("/.cache");
#endif
        dir += QStringLiteral("/rynet");
    }
    const QDir geo(dir + QStringLiteral("/geo"));
    for (const char *f : {"geosite-category-ru.list", "geoip-ru.list"})
        if (QFileInfo(geo.filePath(QLatin1String(f))).size() <= 0)
            return true;
    return false;
}
