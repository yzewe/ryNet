#pragma once

#include "endpoints.h"

#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QMap>
#include <QPair>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <optional>
#include <stdexcept>

namespace Lang {
bool ru();
void setRu(bool on);
}

inline QString t(const char *ru, const char *en) { return QString::fromUtf8(Lang::ru() ? ru : en); }
QString plural(int n, const char *one, const char *few, const char *many, const char *en, const char *enMany = nullptr);

struct Fail : std::runtime_error {
    explicit Fail(const QString &m) : std::runtime_error(m.toStdString()) {}
    QString text() const { return QString::fromStdString(what()); }
};

using Target = std::optional<QPair<QString, QString>>;

struct Rule {
    QString kind = "domain";
    QString value;
    QString action = "proxy";
    Target server;

    std::optional<QString> mihomo(const std::optional<QString> &target = {}) const;
    bool valid() const;
    bool operator==(const Rule &o) const { return kind == o.kind && value == o.value && action == o.action && server == o.server; }
};

struct RuleSet {
    QString id;
    QString name;
    int shape = 0;
    int icon = 0;
    QString action = "proxy";
    Target server;
    bool enabled = true;
    QStringList domains, procs, ips, geosite;

    int size() const { return int(domains.size() + procs.size() + ips.size() + geosite.size()); }
    QStringList mihomo(const std::optional<QString> &target) const;
    bool operator==(const RuleSet &o) const;
};

namespace Rules {
QString actionTarget(const QString &action);
std::optional<QString> domain(const QString &input);
std::optional<QPair<QString, bool>> cidr(const QString &input);
bool validProcess(const QString &s);
QString mihomo(const QList<Rule> &rules, const QString &extra, const QList<RuleSet> &sets = {});
QList<QPair<QPair<QString, QString>, QString>> servers(const QList<Rule> &rules, const QList<RuleSet> &sets = {});
QJsonArray toJson(const QList<Rule> &rules);
QList<Rule> fromJson(const QJsonArray &a);
}

namespace RuleSets {
QString newId();
QJsonArray toJson(const QList<RuleSet> &sets);
QList<RuleSet> fromJson(const QJsonArray &a);
QString link(const RuleSet &s);
bool isLink(const QString &text);
QStringList parts(const QString &link, int size = 400);
std::optional<RuleSet> parse(const QString &text);
std::optional<QString> geosite(const QString &input);
QStringList split(const QString &text, bool lines = false);
QVariantMap toVariant(const RuleSet &s);
RuleSet fromVariant(const QVariantMap &m);
}

namespace SubscriptionRules {
QVariantMap parse(const std::optional<QString> &raw);
}

namespace ModeRules {
QVariantMap entry(const QString &kind, const QString &value, const QString &action);
QVariantMap domain(const QString &raw, const QString &action);
QVariantMap ip(const QString &raw, const QString &action);
QVariantList fromXray(const QJsonObject &o);
QVariantList subscription(const QVariantMap &parsed);
QVariantMap fromConfig(const QString &body, const QString &remarks);
}

namespace Fragment {
struct Preset { const char *ru; const char *en; const char *length; const char *interval; };
extern const Preset presets[3];
int of(const QString &length, const QString &interval);
bool validRange(const QString &v, int max);
}

bool validDns(const QString &address);

struct Settings {
    QString routing = "subscription";
    QString pingMode = "proxy";
    bool pingOnStart = false;
    QString sort = "default";
    QString view = "groups";
    QString grid = "2x1";
    QString serverInfo = "description";
    bool exitFlags = false;
    int pingParallel = 16;
    int pingTimeout = 5;
    QString pingUrl;
    QString ipUrl;
    bool fakeDns = false;
    bool ipv4Only = false;
    QString serverDns = "auto";
    QString serverDnsCustom;
    QString dnsServers;
    QMap<QString, QStringList> pins;
    QMap<QString, QMap<QString, QString>> perServerDns;
    QMap<QString, QString> lastServers;
    QString hwid;
    bool autoUpdate = true;
    bool sendHwid = true;
    bool refreshNotices = false;
    QString subClient = "rynet";
    QString subHeaders;
    QList<Rule> rules;
    QList<RuleSet> sets;
    QString userRules;
    bool autostart = false;
    QString theme = "system";
    QString font = "roboto";
    bool wallpaperColors = true;
    QString seedColor;
    bool debugLog = false;
    bool healthCheck = false;
    int healthSec = 300;
    static constexpr int kHealthMinSec = 3;
    static constexpr int kHealthSteps[] = {3, 5, 10, 15, 30, 60, 120, 300, 600, 3600};
    bool healthNotify = true;
    bool autoSwitch = false;
    bool autoSelect = false;
    bool autoFeature = false;
    QStringList splitSubs;
    QStringList dismissedNotices;
    QStringList autoSkipSubs;
    QMap<QString, QStringList> autoSkip;
    QStringList autoWlSubs;
    QMap<QString, QStringList> autoWl;
    bool autoPins = true;
    QStringList autoTransports;
    QString autoWhitelist = "auto";
    QString autoHome = QStringLiteral("yandex.ru, vk.ru, max.ru, gosuslugi.ru");
    QString autoFree = QStringLiteral("google.com, cloudflare.com");
    int autoInterval = 15;
    int autoMargin = 50;
    bool autoSpeed = true;
    bool autoSaveData = false;
    bool autoNotify = true;
    bool reserveOnlyOnWhitelists = false;
    bool sendLogs = false;
    bool updateCheck = true;
    bool killSwitch = false;
    QString processes;
    QString processesMode = "exclude";
    QString directPorts;
    bool notifications = true;
    int mtu = 0;
    bool fragment = false;
    QString fragmentLength = "100-200";
    QString fragmentInterval = "10-20";
    bool chain = false;
    QString chainSub;
    QString chainProfile;
    QString connMode = "tun";
    int port = 10808;
    bool systemProxy = true;
    int uiScale = 0;
    int sectionRadius = 16;
    QString sectionDivider = "gap";
    std::optional<QString> sub;
    std::optional<QString> profile;

    QString effectiveRules() const { return Rules::mihomo(rules, userRules, sets); }
    Target chainEntry() const;
    int tunMtu() const { return mtu >= 1280 && mtu <= 9000 ? mtu : 9000; }
    QStringList ownDns() const;
    QString serverDnsArg() const;
    QStringList dnsFor(const std::optional<QString> &sub, const std::optional<QString> &profile) const;
    QStringList pinned(const std::optional<QString> &sub) const;
    Settings pin(const QString &sub, const QString &remarks, bool on) const;
    Settings serverDnsFor(const QString &sub, const QString &remarks, const QString &dns) const;
    bool autoWlMarked() const;
    bool isAutoWl(const QString &sub, const QString &remarks) const;

    QJsonObject toJson() const;
    static Settings from(const QJsonObject &o);
};

struct Sub {
    QString id;
    QString url;
    std::optional<QString> title;
    std::optional<QString> customName;
    qint64 upload = 0, download = 0, total = 0, expire = 0, updated = 0;
    std::optional<int> intervalHours;
    std::optional<QString> announce;
    std::optional<QString> routing;
    std::optional<QString> supportUrl;
    std::optional<QString> webPage;

    QString name() const;
    QJsonObject toJson() const;
    static Sub from(const QJsonObject &o);
    QVariantMap toVariant() const;
};

struct Ping {
    std::optional<int> ms;
    QString error;
};

struct Server {
    QString remarks, name, flag;
    int group = 1;
    QString kind;
    bool supported = false;
    QString description;
    bool placeholder = false;
    QString reason, server, sni, fingerprint, link;
    QList<Server> members;

    static QPair<QString, QString> splitFlag(const QString &s);
    static Server from(const QJsonObject &o);
    QVariantMap toVariant() const;
};

struct Section {
    QString title;
    QList<Server> items;
    QString note;
    bool hasNote = false;
};

QString groupTitle(int group, bool russian);
QString groupTitle(int group);
QList<Server> split(const QList<Server> &all);
QList<Section> sections(const QList<Server> &all);
QList<Section> sorted(QList<Section> list, const QString &sort, const QHash<QString, Ping> &pings);
const Server *byRemarks(const QList<Server> &list, const QString &remarks);
QString shortKind(const QString &kind);

namespace AutoSelect {
enum class Net { Free, Whitelist, Offline };
QString netKey(Net n);

struct Probe {
    QString host;
    int ms = -1;
    bool home = false;
    bool ok() const { return ms >= 0; }
};

QStringList hosts(const QString &text);
QString directUrl(const QString &host, bool home);
Net classify(const QList<Probe> &probes);
bool namedForWhitelists(const QString &remarks, const QString &subTitle = {});
bool isUdp(const Server &s);
inline const QStringList TRANSPORTS = {"TCP", "XHTTP", "gRPC", "WebSocket", "HTTPUpgrade", "Hysteria"};
QString transport(const Server &s);
QString sig(const Server &s);

struct Learned {
    QStringList yes, no;
};
Learned learn(const Learned &old, const QStringList &passed, const QStringList &failed, const QStringList &squeezed);

struct Entry {
    QString sub, subTitle;
    Server server;
    bool yes = false, no = false, pinned = false;
    std::optional<int> prior;
    std::optional<bool> mark;
    bool named = false;
    QList<int> samples;
    bool carried = false, squeezed = false;

    Target target() const { return QPair(sub, server.remarks); }
    bool tried() const { return !samples.isEmpty(); }
    bool answered() const { return tried() && samples.first() >= 0; }
    bool whitelist() const { return mark.value_or(yes || named); }
    bool bypass() const { return whitelist() || yes; }
    bool reserve() const { return mark.value_or(false); }
    int ms() const;
};
using Prefer = std::function<bool(const Entry &)>;
QList<int> order(const QList<Entry> &all, const QList<int> &which, const Target &current, const Prefer &prefer);
QList<int> whitelistOrder(const QList<Entry> &all, const QList<int> &which, const Target &current, const Prefer &prefer);
bool switches(int best, int current, int margin, int bestSamples, int currentSamples);
int score(const Entry &e, Net mode, bool pins, int speed = 0);

struct Pick {
    std::optional<int> chosen;
    QHash<int, int> scores;
};
Pick choose(const QList<Entry> &all, Net mode, const Settings &s, const Target &current, bool failed, bool quick,
            const QHash<QString, int> &speeds = {});
bool heldBack(const Settings &s, Net mode, bool reserve);
}

namespace ExitIp {
inline const QString DEFAULT_URL = Endpoints::IpCheck;
struct Result {
    QString ip, country;
};
bool isIp(const QString &s);
std::optional<Result> parse(const QByteArray &body);
}

namespace Local {
inline const QString ID = QStringLiteral("local");
inline const QString URL = QStringLiteral("rynet:local");
QString title();
bool isLocal(const Sub &sub);
QString serverInput(const QString &text);
bool isServers(const QString &text);
QJsonArray entries(const QString &body);
QString merge(const QString &body, const QString &text);
QStringList preview(const QString &text);
std::optional<QString> name(const QJsonValue &entry);
std::optional<QString> without(const QString &body, const QString &remarks);
QString renamed(const QString &body, const QString &remarks, const QString &name);
Sub sub();
}

namespace DeepLink {
QVariantMap parse(const QString &link);
}

namespace Backup {
struct Part {
    QString key, ru, en;
};
const QList<Part> &parts();
QByteArray encode(const Settings &settings, const QList<Sub> &subs, const std::function<std::optional<QString>(const QString &)> &body,
                  const QStringList &parts);
struct Snapshot {
    Settings settings;
    QList<QPair<Sub, QString>> subs;
};
QStringList partsIn(const QByteArray &bytes, int *subs = nullptr);
Snapshot decode(const QByteArray &bytes, const Settings &current, const QStringList &parts);
}

namespace Zlib {
QByteArray gzip(const QByteArray &data);
QByteArray gunzip(const QByteArray &data, qsizetype limit);
QByteArray deflateRaw(const QByteArray &data);
std::optional<QByteArray> inflateRaw(const QByteArray &data, qsizetype limit);
quint32 crc32(const QByteArray &data);
}

qint64 nowSeconds();
qint64 nowMillis();
