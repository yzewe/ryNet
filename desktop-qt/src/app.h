#pragma once

#include "model.h"
#include "update.h"

#include <QHash>
#include <QJSValue>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QQmlEngine>
#include <QTimer>
#include <QVariant>
#include <memory>

class Core;
class QThread;
class Tray;
class QNetworkAccessManager;

class App : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

    Q_PROPERTY(bool ru READ ru CONSTANT)
    Q_PROPERTY(bool windows READ windows CONSTANT)
    Q_PROPERTY(QString version READ version CONSTANT)
    Q_PROPERTY(QString coreVersion READ coreVersion NOTIFY coreVersionChanged)
    Q_PROPERTY(bool sysProxyAvailable READ sysProxyAvailable CONSTANT)
    Q_PROPERTY(QVariantMap update READ updateVariant NOTIFY updateChanged)

    Q_PROPERTY(QVariantList subs READ subsVariant NOTIFY subsChanged)
    Q_PROPERTY(QString currentSub READ currentSubId NOTIFY subsChanged)
    Q_PROPERTY(QVariantMap current READ currentVariant NOTIFY subsChanged)
    Q_PROPERTY(QString expiryAlert READ expiryAlert NOTIFY subsChanged)
    Q_PROPERTY(QVariantMap settings READ settingsVariant NOTIFY settingsChanged)
    Q_PROPERTY(QVariantList servers READ serversVariant NOTIFY serversChanged)
    Q_PROPERTY(int supportedCount READ supportedCount NOTIFY serversChanged)
    Q_PROPERTY(QVariantMap pings READ pingsVariant NOTIFY pingsChanged)
    Q_PROPERTY(QStringList pingWaiting READ pingWaiting NOTIFY pingsChanged)
    Q_PROPERTY(bool pinging READ pinging NOTIFY pingsChanged)
    Q_PROPERTY(int pingDone READ pingDone NOTIFY pingsChanged)

    Q_PROPERTY(int phase READ phase NOTIFY connectionChanged)
    Q_PROPERTY(int status READ status NOTIFY connectionChanged)
    Q_PROPERTY(bool blocked READ blocked NOTIFY connectionChanged)
    Q_PROPERTY(QString failure READ failure NOTIFY connectionChanged)
    Q_PROPERTY(QString statusTitle READ statusTitle NOTIFY connectionChanged)
    Q_PROPERTY(double since READ since NOTIFY connectionChanged)
    Q_PROPERTY(QString connectedName READ connectedName NOTIFY connectionChanged)
    Q_PROPERTY(QVariantMap connectedServer READ connectedServer NOTIFY connectionChanged)
    Q_PROPERTY(QVariantMap connectingServer READ connectingServer NOTIFY connectionChanged)
    Q_PROPERTY(QString exitIp READ exitIp NOTIFY exitChanged)
    Q_PROPERTY(QString exitFlag READ exitFlag NOTIFY exitChanged)
    Q_PROPERTY(double upload READ upload NOTIFY statsChanged)
    Q_PROPERTY(double download READ download NOTIFY statsChanged)
    Q_PROPERTY(bool tunCapable READ tunCapable NOTIFY tunCapableChanged)
    Q_PROPERTY(bool sysProxyOn READ sysProxyOn NOTIFY sysProxyOnChanged)

    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString notice READ notice WRITE setNotice NOTIFY noticeChanged)
    Q_PROPERTY(bool adding READ adding WRITE setAdding NOTIFY addingChanged)
    Q_PROPERTY(QString nav READ nav WRITE setNav NOTIFY navChanged)
    Q_PROPERTY(QString settingsPage READ settingsPage WRITE setSettingsPage NOTIFY settingsPageChanged)
    Q_PROPERTY(QString hwidDefault READ hwidDefault NOTIFY hwidDefaultChanged)
    Q_PROPERTY(bool entryPick READ entryPick WRITE setEntryPick NOTIFY entryChanged)
    Q_PROPERTY(QString entrySub READ entrySub NOTIFY entryChanged)
    Q_PROPERTY(QVariantList entryServers READ entryServersVariant NOTIFY entryChanged)
    Q_PROPERTY(QVariantMap speedRun READ speedRun NOTIFY speedChanged)
    Q_PROPERTY(QVariantList speedHistory READ speedHistory NOTIFY speedChanged)
    Q_PROPERTY(bool trayAvailable READ trayAvailable NOTIFY trayChanged)
    Q_PROPERTY(QVariantMap autoState READ autoState NOTIFY autoChanged)
    Q_PROPERTY(int rev READ rev NOTIFY revChanged)
    Q_PROPERTY(QString hwidRetry READ hwidRetry NOTIFY hwidRetryChanged)
    Q_PROPERTY(QVariantMap incoming READ incoming NOTIFY incomingChanged)

public:
    static App *instance();
    static App *create(QQmlEngine *, QJSEngine *);
    ~App() override;

    void start(bool autostart);
    void shutdown();
    void setWindowVisible(bool on);
    void setWindowFocused(bool on) { m_focused = on; }

    bool ru() const { return Lang::ru(); }
    bool windows() const;
    QString version() const;
    QString coreVersion() const { return m_coreVersion; }
    bool sysProxyAvailable() const;

    QVariantList subsVariant() const;
    QString currentSubId() const;
    QVariantMap currentVariant() const;
    QString expiryAlert() const;
    QVariantMap settingsVariant() const;
    QVariantList serversVariant() const;
    int supportedCount() const;
    QVariantMap pingsVariant() const;
    QStringList pingWaiting() const { return m_waiting; }
    Q_INVOKABLE bool pingWaitingFor(const QString &remarks, const QString &sub = QString()) const { return m_pingSub == (sub.isEmpty() ? m_settings.sub.value_or(QString()) : sub) && m_waiting.contains(remarks); }
    bool pinging() const { return m_pinging; }
    int pingDone() const { return int(m_pings.size()); }

    int phase() const { return m_phase; }
    int status() const;
    bool blocked() const { return m_blocked; }
    QString failure() const { return m_failure; }
    QString statusTitle() const;
    double since() const { return double(m_since); }
    QString connectedName() const { return m_connectedName; }
    QVariantMap connectedServer() const { return m_connectedServer; }
    QVariantMap connectingServer() const { return m_phase == 1 ? m_pendingServer : QVariantMap(); }
    QString exitIp() const { return m_exitIp; }
    QString exitFlag() const { return m_exitFlag; }
    double upload() const { return double(m_upload); }
    double download() const { return double(m_download); }
    bool tunCapable() const { return m_tunCapable; }
    bool sysProxyOn() const { return m_sysProxyOn; }

    bool busy() const { return m_fetching > 0; }
    QString notice() const { return m_notice; }
    void setNotice(const QString &n);
    bool adding() const { return m_adding; }
    void setAdding(bool on);
    QString nav() const { return m_nav; }
    void setNav(const QString &n);
    QString settingsPage() const { return m_settingsPage; }
    void setSettingsPage(const QString &p);
    QString hwidDefault() const { return m_hwidDefault; }
    bool entryPick() const { return m_entryPick; }
    void setEntryPick(bool on);
    QString entrySub() const { return m_entrySub; }
    QVariantList entryServersVariant() const;
    QVariantMap speedRun() const { return m_speed; }
    QVariantList speedHistory() const;
    bool trayAvailable() const;
    QVariantMap autoState() const { return m_autoInfo; }
    int rev() const { return m_rev; }
    QString hwidRetry() const { return m_hwidRetry; }
    QVariantMap incoming() const { return m_incoming; }
    void openLink(const QString &link);
    void openPluginFile(const QString &path);
    Q_INVOKABLE void retryWithHwid();

    const Settings &settingsRef() const { return m_settings; }
    const QList<Server> &serversRef() const { return m_servers; }
    const QList<Server> &entryServersRef() const { return m_entryServers; }
    const QHash<QString, Ping> &pingsRef() const { return m_pings; }

    Q_INVOKABLE void connectVpn();
    Q_INVOKABLE void disconnectVpn();
    Q_INVOKABLE void toggle();
    Q_INVOKABLE void setStats(bool on);
    Q_INVOKABLE void grantTun();

    Q_INVOKABLE void add(const QString &text);
    Q_INVOKABLE bool addServer(const QString &text);
    Q_INVOKABLE QVariantMap takeIncoming();
    Q_INVOKABLE void addLink(const QVariantMap &link);
    Q_INVOKABLE void refresh(const QString &id);
    Q_INVOKABLE void refreshAll();
    Q_INVOKABLE void remove(const QString &id);
    Q_INVOKABLE void editSub(const QString &id, const QString &name, const QString &url, bool split = false);
    Q_INVOKABLE void checkBalancers(const QString &id);
    Q_INVOKABLE bool validSubUrl(const QString &url) const;
    Q_INVOKABLE void selectSub(const QString &id);
    Q_INVOKABLE void selectServer(const QString &remarks);
    Q_INVOKABLE void connectTo(const QString &remarks);
    Q_INVOKABLE void removeLocal(const QString &remarks);
    Q_INVOKABLE void renameLocal(const QString &remarks, const QString &name);
    Q_INVOKABLE QVariantMap server(const QString &remarks) const;
    Q_INVOKABLE QVariantMap entryServer(const QString &remarks) const;
    Q_INVOKABLE QVariantMap ping(const QString &remarks) const;
    Q_INVOKABLE QVariantMap pingIn(const QString &sub, const QString &remarks) const;
    Q_INVOKABLE QString groupOf(const QString &remarks) const;
    Q_INVOKABLE void loadServersOf(const QString &id);
    Q_INVOKABLE void details(const QString &remarks);

    Q_INVOKABLE void ping(const QStringList &names = {});
    Q_INVOKABLE void pingSome(const QStringList &remarks, const QString &sub = QString());
    Q_INVOKABLE void stopPing();
    Q_INVOKABLE void probe();

    Q_INVOKABLE void set(const QString &key, const QVariant &value, bool reconnect = false);
    Q_INVOKABLE void setRouting(const QString &mode);
    Q_INVOKABLE void setSystemProxy(bool on);
    Q_INVOKABLE void setDebugLog(bool on);
    Q_INVOKABLE void setSendLogs(bool on);
    Q_INVOKABLE void checkUpdate();
    Q_INVOKABLE void openUpdate();
    Q_INVOKABLE void openUpdatePost();
    Q_INVOKABLE void setAutostart(bool on);
    Q_INVOKABLE void setSeedColor(const QString &color);
    Q_INVOKABLE void pin(const QString &remarks, bool on);
    Q_INVOKABLE bool pinned(const QString &remarks) const;
    Q_INVOKABLE QString serverDns(const QString &remarks) const;
    Q_INVOKABLE void setServerDns(const QString &remarks, const QString &dns);
    Q_INVOKABLE void setChainEntry(const QString &remarks, const QString &subId = QString());
    Q_INVOKABLE void openEntryPicker();
    Q_INVOKABLE void showEntrySub(const QString &id);
    Q_INVOKABLE void pickEntry(const QString &remarks);
    Q_INVOKABLE QVariantMap chainServer() const;
    Q_INVOKABLE void updateGeo();
    Q_INVOKABLE bool validDns(const QString &text) const;
    Q_INVOKABLE QString fieldText(const QString &field) const;
    Q_INVOKABLE bool fieldValid(const QString &field, const QString &text) const;
    Q_INVOKABLE void saveField(const QString &field, const QString &text);
    Q_INVOKABLE bool validRange(const QString &v, int max) const;

    Q_INVOKABLE QVariantList rules() const;
    Q_INVOKABLE void saveRule(int index, const QVariantMap &rule);
    Q_INVOKABLE void deleteRule(int index);
    Q_INVOKABLE bool ruleValid(const QVariantMap &rule) const;
    Q_INVOKABLE QVariantList sets() const;
    Q_INVOKABLE QVariantMap subscriptionRules() const;
    Q_INVOKABLE QVariantMap modeRules(const QString &which) const;
    Q_INVOKABLE void geoCounts(const QVariantList &items, const QJSValue &done);
    Q_INVOKABLE void importRules(const QString &text, const QJSValue &done);
    Q_INVOKABLE void coverWarnings(const QString &kind, const QString &value, const QString &action, const QJSValue &done);
    Q_INVOKABLE QVariantMap ruleSet(const QString &id) const;
    Q_INVOKABLE QVariantMap newSet() const;
    Q_INVOKABLE void saveSet(const QVariantMap &set);
    Q_INVOKABLE void deleteSet(const QString &id);
    Q_INVOKABLE void toggleSet(const QString &id);
    Q_INVOKABLE void orderSets(const QStringList &order, int enabled);
    Q_INVOKABLE QString setLink(const QVariantMap &set) const;
    Q_INVOKABLE QStringList setParts(const QString &link) const;
    Q_INVOKABLE QVariantMap parseSet(const QString &text) const;
    Q_INVOKABLE QVariantMap normalizeEntries(const QString &kind, const QString &text) const;
    Q_INVOKABLE QStringList processes() const;
    Q_INVOKABLE int extraRuleLines() const;

    Q_INVOKABLE void autoCheck();
    Q_INVOKABLE void autoChoose(const QString &sub, const QString &remarks);
    Q_INVOKABLE void loadAutoServers();
    Q_INVOKABLE void autoSkipSub(const QString &sub, bool on);
    Q_INVOKABLE void autoSkipServer(const QString &sub, const QString &remarks, bool on);
    Q_INVOKABLE void autoMarkServer(const QString &sub, const QString &remarks, bool on);
    Q_INVOKABLE void autoMarkSub(const QString &sub, bool on);
    Q_INVOKABLE QString autoHosts(const QString &text) const;

    Q_INVOKABLE void openSpeed(const QString &remarks);
    Q_INVOKABLE void startSpeed();
    Q_INVOKABLE void stopSpeed();
    Q_INVOKABLE void closeSpeed();
    Q_INVOKABLE void clearSpeedHistory();

    Q_INVOKABLE void pickFile(bool save, const QString &name, const QString &title, const QString &ext, const QJSValue &done);
    Q_INVOKABLE void plugins(const QString &op, const QVariantMap &arg, const QString &file, const QJSValue &done);
    Q_INVOKABLE QVariantList backupParts() const;
    Q_INVOKABLE QVariantList backupFileParts(const QString &file);
    Q_INVOKABLE void exportBackup(const QString &file, const QStringList &parts);
    Q_INVOKABLE void importBackup(const QString &file, const QStringList &parts);
    Q_INVOKABLE bool canImport();
    Q_INVOKABLE void copy(const QString &text);
    Q_INVOKABLE QString clipboard() const;
    Q_INVOKABLE void open(const QString &target);
    Q_INVOKABLE QString dataDir() const;
    Q_INVOKABLE QString logFile() const;
    Q_INVOKABLE QString diagName() const;
    Q_INVOKABLE void exportDiag(const QString &file);
    Q_INVOKABLE void writeText(const QString &file, const QString &text, const QString &doneNotice);
    Q_INVOKABLE QString readText(const QString &file, int maxBytes) const;
    Q_INVOKABLE QString uptime(double sinceMs) const;
    Q_INVOKABLE QString bytes(double b) const;
    Q_INVOKABLE QVariantMap subOr(const QString &id) const;
    Q_INVOKABLE QString subName(const QString &id) const;
    Q_INVOKABLE QString ago(double seconds) const;
    Q_INVOKABLE QString mbps(double bps) const;
    Q_INVOKABLE QString plural(int n, const QString &one, const QString &few, const QString &many, const QString &en, const QString &enMany = {}) const;
    Q_INVOKABLE QString backupName() const;
    Q_INVOKABLE void showWindow() { emit showRequested(); }
    Q_INVOKABLE void copyLinks(const QString &id);
    Q_INVOKABLE void removeDead();
    Q_INVOKABLE bool noticeHidden(const QString &sub, const QString &text) const;
    Q_INVOKABLE void dismissNotice(const QString &sub, const QString &text);
    Q_INVOKABLE bool geoMissing() const;
    Q_INVOKABLE void quit();

signals:
    void coreVersionChanged();
    void updateChanged();
    void subsChanged();
    void settingsChanged();
    void serversChanged();
    void pingsChanged();
    void connectionChanged();
    void exitChanged();
    void statsChanged();
    void tunCapableChanged();
    void sysProxyOnChanged();
    void busyChanged();
    void noticeChanged();
    void addingChanged();
    void navChanged();
    void settingsPageChanged();
    void hwidDefaultChanged();
    void entryChanged();
    void speedChanged();
    void trayChanged();
    void autoChanged();
    void autoServersReady(const QVariantList &groups, const QStringList &transports);
    void revChanged();
    void hwidRetryChanged();
    void incomingChanged();
    void serversOfReady(const QString &id, const QVariantList &servers);
    void balancersOf(const QString &id, bool has);
    void detailsReady(const QString &remarks, const QVariantMap &details);
    void added(const QString &link);
    void subEdited(const QString &id, const QString &error);
    void showRequested();
    void quitRequested();

private slots:
    void prepareForSleep(bool sleeping);

private:
    explicit App(QObject *parent = nullptr);

    std::optional<Sub> current() const;
    std::optional<QString> currentBody() const;
    void save();
    void apply(const Settings &next, bool reconnect = false);
    void changeSettings(const Settings &next);

    Core *newCore(bool elevated);
    bool wantElevated();
    void restartCore();
    void restartCore(bool elevated);
    void onEvent(const QJsonObject &e);
    void onStarted(const QJsonObject &e);
    void onStopped();
    void onError(const QJsonObject &e);
    void loadServers(std::function<void()> then = {});
    void fetch(const QString &url, const std::optional<QString> &id, bool relink = false, const std::optional<QString> &name = {});
    void addLocal(const QString &text);
    QJsonObject options() const;
    void setPhase(int p);
    void checkHealth(bool again = true);
    void connectSelected();
    void autoPick(bool initial, bool failed = false, bool periodic = false, bool look = false);
    void cancelAutoPick();
    void autoTick();
    void autoSchedule();
    int autoDelay() const;
    void publishAuto(const QVariantMap &info);
    struct AutoRun;
    struct AutoBatch;
    void autoStart(const std::shared_ptr<AutoRun> &run);
    void autoDirect(const std::shared_ptr<AutoRun> &run, const QStringList &home, const QStringList &free, bool early,
                    std::function<void(const QList<AutoSelect::Probe> &)> done);
    void autoBatch(const std::shared_ptr<AutoRun> &run, const QList<int> &which, const QString &url, bool race,
                   std::function<void(const QHash<int, QJsonObject> &)> done);
    void autoBatchSend(const std::shared_ptr<AutoRun> &run, const std::shared_ptr<AutoBatch> &b);
    void autoBatchEnd(const std::shared_ptr<AutoRun> &run, const std::shared_ptr<AutoBatch> &b);
    void autoPings(const std::shared_ptr<AutoRun> &run, const QList<int> &which, std::function<void()> then);
    void autoFirst(const std::shared_ptr<AutoRun> &run);
    void autoWiden(const std::shared_ptr<AutoRun> &run);
    void autoKnown(const std::shared_ptr<AutoRun> &run);
    void autoLead(const std::shared_ptr<AutoRun> &run, int round);
    void autoCarry(const std::shared_ptr<AutoRun> &run, int round);
    void autoDone(const std::shared_ptr<AutoRun> &run);
    void autoNotify(const QVariantMap &prev, const QVariantMap &now, const QPair<QString, QString> &from, const AutoSelect::Entry &to, bool failed);
    void onFailing();
    void reconnectAfter(const QString &reason);
    void switchAway(int epoch, const QString &body, const QString &from, const QString &reason);
    void lookupExit(bool proxy, int port);
    void refreshNotice(const Sub &sub);
    void proxyOff();
    void proxyOn(int port);
    void reportTick();
    void clockTick();
    QVariantMap updateVariant() const;
    void updateTick();
    void updateCheck(bool manual);
    void updateOutcome(bool manual, bool failed);
    void getLimited(const QString &url, qsizetype limit, std::function<void(std::optional<QByteArray>)> done);
    void networkChanged();
    void autoUpdateSubs();
    void expiryAlerts();
    void updateTray();
    void notify(const QString &title, const QString &text);
    void clearConnection();
    void setFetching(int n);
    QList<Server> parseServers(const QJsonObject &reply, const QString &sub) const;
    void askCountries(const QString &sub);
    void putCountries(const QString &sub, const QHash<QString, QString> &found, const QHash<QString, QString> &endpoints);
    void addSpeedHistory();
    void serial(std::function<void()> work);
    void setTunCapable(bool on);
    QJsonObject headers() const;

    Core *m_core = nullptr;
    Tray *m_tray = nullptr;
    QNetworkAccessManager *m_net = nullptr;

    QVariantMap subVariant(const Sub &s) const;
    QList<Sub> m_subs;
    Settings m_settings;
    QList<Server> m_servers;
    QHash<QString, Ping> m_pings;
    QHash<QString, QHash<QString, Ping>> m_pingsBySub;
    std::optional<QString> m_pingSub;
    QStringList m_waiting;
    bool m_pinging = false;
    quint64 m_pingGeneration = 0;
    bool m_startupPingPending = false;

    int m_phase = 0;
    bool m_blocked = false;
    QString m_failure;
    bool m_stopping = false;
    bool m_resumeAfterStop = false;
    bool m_restartAfterStop = false;
    bool m_reconnectAfterReady = false;
    QList<qint64> m_respawns;
    int m_epoch = 0;
    bool m_checkingHealth = false;
    qint64 m_lastFailing = 0;
    QJsonObject m_activeOptions;
    QJsonObject m_pendingOptions;
    std::optional<QString> m_connectedSub, m_connectedProfile, m_pendingSub, m_pendingProfile;
    QString m_connectedName;
    QVariantMap m_pendingServer, m_connectedServer;
    qint64 m_since = 0;
    QString m_exitIp, m_exitFlag;
    qint64 m_upload = 0, m_download = 0;
    bool m_skipSpeed = false;
    bool m_statsOn = false;
    bool m_tunCapable = false;
    bool m_sysProxyOn = false;
    QString m_coreVersion;
    QString m_hwidDefault;

    int m_fetching = 0;
    int m_loading = 0;
    QString m_notice;
    bool m_adding = false;
    QString m_nav = QStringLiteral("home");
    QString m_settingsPage;
    bool m_entryPick = false;
    QString m_entrySub;
    QList<Server> m_entryServers;
    QVariantMap m_speed;
    qint64 m_speedShown = 0;

    QTimer m_pingFlush;
    QTimer m_health;
    QTimer m_startGuard;
    QTimer m_auto;
    QTimer m_autoNet;
    bool m_autoPicking = false;
    std::shared_ptr<AutoRun> m_autoState;
    QVariantMap m_autoInfo;
    int m_autoServersGen = 0;
    QHash<QString, QStringList> m_autoServers;
    qint64 m_autoFullAt = 0, m_autoDeepAt = 0;
    QTimer m_report;
    qint64 m_tickAt = 0;
    QTimer m_netChange;
    QHash<QString, qint64> m_subTried;
    QTimer m_expiry;
    QTimer m_noticeTimer;
    QThread *m_worker = nullptr;
    QObject *m_serial = nullptr;
    bool m_winTun = false;
    bool m_granting = false;
    QString m_serversSub;
    qint64 m_reportRetryAt = 0;
    bool m_reporting = false;
    std::optional<Update::Found> m_update;
    bool m_updateChecking = false, m_updateNone = false;
    qint64 m_updateAt = 0, m_updateRetryAt = 0;
    std::optional<Update::Install> m_install;
    bool m_focused = true;
    bool m_visible = true;
    bool m_closed = false;
    int m_rev = 0;
    QJsonObject m_smartRules;
    QString m_hwidRetry;
    QVariantMap m_incoming;
};
