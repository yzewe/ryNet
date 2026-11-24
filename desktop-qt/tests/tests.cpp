#include "reports.h"
#include "model.h"
#include "platform.h"
#include "update.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QSet>
#include <QTemporaryDir>
#include <QTest>

class Tests : public QObject
{
    Q_OBJECT
private slots:
    void reportsUnsentFollowsTrims();
    void reportsLimits();
    void reportsKeepUpWithAFastLog();
    void autoSelectRanksWorkingServers();
    void autoSelectKeepsStableConnection();
    void autoSelectBackup();
    void autoSelectReadsTheNetwork();
    void subscriptionRulesMatchCoreOrder();
    void pingOnStartupDefaultsOff();
    void ipv4OnlySettingsAndNetworkBackup();
    void explicitProxyInputPreservesSubscriptions();
    void localProxyAliasesSurviveMerge();
    void deepLinks();
    void splitBalancers();
    void processRoutingSettings();
    void polkitScriptQuoting();
    void directPortsSettings();
    void updateVersionsCompareByNumber();
    void updateReleasesAnswer();
    void updateReleasesAreChecked();
    void updateChangelogIsCleaned();
    void updateDownloadsAnswer();
    void updateDownloadsAreChecked();
    void updateInstallKinds();
    void updateFoundSurvivesTheFile();
    void updateCheckSettingDefaultsOn();
};

void Tests::subscriptionRulesMatchCoreOrder()
{
    const QVariantMap view = SubscriptionRules::parse(QStringLiteral(R"({"name":"RU","routeorder":"proxy-direct-proxy","globalproxy":false,"proxysites":["full:example.org"],"directip":["geoip:ru"],"blocksites":["geosite:ads"]})"));
    QCOMPARE(view.value("name").toString(), QStringLiteral("RU"));
    QCOMPARE(view.value("rest").toString(), QStringLiteral("direct"));
    QCOMPARE(view.value("size").toInt(), 3);
    const QVariantList groups = view.value("groups").toList();
    QCOMPARE(groups.size(), 3);
    QCOMPARE(groups[0].toMap().value("action").toString(), QStringLiteral("proxy"));
    QCOMPARE(groups[1].toMap().value("action").toString(), QStringLiteral("direct"));
    QCOMPARE(groups[2].toMap().value("action").toString(), QStringLiteral("block"));
    QCOMPARE(groups[0].toMap().value("domains").toStringList(), QStringList{QStringLiteral("full:example.org")});
    QCOMPARE(SubscriptionRules::parse(QStringLiteral("{}")).value("rest").toString(), QStringLiteral("proxy"));
    QVERIFY(SubscriptionRules::parse(QStringLiteral("bad json")).isEmpty());
}

void Tests::pingOnStartupDefaultsOff()
{
    QVERIFY(!Settings{}.pingOnStart);
    QVERIFY(!Settings::from(QJsonObject{}).pingOnStart);
    Settings s;
    s.pingOnStart = true;
    QVERIFY(Settings::from(s.toJson()).pingOnStart);
}

void Tests::ipv4OnlySettingsAndNetworkBackup()
{
    QVERIFY(!Settings{}.ipv4Only);
    QVERIFY(!Settings::from(QJsonObject{}).ipv4Only);
    QVERIFY(!Settings::from(QJsonObject{{"ipv4Only", "not-a-boolean"}}).ipv4Only);
    Settings s;
    s.ipv4Only = true;
    s.mtu = 1400;
    QVERIFY(s.toJson().value("ipv4Only").isBool());
    QVERIFY(Settings::from(s.toJson()).ipv4Only);
    const auto none = [](const QString &) { return std::optional<QString>(); };
    const auto network = Backup::encode(s, {}, none, {QStringLiteral("network")});
    QCOMPARE(Backup::partsIn(network), QStringList{QStringLiteral("network")});
    const auto restored = Backup::decode(network, Settings(), {QStringLiteral("network")}).settings;
    QVERIFY(restored.ipv4Only);
    QCOMPARE(restored.mtu, 1400);
    QVERIFY(!Backup::decode(network, Settings(), {QStringLiteral("app")}).settings.ipv4Only);
    s.ipv4Only = false;
    QVERIFY(!Settings::from(s.toJson()).ipv4Only);
    const auto disabled = Backup::encode(s, {}, none, {QStringLiteral("network")});
    QVERIFY(!Backup::decode(disabled, restored, {QStringLiteral("network")}).settings.ipv4Only);
}

void Tests::explicitProxyInputPreservesSubscriptions()
{
    const QString subscription = QStringLiteral("https://panel.example/sub/token?client=1#Home");
    QVERIFY(!Local::isServers(subscription));
    const auto deep = DeepLink::parse(QStringLiteral("rynet://add?url=https%3A%2F%2Fpanel.example%2Fsub%2Ftoken%3Fclient%3D1%23Home"));
    QCOMPARE(deep.value("kind").toString(), QStringLiteral("sub"));
    QCOMPARE(deep.value("url").toString(), subscription.section(u'#', 0, 0));
    QCOMPARE(deep.value("name").toString(), QStringLiteral("Home"));
    const QString proxy = QStringLiteral("https://user:p%40ss@proxy.example:8443#Work");
    QVERIFY(!Local::isServers(proxy));
    const QString explicitProxy = Local::serverInput(proxy);
    QCOMPARE(explicitProxy, QStringLiteral("https-proxy://user:p%40ss@proxy.example:8443#Work"));
    QVERIFY(Local::isServers(explicitProxy));
    const QString mixed = Local::serverInput(QStringLiteral("# My proxies\nhttp://127.0.0.1:3128#HTTP\nsocks5h://127.0.0.1:1080#SOCKS"));
    QVERIFY(mixed.startsWith(QStringLiteral("# My proxies\nhttp-proxy://")));
    QCOMPARE(Local::entries(Local::merge(QStringLiteral("[]"), mixed)).size(), 2);
    const QString json = QStringLiteral("[{\"remarks\":\"HTTPS in config\",\"server\":\"https://proxy.example\"}]");
    QCOMPARE(Local::serverInput(json), json);
    QVERIFY(!Local::isServers(subscription));
}

void Tests::localProxyAliasesSurviveMerge()
{
    const QStringList schemes = {QStringLiteral("socks"), QStringLiteral("socks4"), QStringLiteral("socks4a"),
        QStringLiteral("socks5"), QStringLiteral("socks5h"), QStringLiteral("http-proxy"), QStringLiteral("https-proxy")};
    QString body = QStringLiteral("[]");
    for (const QString &scheme : schemes) {
        const QString link = scheme + QStringLiteral("://127.0.0.1:1080#") + scheme;
        QVERIFY2(Local::isServers(link), qPrintable(link));
        QCOMPARE(Local::serverInput(link), link);
        body = Local::merge(body, link);
    }
    const QJsonArray entries = Local::entries(body);
    QCOMPARE(entries.size(), schemes.size());
    for (int i = 0; i < schemes.size(); ++i)
        QCOMPARE(entries.at(i).toString(), schemes.at(i) + QStringLiteral("://127.0.0.1:1080#") + schemes.at(i));
    body = Local::merge(body, QStringLiteral("socks5h://127.0.0.1:1080#Renamed"));
    QCOMPARE(Local::entries(body).size(), schemes.size());
    QVERIFY(Local::entries(body).contains(QStringLiteral("socks5h://127.0.0.1:1080#Renamed")));
}

namespace {
QByteArray lines(int from, int count)
{
    QByteArray out;
    for (int i = from; i < from + count; ++i)
        out += QByteArray::number(i).rightJustified(8, '0') + ' ' + QByteArray(90, 'x') + '\n';
    return out;
}

void append(const QString &file, const QByteArray &bytes)
{
    QFile f(file);
    QVERIFY(f.open(QIODevice::Append));
    f.write(bytes);
}

void trim(const QString &file, qint64 keep)
{
    QFile f(file);
    QVERIFY(f.open(QIODevice::ReadWrite));
    f.seek(std::max<qint64>(0, f.size() - keep));
    QByteArray tail = f.read(keep);
    tail.remove(0, tail.indexOf('\n') + 1);
    f.resize(0);
    f.seek(0);
    f.write(tail);
}
}

void Tests::reportsUnsentFollowsTrims()
{
    QTemporaryDir dir;
    const QString log = dir.filePath(QStringLiteral("core.log"));
    append(log, lines(0, 1000));
    Reports::skipToEnd(dir.path());
    QCOMPARE(Reports::unsent(dir.path()), 0);
    const QByteArray more = lines(1000, 500);
    append(log, more);
    QCOMPARE(Reports::unsent(dir.path()), more.size());

    trim(log, more.size() + 30000);
    QCOMPARE(Reports::unsent(dir.path()), more.size());
    const qint64 rebased = Reports::state(dir.path()).offset;
    QVERIFY(rebased < 40000);
    append(log, lines(1500, 10));
    QCOMPARE(Reports::unsent(dir.path()), more.size() + lines(1500, 10).size());
    QCOMPARE(Reports::state(dir.path()).offset, rebased);

    trim(log, more.size() / 2);
    QFile f(log);
    QCOMPARE(Reports::unsent(dir.path()), f.size());
}

void Tests::reportsLimits()
{
    const qint64 now = 1'800'000'000'000;
    Reports::State s;
    QVERIFY(Reports::allowed(s, now));
    s.sentAt = now - Reports::GAP_MS + 1000;
    QVERIFY(!Reports::allowed(s, now));
    s.sentAt = now - Reports::GAP_MS;
    QVERIFY(Reports::allowed(s, now));
    for (int i = 0; i < Reports::PER_DAY; ++i)
        s.sends.append(now - Reports::DAY_MS + 60000 + i * 60000);
    QVERIFY(!Reports::allowed(s, now));
    s.sends.first() = now - Reports::DAY_MS;
    QVERIFY(Reports::allowed(s, now));
    s.sentAt = now + Reports::DAY_MS;
    s.sends = {now + Reports::DAY_MS};
    QVERIFY(Reports::allowed(s, now));
}

void Tests::reportsKeepUpWithAFastLog()
{
    QTemporaryDir dir;
    const QString log = dir.filePath(QStringLiteral("core.log"));
    append(log, lines(0, 1));
    Reports::skipToEnd(dir.path());
    int next = 1, expect = 1, sends = 0;
    for (int minute = 1; minute <= 12 * 60 + 1; ++minute) {
        const bool last = minute > 12 * 60;
        if (!last) {
            append(log, lines(next, 500));
            next += 500;
            if (QFile(log).size() > 8LL << 20)
                trim(log, 5LL << 20);
        }
        const bool due = Reports::worked(60000, dir.path());
        const qint64 unsent = Reports::unsent(dir.path());
        if (!due && !last && unsent < Reports::SOON_BYTES)
            continue;
        QFile f(log);
        QVERIFY(f.open(QIODevice::ReadOnly));
        const QByteArray part = f.readAll().right(unsent);
        QVERIFY(part.startsWith(QByteArray::number(expect).rightJustified(8, '0')));
        expect += int(part.count('\n'));
        Reports::skipToEnd(dir.path());
        sends += !last;
    }
    QCOMPARE(expect, next);
    QVERIFY(sends <= Reports::PER_DAY);
}

namespace {
AutoSelect::Entry entry(const QString &sub, const QString &name, QList<int> samples, bool carried = false)
{
    AutoSelect::Entry e;
    e.sub = sub;
    e.server.remarks = name;
    e.server.name = name;
    e.server.kind = QStringLiteral("VLESS · TCP · REALITY");
    e.samples = samples;
    e.carried = carried;
    e.named = AutoSelect::namedForWhitelists(name);
    return e;
}
}

void Tests::autoSelectRanksWorkingServers()
{
    using AutoSelect::Net;
    Settings s;
    QList<AutoSelect::Entry> list{entry("one", "LTE Москва", {120, 125}, true), entry("two", "Amsterdam", {50, 60}, true),
                                  entry("three", "Frankfurt", {5, 5})};
    list[2].squeezed = true;
    QCOMPARE(AutoSelect::choose(list, Net::Whitelist, s, {}, false, true).chosen, std::optional<int>(1));
    list[1].carried = false;
    QCOMPARE(AutoSelect::choose(list, Net::Whitelist, s, {}, false, true).chosen, std::optional<int>(0));
    s.autoWl["two"] = QStringList{"Amsterdam"};
    list[1].carried = true;
    list[0].mark = false;
    list[1].mark = true;
    list[2].mark = false;
    QCOMPARE(AutoSelect::choose(list, Net::Whitelist, s, {}, false, true).chosen, std::optional<int>(1));
    QList<AutoSelect::Entry> open{entry("one", "LTE Москва", {10, 12}), entry("two", "Amsterdam", {100, 120})};
    QCOMPARE(AutoSelect::choose(open, Net::Free, Settings{}, {}, false, true).chosen, std::optional<int>(1));
    open[1].samples = {-1};
    QCOMPARE(AutoSelect::choose(open, Net::Free, Settings{}, {}, false, true).chosen, std::optional<int>(0));
    open[0].samples = {-1};
    QVERIFY(!AutoSelect::choose(open, Net::Free, Settings{}, {}, false, true).chosen);
    QList<AutoSelect::Entry> kinds{entry("a", "fast", {20, 20}), entry("a", "grpc", {90, 90})};
    kinds[1].server.kind = QStringLiteral("VLESS · gRPC · TLS");
    Settings grpc;
    grpc.autoTransports = QStringList{"gRPC"};
    QCOMPARE(AutoSelect::choose(kinds, Net::Free, grpc, {}, false, true).chosen, std::optional<int>(1));
    QCOMPARE(AutoSelect::transport(kinds[0].server), QStringLiteral("TCP"));
}

void Tests::autoSelectKeepsStableConnection()
{
    using AutoSelect::Net;
    const Settings s;
    QList<AutoSelect::Entry> list{entry("sub", "current", {150, 160}), entry("sub", "best", {100, 110})};
    const Target current = QPair(QStringLiteral("sub"), QStringLiteral("current"));
    QCOMPARE(AutoSelect::choose(list, Net::Free, s, current, false, false).chosen, std::optional<int>(0));
    list[1].samples = {20, 25};
    QCOMPARE(AutoSelect::choose(list, Net::Free, s, current, false, false).chosen, std::optional<int>(1));
    list[1].samples = {20};
    QCOMPARE(AutoSelect::choose(list, Net::Free, s, current, false, false).chosen, std::optional<int>(0));
    QCOMPARE(AutoSelect::choose(list, Net::Free, s, current, false, true).chosen, std::optional<int>(1));
    list[1].samples = {140};
    QCOMPARE(AutoSelect::choose(list, Net::Free, s, current, true, false).chosen, std::optional<int>(1));
}

void Tests::autoSelectReadsTheNetwork()
{
    using AutoSelect::Net;
    using AutoSelect::Probe;
    QCOMPARE(AutoSelect::classify({Probe{"yandex.ru", 40, true}, Probe{"google.com", 60, false}}), Net::Free);
    QCOMPARE(AutoSelect::classify({Probe{"yandex.ru", 40, true}, Probe{"google.com", -1, false}}), Net::Whitelist);
    QCOMPARE(AutoSelect::classify({Probe{"yandex.ru", -1, true}, Probe{"google.com", -1, false}}), Net::Offline);
    QCOMPARE(AutoSelect::classify({}), Net::Free);
    QVERIFY(AutoSelect::namedForWhitelists("🇷🇺 Москва LTE"));
    QVERIFY(AutoSelect::namedForWhitelists("Для белых списков"));
    QVERIFY(AutoSelect::namedForWhitelists("Server", "Мобильный интернет"));
    QVERIFY(AutoSelect::namedForWhitelists("БС-1"));
    QVERIFY(!AutoSelect::namedForWhitelists("Обход блокировок"));
    QVERIFY(!AutoSelect::namedForWhitelists("Netherlands BLTE"));
    QCOMPARE(AutoSelect::hosts("yandex.ru, vk.ru;\nmax.ru  gosuslugi.ru"), (QStringList{"yandex.ru", "vk.ru", "max.ru", "gosuslugi.ru"}));
    QCOMPARE(AutoSelect::directUrl("google.com", false), QStringLiteral("https://www.google.com/#body=32768&within=1500"));
    QCOMPARE(AutoSelect::directUrl("vk.ru", true), QStringLiteral("https://vk.ru/"));
    auto l = AutoSelect::learn({}, {"a|x"}, {}, {});
    QCOMPARE(l.yes, QStringList{"a|x"});
    l = AutoSelect::learn(l, {"b|y"}, {"a|x"}, {});
    QCOMPARE(l.yes, (QStringList{"a|x", "b|y"}));
    QVERIFY(l.no.isEmpty());
    l = AutoSelect::learn(l, {}, {}, {"a|x"});
    QCOMPARE(l.yes, QStringList{"b|y"});
    QCOMPARE(l.no, QStringList{"a|x"});
}

void Tests::autoSelectBackup()
{
    Settings s;
    s.autoSelect = true;
    s.autoInterval = 0;
    s.autoWhitelist = "never";
    s.autoWl["sub"] = QStringList{"LTE"};
    s.autoSkipSubs = QStringList{"other"};
    s.autoTransports = QStringList{"XHTTP"};
    s.autoNotify = false;
    QStringList all;
    for (const Backup::Part &p : Backup::parts())
        all << p.key;
    const auto none = [](const QString &) { return std::optional<QString>(); };
    const auto backup = Backup::encode(s, {}, none, all);
    const auto restored = Backup::decode(backup, Settings(), all).settings;
    const auto appOnly = Backup::encode(s, {}, none, {QStringLiteral("app"), QStringLiteral("ping")});
    QCOMPARE(Backup::partsIn(appOnly), (QStringList{QStringLiteral("ping"), QStringLiteral("app")}));
    QVERIFY(!Backup::decode(appOnly, Settings(), {QStringLiteral("app")}).settings.autoSelect);
    QVERIFY(restored.autoSelect);
    QCOMPARE(restored.autoInterval, 0);
    QCOMPARE(restored.autoWhitelist, QStringLiteral("never"));
    QCOMPARE(restored.autoWl.value("sub"), QStringList{"LTE"});
    QCOMPARE(restored.autoSkipSubs, QStringList{"other"});
    QCOMPARE(restored.autoTransports, QStringList{"XHTTP"});
    QVERIFY(!restored.autoNotify);
    QVERIFY(restored.autoWlMarked());
    QCOMPARE(Settings::from(QJsonObject{{"autoInterval", -1}}).autoInterval, 15);
    QCOMPARE(Settings::from(QJsonObject{{"autoInterval", 100000}}).autoInterval, 60);
    QCOMPARE(Settings::from(QJsonObject{{"autoHome", " "}}).autoHome, Settings{}.autoHome);
}

void Tests::deepLinks()
{
    const auto sub = DeepLink::parse(QStringLiteral("rynet://add?url=https%3A%2F%2Fpanel.example%2Fs%3Ft%3D1&name=%D0%9C%D0%BE%D0%B9"));
    QCOMPARE(sub.value("kind").toString(), QStringLiteral("sub"));
    QCOMPARE(sub.value("url").toString(), QStringLiteral("https://panel.example/s?t=1"));
    QCOMPARE(sub.value("name").toString(), QStringLiteral("Мой"));
    const auto path = DeepLink::parse(QStringLiteral("rynet://install-sub/https://panel.example/s#Home"));
    QCOMPARE(path.value("url").toString(), QStringLiteral("https://panel.example/s"));
    QCOMPARE(path.value("name").toString(), QStringLiteral("Home"));
    const QByteArray link = "vless://9205c503-358f-4d8d-a081-20c98eddf64a@203.0.113.1:443?security=none#A";
    const auto servers = DeepLink::parse(QStringLiteral("rynet://import/") + QString::fromLatin1(link.toBase64(QByteArray::Base64UrlEncoding | QByteArray::OmitTrailingEquals)));
    QCOMPARE(servers.value("kind").toString(), QStringLiteral("servers"));
    QCOMPARE(servers.value("text").toString(), QString::fromLatin1(link));
    QCOMPARE(Local::preview(QString::fromLatin1(link)), QStringList{QStringLiteral("A")});
    QCOMPARE(Local::preview(QStringLiteral("vless://9205c503-358f-4d8d-a081-20c98eddf64a@203.0.113.1:443?security=none\ntrojan://pw@example.org:443#B")),
             (QStringList{QStringLiteral("203.0.113.1"), QStringLiteral("B")}));
    QVERIFY(Local::preview(QStringLiteral("not a link")).isEmpty());
    QVERIFY(DeepLink::parse(QStringLiteral("rynet://elsewhere/https://panel.example")).isEmpty());
    QVERIFY(DeepLink::parse(QStringLiteral("rynet://add/ftp://panel.example")).isEmpty());
    QVERIFY(DeepLink::parse(QStringLiteral("https://panel.example")).isEmpty());
}

void Tests::splitBalancers()
{
    Server lt;
    lt.remarks = QStringLiteral("🇱🇹 Литва");
    lt.name = QStringLiteral("Литва");
    lt.flag = QStringLiteral("🇱🇹");
    Server a, b;
    a.remarks = lt.remarks + QChar(0x1f) + QStringLiteral("🇱🇹 Vilnius");
    a.name = QStringLiteral("Vilnius");
    a.flag = lt.flag;
    b.remarks = lt.remarks + QChar(0x1f) + QStringLiteral("🇩🇪 Frankfurt");
    b.name = QStringLiteral("Frankfurt");
    b.flag = QStringLiteral("🇩🇪");
    lt.members = {a, b};
    Server fi;
    fi.remarks = fi.name = QStringLiteral("Финляндия");
    const QList<Server> flat = split({lt, fi});
    QCOMPARE(flat.size(), 3);
    QCOMPARE(flat[0].name, QStringLiteral("Литва [Vilnius]"));
    QCOMPARE(flat[1].remarks, b.remarks);
    QCOMPARE(flat[1].flag, QStringLiteral("🇩🇪"));
    QCOMPARE(flat[2].name, QStringLiteral("Финляндия"));
    QVERIFY(Settings::from(Settings{}.toJson()).splitSubs.isEmpty());
    Settings s;
    s.splitSubs = {QStringLiteral("t1")};
    s.exitFlags = true;
    const Settings back = Settings::from(s.toJson());
    QCOMPARE(back.splitSubs, QStringList{QStringLiteral("t1")});
    QVERIFY(back.exitFlags);
}

void Tests::directPortsSettings()
{
    QCOMPARE(Settings{}.directPorts, QString());
    Settings s;
    s.directPorts = QStringLiteral("443, 8000-8100");
    QCOMPARE(Settings::from(s.toJson()).directPorts, s.directPorts);
    QCOMPARE(Settings::from(QJsonObject{}).directPorts, QString());
}

void Tests::processRoutingSettings()
{
    QCOMPARE(Settings{}.processes, QString());
    QCOMPARE(Settings{}.processesMode, QStringLiteral("exclude"));
    Settings s;
    s.processes = QStringLiteral("chrome.exe, steam.exe");
    s.processesMode = QStringLiteral("include");
    const Settings back = Settings::from(s.toJson());
    QCOMPARE(back.processes, s.processes);
    QCOMPARE(back.processesMode, QStringLiteral("include"));
    QCOMPARE(Settings::from(QJsonObject{}).processesMode, QStringLiteral("exclude"));
    QCOMPARE(Settings::from(QJsonObject{{"processesMode", "x"}}).processesMode, QStringLiteral("exclude"));
}

void Tests::polkitScriptQuoting()
{
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
    const QString script = Platform::polkitScript();
    QVERIFY(script.contains(QStringLiteral("/etc/polkit-1/rules.d/49-rynet-resolved.rules")));
    QVERIFY(script.contains(QStringLiteral("/etc/polkit-1/localauthority/50-local.d/49-rynet-resolved.pkla")));
    for (const QChar c : {u'\'', u'"', u'\\', u'$', u'`', u'\n'})
        QVERIFY2(!script.contains(c), qPrintable(QString(c)));
    for (const char *name : {"49-rynet-resolved.rules", "49-rynet-resolved.pkla"}) {
        QFile f(QStringLiteral(":/polkit/") + QLatin1String(name));
        QVERIFY(f.open(QIODevice::ReadOnly));
        QVERIFY(script.contains(QString::fromLatin1(f.readAll().toBase64())));
    }
#endif
}

namespace {
const QString kSha = QStringLiteral("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");

const char *kReleases = R"({"android":{"version":"1.17.0","versionCode":177,"url":"https://t.me/rynetforum/4/7803","changelog":"x","downloadUrl":"https://dl.rynet.app/releases/aa/ryNet-1.17.0-177.apk"},"pc":{"version":"0.12.0","url":"https://t.me/rynetforum/700/7809","changelog":"новое:\n• логика интерфейса перенесена с JavaScript на C++\n• балансеры подписки можно разобрать на отдельные серверы\n\nбатарея:\n• меньше расход памяти и пробуждений системы\n\nфиксы:\n• обновление подписок по расписанию\n• переподключение после сна и смены сети"}})";

QJsonObject entry(const QString &alias, const QString &file, const QString &sha = kSha, const QString &path = {}, const QString &url = {})
{
    return {{"file", file},
            {"bytes", 19341312},
            {"sha256", sha},
            {"source", "https://t.me/x"},
            {"alias", alias},
            {"path", path.isNull() ? QStringLiteral("/releases/%1/%2").arg(sha, file) : path},
            {"url", url.isNull() ? QStringLiteral("https://dl.rynet.app") + alias : url}};
}

QByteArray downloads(const QList<QJsonObject> &items)
{
    QJsonArray a;
    for (const auto &o : items)
        a.append(o);
    return QJsonDocument(a).toJson(QJsonDocument::Compact);
}

QList<QJsonObject> desktopFiles(const QString &v)
{
    return {entry("/windows/latest.msi", "ryNet-" + v + ".msi"),
            entry("/windows/latest.exe", "ryNet-" + v + "-setup.exe"),
            entry("/windows/portable/latest.zip", "ryNet-" + v + "-portable.zip"),
            entry("/windows/cli/latest.zip", "ryNet-cli-" + v + "-windows.zip"),
            entry("/linux/latest.deb", "rynet_" + v + "-1_amd64.deb"),
            entry("/linux/latest.rpm", "rynet-" + v + "-1.x86_64.rpm"),
            entry("/linux/latest.pkg.tar.zst", "rynet-" + v + "-1-x86_64.pkg.tar.zst"),
            entry("/linux/latest.AppImage", "ryNet-" + v + "-x86_64.AppImage"),
            entry("/linux/latest.tar.gz", "ryNet-linux-" + v + ".tar.gz"),
            entry("/linux/cli/latest.tar.gz", "ryNet-cli-linux-" + v + ".tar.gz")};
}
}

void Tests::updateVersionsCompareByNumber()
{
    using Update::newer;
    QVERIFY(newer("0.12.0", "0.12.1"));
    QVERIFY(newer("0.12.9", "0.13"));
    QVERIFY(newer("0.99.9", "1.0"));
    QVERIFY(newer("0.9.0", "0.10.0"));
    QVERIFY(newer("0.12", "0.12.0.1"));
    QVERIFY(!newer("0.12.0", "0.12.0"));
    QVERIFY(!newer("0.12.0", "0.12"));
    QVERIFY(!newer("0.12", "0.12.0"));
    QVERIFY(!newer("0.13.0", "0.12.9"));
    QVERIFY(!newer("1.0", "0.99.9"));
    QVERIFY(!newer("0.10.0", "0.9.0"));
    for (const QString &junk : {QString(), QStringLiteral(" "), QStringLiteral("abc"), QStringLiteral("1.x"), QStringLiteral("1..2"), QStringLiteral(".1"), QStringLiteral("1."),
                                QStringLiteral("-1.0"), QStringLiteral("1.0-beta"), QStringLiteral("v1.0"), QStringLiteral("1.2.3.4.5"), QStringLiteral("99999999999.0"),
                                QStringLiteral("1.0\n2")}) {
        QVERIFY2(!newer("0.12.0", junk), qPrintable(junk));
        QVERIFY2(!newer(junk, "9.9.9"), qPrintable(junk));
    }
    QVERIFY(Update::parseVersion("0.12.0").has_value());
    QCOMPARE(*Update::parseVersion(" 1.2.3 "), (QList<int>{1, 2, 3}));
}

void Tests::updateReleasesAnswer()
{
    const auto r = Update::parseRelease(kReleases);
    QVERIFY(r);
    QCOMPARE(r->version, QStringLiteral("0.12.0"));
    QCOMPARE(r->url, QStringLiteral("https://t.me/rynetforum/700/7809"));
    QVERIFY(r->changelog.startsWith(QStringLiteral("новое:\n• логика интерфейса")));
    QVERIFY(r->changelog.contains(QStringLiteral("\n\nфиксы:\n")));
    QVERIFY(r->changelog.endsWith(QStringLiteral("смены сети")));
    QVERIFY(!Update::parseRelease(R"({"android":null,"pc":null})"));
    QVERIFY(!Update::parseRelease("{}"));
}

void Tests::updateReleasesAreChecked()
{
    QVERIFY(!Update::parseRelease(""));
    QVERIFY(!Update::parseRelease("not json"));
    QVERIFY(!Update::parseRelease("[]"));
    QVERIFY(!Update::parseRelease("null"));
    QVERIFY(!Update::parseRelease(R"({"pc":"0.12.0"})"));
    QVERIFY(!Update::parseRelease(R"({"pc":[]})"));
    QVERIFY(!Update::parseRelease(R"({"pc":{}})"));
    QVERIFY(!Update::parseRelease(R"({"pc":{"version":12}})"));
    QVERIFY(!Update::parseRelease(R"({"pc":{"version":null}})"));
    QVERIFY(!Update::parseRelease(R"({"pc":{"version":"latest"}})"));
    QVERIFY(!Update::parseRelease(R"({"pc":{"version":"0.12.0-rc1"}})"));
    const QByteArray big = R"({"pc":{"version":"0.12.0","changelog":")" + QByteArray(Update::MAX_RELEASES, 'x') + R"("}})";
    QVERIFY(!Update::parseRelease(big));
    for (const char *url : {"http://t.me/a/1", "https://evil.example/a/1", "https://t.me.evil.example/a/1", "https://user@t.me/a/1", "https://t.me:444/a/1", "javascript:alert(1)",
                            "https://t.me/a b", "https://t.me/", "ftp://t.me/a", ""}) {
        const auto r = Update::parseRelease(QByteArray(R"({"pc":{"version":"0.12.0","url":")") + url + R"("}})");
        QVERIFY2(r, url);
        QVERIFY2(r->url.isEmpty(), url);
    }
    const auto r = Update::parseRelease(R"({"pc":{"version":"0.12.0","url":5,"changelog":{"a":1}}})");
    QVERIFY(r);
    QVERIFY(r->url.isEmpty() && r->changelog.isEmpty());
}

void Tests::updateChangelogIsCleaned()
{
    QCOMPARE(Update::cleanText(QStringLiteral("a\r\nb\rc\td"), 100), QStringLiteral("a\nb\nc d"));
    QCOMPARE(Update::cleanText(QStringLiteral("a\u0000b\u0007c\u001b[31md\u007f"), 100), QStringLiteral("abc[31md"));
    QCOMPARE(Update::cleanText(QStringLiteral("a\u202eb\u200bc\ufeffd"), 100), QStringLiteral("abcd"));
    QCOMPARE(Update::cleanText(QStringLiteral("  a\n\n\n\n\nb  "), 100), QStringLiteral("a\n\nb"));
    QCOMPARE(Update::cleanText(QStringLiteral("a\u2028b"), 100), QStringLiteral("a\nb"));
    const QString cut = Update::cleanText(QString(10000, u'x'), Update::MAX_CHANGELOG);
    QCOMPARE(cut.size(), qsizetype(Update::MAX_CHANGELOG + 1));
    QVERIFY(cut.endsWith(QStringLiteral("…")));
    const QString emoji = QString(3999, u'x') + QStringLiteral("😀") + QStringLiteral("tail");
    const QString out = Update::cleanText(emoji, 4000);
    QVERIFY(!out.chopped(1).back().isHighSurrogate());
    QCOMPARE(out.chopped(1).size(), qsizetype(3999));
    const auto r = Update::parseRelease(QJsonDocument(QJsonObject{{"pc", QJsonObject{{"version", "1.0"}, {"changelog", QString(9000, u'y') + u'\u0001'}}}}).toJson());
    QVERIFY(r);
    QCOMPARE(r->changelog.size(), qsizetype(Update::MAX_CHANGELOG + 1));
}

void Tests::updateDownloadsAnswer()
{
    const QByteArray list = downloads(desktopFiles("0.12.0"));
    struct Row {
        Update::Install kind;
        const char *alias;
        const char *file;
    };
    const Row rows[] = {
        {Update::Install::Msi, "/windows/latest.msi", "ryNet-0.12.0.msi"},
        {Update::Install::Setup, "/windows/latest.exe", "ryNet-0.12.0-setup.exe"},
        {Update::Install::Portable, "/windows/portable/latest.zip", "ryNet-0.12.0-portable.zip"},
        {Update::Install::Deb, "/linux/latest.deb", "rynet_0.12.0-1_amd64.deb"},
        {Update::Install::Rpm, "/linux/latest.rpm", "rynet-0.12.0-1.x86_64.rpm"},
        {Update::Install::Pacman, "/linux/latest.pkg.tar.zst", "rynet-0.12.0-1-x86_64.pkg.tar.zst"},
        {Update::Install::AppImage, "/linux/latest.AppImage", "ryNet-0.12.0-x86_64.AppImage"},
        {Update::Install::Tar, "/linux/latest.tar.gz", "ryNet-linux-0.12.0.tar.gz"},
    };
    for (const Row &row : rows) {
        QCOMPARE(Update::alias(row.kind), QString::fromLatin1(row.alias));
        const auto d = Update::parseDownload(list, row.kind, "0.12.0");
        QVERIFY2(d, row.alias);
        QCOMPARE(d->file, QString::fromLatin1(row.file));
        QCOMPARE(d->sha256, kSha);
        QCOMPARE(d->link, QStringLiteral("https://dl.rynet.app/releases/%1/%2").arg(kSha, QString::fromLatin1(row.file)));
    }
    QVERIFY(Update::alias(Update::Install::Unknown).isEmpty());
    QVERIFY(!Update::parseDownload(list, Update::Install::Unknown, "0.12.0"));
    QList<QJsonObject> mixed = desktopFiles("0.12.0");
    mixed.prepend(entry("/android/latest.apk", "ryNet-1.17.0-177.apk"));
    QVERIFY(Update::parseDownload(downloads(mixed), Update::Install::Msi, "0.12.0"));
    const auto viaAlias = Update::parseDownload(downloads({entry("/windows/latest.msi", "ryNet-0.12.0.msi", kSha, "/elsewhere")}), Update::Install::Msi, "0.12.0");
    QVERIFY(viaAlias);
    QCOMPARE(viaAlias->link, QStringLiteral("https://dl.rynet.app/windows/latest.msi"));
}

void Tests::updateDownloadsAreChecked()
{
    const auto one = [](const QJsonObject &o, Update::Install k = Update::Install::Msi, const QString &v = QStringLiteral("0.12.0")) { return Update::parseDownload(downloads({o}), k, v); };
    const QString msi = QStringLiteral("ryNet-0.12.0.msi"), al = QStringLiteral("/windows/latest.msi");
    QVERIFY(one(entry(al, msi)));
    QVERIFY(!one(entry(al, msi, QStringLiteral("abc"))));
    QVERIFY(!one(entry(al, msi, kSha.left(63))));
    QVERIFY(!one(entry(al, msi, kSha + "0")));
    QVERIFY(!one(entry(al, msi, QString(64, u'g'))));
    QVERIFY(!one(entry(al, msi, QStringLiteral(""))));
    QJsonObject nonString = entry(al, msi);
    nonString["sha256"] = 5;
    QVERIFY(!one(nonString));
    const auto upper = one(entry(al, msi, kSha.toUpper(), QStringLiteral("/releases/%1/%2").arg(kSha, msi)));
    QVERIFY(upper);
    QCOMPARE(upper->sha256, kSha);
    for (const char *bad : {"http://dl.rynet.app/windows/latest.msi", "https://evil.example/windows/latest.msi", "https://dl.rynet.app.evil.example/windows/latest.msi",
                            "https://user@dl.rynet.app/windows/latest.msi", "https://dl.rynet.app:8443/windows/latest.msi", "https://dl.rynet.app/windows/other.msi", ""}) {
        QVERIFY2(!one(entry(al, msi, kSha, QStringLiteral("/nope"), QString::fromLatin1(bad))), bad);
    }
    QVERIFY(!one(entry(al, msi, kSha, QStringLiteral("/releases/%1/%2").arg(QString(64, u'a'), msi), QStringLiteral("https://evil.example/x"))));
    QVERIFY(!one(entry(al, msi, kSha, QStringLiteral("//evil.example/releases/%1/%2").arg(kSha, msi), QStringLiteral("https://evil.example/x"))));
    QVERIFY(!one(entry(al, msi, kSha, QStringLiteral("/releases/%1/../%2").arg(kSha, msi), QStringLiteral("https://evil.example/x"))));
    QVERIFY(!one(entry(al, QStringLiteral("ryNet-0.11.0.msi"))));
    QVERIFY(!one(entry(al, QStringLiteral("ryNet-0.12.0.msi.exe"))));
    QVERIFY(!one(entry(al, QStringLiteral("x/ryNet-0.12.0.msi"))));
    QVERIFY(!one(entry(al, QStringLiteral("ryNet-0.12.0-setup.exe"))));
    QVERIFY(!one(entry(al, msi), Update::Install::Msi, QStringLiteral("0.12.1")));
    QVERIFY(!one(entry(al, msi), Update::Install::Msi, QStringLiteral("0x12")));
    QVERIFY(!one(entry(al, msi), Update::Install::Msi, QStringLiteral("0.12.0.*")));
    QVERIFY(!one(entry(QStringLiteral("/linux/latest.deb"), QStringLiteral("rynet_0.12.0-1_arm64.deb")), Update::Install::Deb));
    QVERIFY(!one(entry(QStringLiteral("/linux/latest.deb"), QStringLiteral("rynet_0.12.0-_amd64.deb")), Update::Install::Deb));
    QVERIFY(!Update::parseDownload("{}", Update::Install::Msi, "0.12.0"));
    QVERIFY(!Update::parseDownload("null", Update::Install::Msi, "0.12.0"));
    QVERIFY(!Update::parseDownload("[1,null,\"x\"]", Update::Install::Msi, "0.12.0"));
    QVERIFY(!Update::parseDownload("", Update::Install::Msi, "0.12.0"));
    QVERIFY(!Update::parseDownload(downloads({entry("/linux/latest.deb", "rynet_0.12.0-1_amd64.deb")}), Update::Install::Msi, "0.12.0"));
    QVERIFY(!Update::parseDownload(downloads(desktopFiles("0.12.0")) + QByteArray(Update::MAX_DOWNLOADS, ' '), Update::Install::Msi, "0.12.0"));
}

void Tests::updateInstallKinds()
{
    using Update::Install;
    QCOMPARE(Update::windowsInstall(true, "C:/x", "C:/x", "C:/x"), Install::Portable);
    QCOMPARE(Update::windowsInstall(false, "C:/Users/a/AppData/Local/rynet", "C:\\Users\\a\\AppData\\Local\\rynet\\", ""), Install::Msi);
    QCOMPARE(Update::windowsInstall(false, "C:/Program Files/ryNet", "", "c:\\program files\\rynet\\"), Install::Setup);
    QCOMPARE(Update::windowsInstall(false, "C:/Program Files/ryNet", "C:/Elsewhere", "D:/Other"), Install::Unknown);
    QCOMPARE(Update::windowsInstall(false, "D:/tools/ryNet", "", ""), Install::Unknown);
    const QString home = QStringLiteral("/home/u");
    QCOMPARE(Update::linuxInstall("/home/u/Apps/ryNet.AppImage", "/tmp/.mount_x/usr/bin/rynet", home, ""), Install::AppImage);
    QCOMPARE(Update::linuxInstall("", "/home/u/.local/opt/rynet/bin/rynet", home, ""), Install::Tar);
    QCOMPARE(Update::linuxInstall("", "/home/u/.local/opt/rynet/bin/rynet", "/home/u/", ""), Install::Tar);
    QCOMPARE(Update::linuxInstall("", "/home/other/.local/opt/rynet/bin/rynet", home, ""), Install::Unknown);
    QCOMPARE(Update::linuxInstall("", "/opt/rynet/bin/rynet", home, "deb"), Install::Deb);
    QCOMPARE(Update::linuxInstall("", "/opt/rynet/bin/rynet", home, "rpm"), Install::Rpm);
    QCOMPARE(Update::linuxInstall("", "/opt/rynet/bin/rynet", home, "pacman"), Install::Pacman);
    QCOMPARE(Update::linuxInstall("", "/opt/rynet/bin/rynet", home, ""), Install::Unknown);
    QCOMPARE(Update::linuxInstall("", "/home/u/src/ryNet/build/rynet", home, "deb"), Install::Unknown);
    QCOMPARE(Update::linuxInstall("", "/opt/rynet-old/bin/rynet", home, "deb"), Install::Unknown);
    QCOMPARE(Update::linuxInstall("", "/home/u/.local/opt/rynet/bin/rynet", "", ""), Install::Unknown);
    QSet<QString> seen;
    for (Install k : {Install::Msi, Install::Setup, Install::Portable, Install::Deb, Install::Rpm, Install::Pacman, Install::AppImage, Install::Tar}) {
        QVERIFY(!Update::alias(k).isEmpty());
        seen.insert(Update::alias(k));
    }
    QCOMPARE(seen.size(), 8);
}

void Tests::updateFoundSurvivesTheFile()
{
    const Update::Found f{"0.12.0", "notes", "https://t.me/a/1", "ryNet-0.12.0.msi", QStringLiteral("https://dl.rynet.app/releases/%1/ryNet-0.12.0.msi").arg(kSha), kSha};
    const auto back = Update::Found::fromJson(f.toJson());
    QVERIFY(back);
    QCOMPARE(back->link, f.link);
    QCOMPARE(back->sha256, kSha);
    QCOMPARE(back->post, f.post);
    const Update::Found page{"0.12.0", "", "", "", Update::pageUrl(), ""};
    const auto p = Update::Found::fromJson(page.toJson());
    QVERIFY(p);
    QVERIFY(p->file.isEmpty() && p->sha256.isEmpty());
    for (const char *link : {"http://dl.rynet.app/x", "https://evil.example/x", "https://dl.rynet.app@evil.example/x", "javascript:1", ""}) {
        QJsonObject o = f.toJson();
        o["link"] = QString::fromLatin1(link);
        QVERIFY2(!Update::Found::fromJson(o), link);
    }
    QJsonObject badSha = f.toJson();
    badSha["sha256"] = "zz";
    QVERIFY(!Update::Found::fromJson(badSha));
    QJsonObject badVersion = f.toJson();
    badVersion["version"] = "soon";
    QVERIFY(!Update::Found::fromJson(badVersion));
    QVERIFY(!Update::Found::fromJson(QJsonObject{}));
    QJsonObject badPost = f.toJson();
    badPost["post"] = "https://evil.example/";
    QVERIFY(Update::Found::fromJson(badPost)->post.isEmpty());
}

void Tests::updateCheckSettingDefaultsOn()
{
    QVERIFY(Settings{}.updateCheck);
    QVERIFY(Settings::from(QJsonObject{}).updateCheck);
    Settings s;
    s.updateCheck = false;
    QVERIFY(!Settings::from(s.toJson()).updateCheck);
}

QTEST_GUILESS_MAIN(Tests)
#include "tests.moc"
