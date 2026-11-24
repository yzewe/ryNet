#pragma once

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

namespace AppDetail {

inline int pingCode(const QString &mode)
{
    if (mode == u"tcp")
        return 0;
    if (mode == u"icmp")
        return 2;
    if (mode == u"proxy2")
        return 3;
    return 1;
}

inline QString flagOf(const QString &country)
{
    const QString c = country.toUpper();
    if (c.size() != 2 || !c[0].isLetter() || !c[1].isLetter() || c[0].unicode() > 'Z' || c[1].unicode() > 'Z')
        return {};
    char32_t cps[2] = {char32_t(0x1F1E6 + (c[0].unicode() - 'A')), char32_t(0x1F1E6 + (c[1].unicode() - 'A'))};
    return QString::fromUcs4(cps, 2);
}

inline QJsonArray strings(const QStringList &l) { return QJsonArray::fromStringList(l); }

inline constexpr qint64 kCountryTtl = 7LL * 24 * 3600 * 1000;
inline QString countriesPath() { return Store::path(QStringLiteral("countries.json")); }

inline QJsonObject countriesFile()
{
    QFile f(countriesPath());
    return f.open(QIODevice::ReadOnly) ? QJsonDocument::fromJson(f.readAll()).object() : QJsonObject();
}

inline QString endpointOf(const Server &p)
{
    return p.server;
}

inline QHash<QString, QString> endpointsOf(const QList<Server> &list)
{
    QHash<QString, QString> out;
    for (const Server &p : list) {
        out.insert(p.remarks, endpointOf(p));
        for (const Server &m : p.members)
            out.insert(m.remarks, endpointOf(m));
    }
    return out;
}

inline QHash<QString, QString> countriesOf(const QJsonObject &all, const QString &sub, const QHash<QString, QString> &endpoints)
{
    QHash<QString, QString> out;
    const QJsonObject o = all.value(sub).toObject();
    const qint64 now = nowMillis();
    for (auto it = o.begin(); it != o.end(); ++it) {
        const QJsonObject e = it.value().toObject();
        const qint64 age = now - qint64(e.value("at").toDouble());
        const auto ep = endpoints.constFind(it.key());
        if (age >= 0 && age < kCountryTtl && e.value("c").toString().size() == 2 && ep != endpoints.constEnd() && e.value("e").toString() == *ep)
            out.insert(it.key(), e.value("c").toString());
    }
    return out;
}

inline void flagByCountry(QList<Server> &list, const QHash<QString, QString> &known)
{
    for (Server &p : list) {
        if (const auto c = known.constFind(p.remarks); c != known.constEnd())
            p.flag = flagOf(*c);
        flagByCountry(p.members, known);
    }
}

inline QString decimal(double v, int digits)
{
    return Lang::ru() ? QLocale(QLocale::Russian).toString(v, 'f', digits) : QString::number(v, 'f', digits);
}

inline QVariantMap pingVariant(const Ping &p)
{
    QVariantMap m;
    if (p.ms)
        m["ms"] = *p.ms;
    if (!p.error.isEmpty())
        m["error"] = p.error;
    return m;
}

inline QString speedFile() { return Store::path(QStringLiteral("speedtests.json")); }

inline const QString kCarryUrl = QStringLiteral("https://www.google.com/#body=32768&within=3000");
inline const QString kCarryAlt = Endpoints::SpeedDown + QStringLiteral("32768#body=32768&within=3000");
inline constexpr int kProbeMs = 3500;
inline constexpr int kRaceMs = 700;
inline constexpr int kWhitelistMs = 5 * 60000;
inline constexpr qint64 kDeepMs = 60 * 60000;
inline constexpr int kNetMs = 4000;
inline constexpr qint64 kSubRetrySec = 30 * 60;
inline constexpr qint64 kSleptMs = 60000;

inline const QString &ownAgent()
{
    static const QString ours = QStringLiteral("ryNet/%1 (%2)").arg(
        Platform::version(), Platform::windows() ? QStringLiteral("Windows ") + QSysInfo::productVersion() : QStringLiteral("Linux ") + QSysInfo::kernelVersion());
    return ours;
}

inline QString autoFile() { return Store::path(QStringLiteral("auto-state.json")); }
inline QString updateFile() { return Store::path(QStringLiteral("update.json")); }
inline QString learnedFile() { return Store::path(QStringLiteral("auto-learned.json")); }

inline QJsonObject readJson(const QString &file)
{
    QFile f(file);
    return f.open(QIODevice::ReadOnly) ? QJsonDocument::fromJson(f.read(4 << 20)).object() : QJsonObject();
}

inline AutoSelect::Learned loadLearned()
{
    const QJsonObject o = readJson(learnedFile());
    AutoSelect::Learned l;
    for (const QJsonValue &v : o.value("yes").toArray())
        l.yes << v.toString();
    for (const QJsonValue &v : o.value("no").toArray())
        l.no << v.toString();
    return l;
}

inline bool tooSlow(const QJsonObject &r)
{
    const QString e = r.value("error").toString();
    return e.startsWith(u"Слишком медленно") || e.startsWith(u"Too slow");
}

inline QString networkName()
{
    const QNetworkInformation *n = QNetworkInformation::instance();
    switch (n ? n->transportMedium() : QNetworkInformation::TransportMedium::Unknown) {
    case QNetworkInformation::TransportMedium::Ethernet:
        return QStringLiteral("Ethernet");
    case QNetworkInformation::TransportMedium::WiFi:
        return QStringLiteral("Wi-Fi");
    case QNetworkInformation::TransportMedium::Cellular:
        return t("мобильная", "mobile");
    default:
        return {};
    }
}

inline bool metered()
{
    const QNetworkInformation *n = QNetworkInformation::instance();
    return n && n->isMetered();
}
}

struct App::AutoRun {
    bool initial = false, failed = false, light = false, look = false, finished = false;
    Target current;
    QHash<QString, QString> bodies;
    QJsonObject tune;
    QString pingUrl;
    int timeout = 5;
    int describing = 0;
    AutoSelect::Learned learned;
    QHash<QString, QList<AutoSelect::Entry>> bySub;
    QList<AutoSelect::Entry> all;
    QHash<int, QString> errors;
    QList<AutoSelect::Probe> probes;
    int waiting = 0;
    AutoSelect::Net mode = AutoSelect::Net::Free;
    QString network;
    bool small = false;
    QList<int> carryLeft;
    std::function<bool(const AutoSelect::Entry &)> prefer;
};

struct App::AutoBatch {
    QList<int> queue;
    QString url;
    bool race = false, raced = false, over = false;
    int next = 0, active = 0;
    QHash<int, QJsonObject> got;
    std::function<void(const QHash<int, QJsonObject> &)> done;
};
