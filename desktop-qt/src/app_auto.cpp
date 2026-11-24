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

int App::autoDelay() const
{
    const int full = m_settings.autoInterval > 0 ? m_settings.autoInterval * 60000 : 0;
    if (m_autoInfo.value("mode").toString() == u"whitelist" && m_settings.autoWhitelist == u"auto")
        return full > 0 ? std::min(full, kWhitelistMs) : kWhitelistMs;
    return full;
}

void App::autoSchedule()
{
    m_auto.stop();
    const int delay = m_settings.autoSelect && m_phase == 2 ? autoDelay() : 0;
    if (delay > 0)
        m_auto.start(delay);
    const qint64 next = delay > 0 ? nowMillis() + delay : 0;
    if (m_autoInfo.value("next").toLongLong() != next) {
        m_autoInfo["next"] = next;
        emit autoChanged();
    }
}

void App::autoTick()
{
    if (!m_settings.autoSelect || m_autoPicking || m_stopping || m_phase != 2)
        return;
    const bool dueFull = m_settings.autoInterval > 0 && nowMillis() - m_autoFullAt >= qint64(m_settings.autoInterval) * 60000;
    if (m_autoInfo.value("mode").toString() == u"whitelist" && m_settings.autoWhitelist == u"auto" && !dueFull) {
        auto run = std::make_shared<AutoRun>();
        m_autoState = run;
        m_autoPicking = true;
        autoDirect(run, {}, AutoSelect::hosts(m_settings.autoFree), false, [this, run](const QList<AutoSelect::Probe> &probes) {
            m_autoState.reset();
            m_autoPicking = false;
            if (std::any_of(probes.begin(), probes.end(), [](const auto &p) { return p.ok(); }))
                autoPick(false, false, true);
            else
                autoSchedule();
        });
        return;
    }
    autoPick(false, false, true);
}

void App::autoCheck()
{
    if (m_autoPicking || m_stopping || m_phase == 1)
        return;
    if (m_phase == 2 && m_settings.autoSelect)
        autoPick(false);
    else
        autoPick(false, false, false, true);
}

void App::autoPick(bool initial, bool failed, bool periodic, bool look)
{
    if (initial && m_autoPicking && m_autoState && m_autoState->look) {
        m_autoState.reset();
        m_autoPicking = false;
    }
    if ((!m_settings.autoSelect && !look) || m_autoPicking || m_stopping || m_closed || (!initial && !look && m_phase != 2))
        return;
    if (initial && m_settings.connMode == u"tun" && !m_tunCapable) {
        setNotice(t("Разрешите режим «Весь трафик» в настройках", "Allow \"All traffic\" mode in settings"));
        setNav(QStringLiteral("settings"));
        setSettingsPage(QStringLiteral("Network"));
        return;
    }
    auto run = std::make_shared<AutoRun>();
    run->initial = initial;
    run->failed = failed;
    run->look = look;
    run->light = periodic && nowMillis() - m_autoDeepAt < kDeepMs;
    if (m_connectedSub && m_connectedProfile)
        run->current = QPair(*m_connectedSub, *m_connectedProfile);
    else if (m_settings.sub && m_settings.profile)
        run->current = QPair(*m_settings.sub, *m_settings.profile);
    try {
        run->tune = options();
    } catch (const Fail &f) {
        setNotice(f.text());
        return;
    }
    run->tune.remove("servers");
    run->tune.remove("subRouting");
    run->tune.remove("chain");
    run->timeout = std::clamp(m_settings.pingTimeout, 3, 8);
    run->tune["pingTimeout"] = run->timeout;
    run->pingUrl = m_settings.pingUrl;
    for (const Sub &s : std::as_const(m_subs))
        if (!m_settings.autoSkipSubs.contains(s.id))
            if (const auto body = Store::body(s.id))
                run->bodies[s.id] = *body;
    if (run->bodies.isEmpty() && !look) {
        setNotice(t("Добавьте подписку", "Add a subscription"));
        return;
    }
    run->learned = loadLearned();
    run->network = networkName();
    run->small = m_settings.autoSaveData && metered();
    const QStringList wanted = m_settings.autoTransports;
    run->prefer = [wanted](const AutoSelect::Entry &e) { return wanted.contains(AutoSelect::transport(e.server)); };
    m_autoState = run;
    m_autoPicking = true;
    m_auto.stop();
    if (initial) {
        m_pendingServer.clear();
        setPhase(1);
        QTimer::singleShot(60000, this, [this, run] {
            if (m_autoState == run)
                autoDone(run);
        });
    }
    QVariantMap info = m_autoInfo;
    info["running"] = true;
    m_autoInfo = info;
    emit autoChanged();
    run->describing = int(run->bodies.size());
    if (run->describing == 0) {
        autoStart(run);
        return;
    }
    const auto chain = m_settings.chainEntry();
    for (auto it = run->bodies.cbegin(); it != run->bodies.cend(); ++it) {
        const QString sub = it.key();
        m_core->request(QJsonObject{{"cmd", "describe"}, {"text", it.value()}}, [this, run, sub, chain](const QJsonObject &r) {
            if (m_autoState != run)
                return;
            const auto it = std::find_if(m_subs.cbegin(), m_subs.cend(), [&](const Sub &s) { return s.id == sub; });
            const QString title = it != m_subs.cend() ? it->name() : QString();
            const QString panelTitle = it != m_subs.cend() ? it->title.value_or(QString()) : QString();
            const QStringList pins = m_settings.pinned(sub), skip = m_settings.autoSkip.value(sub);
            const auto pings = sub == m_settings.sub ? m_pings : m_pingsBySub.value(sub);
            const bool marks = m_settings.autoWlMarked();
            auto &list = run->bySub[sub];
            for (const Server &p : parseServers(r, sub)) {
                const bool group = !p.members.isEmpty();
                for (const Server &s : group ? p.members : QList<Server>{p}) {
                    const auto covers = [&](const QStringList &l) { return l.contains(s.remarks) || (group && l.contains(p.remarks)); };
                    if (!s.supported || s.placeholder || covers(skip) || (chain && *chain == QPair(sub, s.remarks)))
                        continue;
                    AutoSelect::Entry e;
                    e.sub = sub;
                    e.subTitle = title;
                    e.server = s;
                    const QString sig = AutoSelect::sig(s);
                    e.yes = run->learned.yes.contains(sig);
                    e.no = run->learned.no.contains(sig);
                    e.pinned = covers(pins);
                    if (const auto ms = pings.value(s.remarks).ms; ms && *ms >= 0)
                        e.prior = *ms;
                    if (marks)
                        e.mark = m_settings.isAutoWl(sub, s.remarks) || (group && m_settings.isAutoWl(sub, p.remarks));
                    e.named = AutoSelect::namedForWhitelists(s.remarks, panelTitle);
                    list << e;
                }
            }
            if (--run->describing == 0)
                autoStart(run);
        });
    }
}

void App::autoStart(const std::shared_ptr<AutoRun> &run)
{
    for (bool more = true; more;) {
        more = false;
        for (const Sub &s : std::as_const(m_subs)) {
            auto &list = run->bySub[s.id];
            if (!list.isEmpty()) {
                run->all << list.takeFirst();
                more = true;
            }
        }
    }
    run->bySub.clear();
    if (run->all.isEmpty()) {
        autoDone(run);
        return;
    }
    QList<int> every, ordinary;
    const bool holdReserve = m_settings.autoWhitelist != u"always";
    for (int i = 0; i < run->all.size(); ++i) {
        if (holdReserve && run->all[i].reserve())
            continue;
        every << i;
        if (!run->all[i].whitelist())
            ordinary << i;
    }
    const int batch = run->small ? 5 : run->initial ? 16 : run->light ? 8 : 32;
    QList<int> first = AutoSelect::order(run->all, ordinary, run->current, run->prefer).mid(0, batch);
    for (int i : AutoSelect::whitelistOrder(run->all, every, run->current, run->prefer).mid(0, run->small || run->light ? 3 : 8))
        if (!first.contains(i))
            first << i;
    run->waiting = 2;
    if (m_settings.autoWhitelist == u"auto") {
        autoDirect(run, AutoSelect::hosts(m_settings.autoHome), AutoSelect::hosts(m_settings.autoFree), run->initial,
                   [this, run](const QList<AutoSelect::Probe> &probes) {
                       run->probes = probes;
                       autoFirst(run);
                   });
    } else {
        QTimer::singleShot(0, this, [this, run] { autoFirst(run); });
    }
    autoPings(run, first, [this, run] { autoFirst(run); });
}

void App::autoDirect(const std::shared_ptr<AutoRun> &run, const QStringList &home, const QStringList &free, bool early,
                     std::function<void(const QList<AutoSelect::Probe> &)> done)
{
    struct Direct {
        QList<AutoSelect::Probe> all;
        QSet<int> got;
        bool over = false;
    };
    auto d = std::make_shared<Direct>();
    for (const QString &h : home)
        d->all << AutoSelect::Probe{h, -1, true};
    for (const QString &h : free)
        d->all << AutoSelect::Probe{h, -1, false};
    const auto finish = [this, run, d, done](bool cut) {
        if (d->over || m_autoState != run)
            return;
        d->over = true;
        QList<AutoSelect::Probe> out;
        for (int k = 0; k < d->all.size(); ++k)
            if (!cut || d->got.contains(k))
                out << d->all[k];
        done(out);
    };
    if (d->all.isEmpty()) {
        QTimer::singleShot(0, this, [finish] { finish(false); });
        return;
    }
    QTimer::singleShot(kProbeMs * 2 + 1000, this, [finish] { finish(false); });
    for (int k = 0; k < d->all.size(); ++k) {
        const auto &p = d->all[k];
        m_core->request(QJsonObject{{"cmd", "direct"}, {"url", AutoSelect::directUrl(p.host, p.home)}, {"timeoutMs", kProbeMs * 2}},
                        [d, k, early, finish](const QJsonObject &r) {
                            if (d->over)
                                return;
                            auto &p = d->all[k];
                            p.ms = r.contains("ms") ? std::max(1, r.value("ms").toInt()) : -1;
                            d->got << k;
                            if (early && !p.home && p.ok())
                                finish(true);
                            else if (d->got.size() == d->all.size())
                                finish(false);
                        });
    }
}

void App::autoBatch(const std::shared_ptr<AutoRun> &run, const QList<int> &which, const QString &url, bool race,
                    std::function<void(const QHash<int, QJsonObject> &)> done)
{
    auto b = std::make_shared<AutoBatch>();
    b->queue = which;
    b->url = url;
    b->race = race;
    b->done = std::move(done);
    QTimer::singleShot(which.isEmpty() ? 0 : run->timeout * 4000 + 10000, this, [this, run, b] { autoBatchEnd(run, b); });
    autoBatchSend(run, b);
}

void App::autoBatchSend(const std::shared_ptr<AutoRun> &run, const std::shared_ptr<AutoBatch> &b)
{
    const int parallel = std::clamp(m_settings.pingParallel, 1, 16);
    while (!b->over && b->active < parallel && b->next < b->queue.size()) {
        const int i = b->queue[b->next++];
        ++b->active;
        const auto &e = run->all[i];
        QJsonObject opts = run->tune;
        opts["pingUrl"] = b->url;
        opts["dns"] = strings(m_settings.dnsFor(e.sub, e.server.remarks));
        m_core->request(QJsonObject{{"cmd", "probe"}, {"text", run->bodies.value(e.sub)}, {"profile", e.server.remarks}, {"options", opts}},
                        [this, run, b, i](const QJsonObject &r) {
                            if (b->over || m_autoState != run)
                                return;
                            --b->active;
                            b->got[i] = r;
                            if (b->race && r.contains("ms") && !b->raced) {
                                b->raced = true;
                                QTimer::singleShot(kRaceMs, this, [this, run, b] { autoBatchEnd(run, b); });
                            }
                            if (b->got.size() == b->queue.size())
                                autoBatchEnd(run, b);
                            else
                                autoBatchSend(run, b);
                        });
    }
}

void App::autoBatchEnd(const std::shared_ptr<AutoRun> &run, const std::shared_ptr<AutoBatch> &b)
{
    if (b->over || m_autoState != run)
        return;
    b->over = true;
    b->done(b->got);
}

void App::autoPings(const std::shared_ptr<AutoRun> &run, const QList<int> &which, std::function<void()> then)
{
    autoBatch(run, which, run->pingUrl, run->initial, [run, then](const QHash<int, QJsonObject> &got) {
        for (auto it = got.cbegin(); it != got.cend(); ++it) {
            const QJsonObject &r = it.value();
            run->all[it.key()].samples << (r.contains("ms") ? std::max(1, r.value("ms").toInt()) : -1);
            if (!r.contains("ms"))
                run->errors[it.key()] = r.value("error").toString();
        }
        then();
    });
}

void App::autoFirst(const std::shared_ptr<AutoRun> &run)
{
    if (m_autoState != run || --run->waiting > 0)
        return;
    using AutoSelect::Net;
    const QString wl = m_settings.autoWhitelist;
    run->mode = wl == u"always" ? Net::Whitelist : wl == u"never" ? Net::Free : AutoSelect::classify(run->probes);
    if (run->mode == Net::Whitelist && wl == u"auto" && !run->initial) {
        autoDirect(run, {}, AutoSelect::hosts(m_settings.autoFree), false, [this, run](const QList<AutoSelect::Probe> &again) {
            if (std::any_of(again.begin(), again.end(), [](const auto &p) { return p.ok(); })) {
                run->probes.removeIf([](const auto &p) { return !p.home; });
                run->probes << again;
                run->mode = AutoSelect::Net::Free;
            }
            autoWiden(run);
        });
        return;
    }
    autoWiden(run);
}

void App::autoWiden(const std::shared_ptr<AutoRun> &run)
{
    run->all.removeIf([&](const AutoSelect::Entry &e) { return AutoSelect::heldBack(m_settings, run->mode, e.reserve()); });
    const auto &all = run->all;
    if (std::none_of(all.begin(), all.end(), [](const auto &e) { return e.answered(); })) {
        QList<int> rest;
        for (int i = 0; i < all.size(); ++i)
            if (!all[i].tried())
                rest << i;
        rest = run->mode == AutoSelect::Net::Free ? AutoSelect::order(all, rest, run->current, run->prefer)
                                                  : AutoSelect::whitelistOrder(all, rest, run->current, run->prefer);
        autoPings(run, rest.mid(0, run->small ? 5 : 24), [this, run] { autoKnown(run); });
        return;
    }
    autoKnown(run);
}

void App::autoKnown(const std::shared_ptr<AutoRun> &run)
{
    QList<int> known;
    if (run->mode == AutoSelect::Net::Whitelist)
        for (int i = 0; i < run->all.size() && known.size() < 8; ++i)
            if (run->all[i].whitelist() && !run->all[i].tried())
                known << i;
    autoPings(run, known, [this, run] { autoLead(run, 0); });
}

void App::autoLead(const std::shared_ptr<AutoRun> &run, int round)
{
    const auto &all = run->all;
    QList<int> lead;
    for (int i = 0; i < all.size(); ++i)
        if (all[i].answered())
            lead << i;
    std::stable_sort(lead.begin(), lead.end(), [&](int a, int b) {
        return std::pair(!run->prefer(all[a]), all[a].samples.first()) < std::pair(!run->prefer(all[b]), all[b].samples.first());
    });
    lead = lead.mid(0, 3);
    for (int i = 0; i < all.size(); ++i)
        if (run->current && *run->current == all[i].target() && all[i].answered() && !lead.contains(i))
            lead << i;
    if (run->initial || lead.size() < 2 || round >= (run->light ? 1 : 2)) {
        autoCarry(run, 0);
        return;
    }
    autoPings(run, lead, [this, run, round] { autoLead(run, round + 1); });
}

void App::autoCarry(const std::shared_ptr<AutoRun> &run, int round)
{
    auto &all = run->all;
    if (run->mode != AutoSelect::Net::Whitelist) {
        autoDone(run);
        return;
    }
    if (round == 0) {
        const auto best = [&](int i) {
            int b = INT_MAX;
            for (int ms : all[i].samples)
                if (ms >= 0)
                    b = std::min(b, ms);
            return b;
        };
        run->carryLeft.clear();
        for (int i = 0; i < all.size(); ++i)
            if (all[i].answered())
                run->carryLeft << i;
        std::stable_sort(run->carryLeft.begin(), run->carryLeft.end(), [&](int a, int b) {
            const auto key = [&](int i) { return std::tuple(!all[i].whitelist(), !(run->current && *run->current == all[i].target()), best(i)); };
            return key(a) < key(b);
        });
    }
    const bool carried = std::any_of(all.begin(), all.end(), [](const auto &e) { return e.carried; });
    if (round >= 3 || run->carryLeft.isEmpty() || carried) {
        autoDone(run);
        return;
    }
    const QList<int> chunk = run->carryLeft.mid(0, run->small ? 3 : 6);
    run->carryLeft = run->carryLeft.mid(chunk.size());
    const auto mark = [run](const QHash<int, QJsonObject> &got) {
        QList<int> unknown;
        for (auto it = got.cbegin(); it != got.cend(); ++it) {
            if (it.value().contains("ms"))
                run->all[it.key()].carried = true;
            else if (tooSlow(it.value()))
                run->all[it.key()].squeezed = true;
            else
                unknown << it.key();
        }
        return unknown;
    };
    autoBatch(run, chunk, kCarryUrl, false, [this, run, chunk, mark, round](const QHash<int, QJsonObject> &got) {
        QList<int> again = mark(got);
        for (int i : chunk)
            if (!got.contains(i))
                again << i;
        autoBatch(run, again, kCarryAlt, false, [this, run, mark, round](const QHash<int, QJsonObject> &got) {
            mark(got);
            autoCarry(run, round + 1);
        });
    });
}

void App::autoDone(const std::shared_ptr<AutoRun> &run)
{
    if (m_autoState != run || std::exchange(run->finished, true))
        return;
    using AutoSelect::Net;
    const auto &all = run->all;
    QHash<QString, int> speeds;
    for (const QVariant &v : speedHistory()) {
        const QVariantMap m = v.toMap();
        if (const QString name = m.value("name").toString(); !speeds.contains(name))
            speeds[name] = std::clamp(int(m.value("download").toDouble() / 1e6 / 5), 0, 40);
    }
    const auto pick = AutoSelect::choose(all, run->mode, m_settings, run->current, run->failed, run->initial, speeds);
    if (run->mode == Net::Whitelist) {
        QStringList passed, failed, squeezed;
        for (const auto &e : all) {
            if (!e.tried())
                continue;
            const QString sig = AutoSelect::sig(e.server);
            if (e.carried && !passed.contains(sig))
                passed << sig;
            if (!e.answered() && !failed.contains(sig))
                failed << sig;
            if (e.squeezed && !squeezed.contains(sig))
                squeezed << sig;
        }
        const auto learned = AutoSelect::learn(loadLearned(), passed, failed, squeezed);
        Store::write(learnedFile(), QJsonDocument(QJsonObject{{"yes", strings(learned.yes)}, {"no", strings(learned.no)}}).toJson(QJsonDocument::Compact));
    }
    bool pinged = false;
    for (int i = 0; i < all.size(); ++i) {
        if (!all[i].tried())
            continue;
        const int ms = all[i].ms();
        const Ping p = ms >= 0 ? Ping{ms, {}} : Ping{std::nullopt, run->errors.value(i, t("Не проходит", "Fails"))};
        if (all[i].sub == m_settings.sub) {
            m_pings[all[i].server.remarks] = p;
            pinged = true;
        } else {
            m_pingsBySub[all[i].sub][all[i].server.remarks] = p;
        }
    }
    if (pinged)
        emit pingsChanged();

    QList<int> shown;
    for (int i = 0; i < all.size(); ++i)
        if (all[i].tried())
            shown << i;
    std::stable_sort(shown.begin(), shown.end(), [&](int a, int b) {
        return std::pair(all[a].ms() < 0, pick.scores.value(a)) < std::pair(all[b].ms() < 0, pick.scores.value(b));
    });
    QVariantList candidates;
    for (int i : shown.mid(0, 12)) {
        const auto &e = all[i];
        candidates << QVariantMap{{"sub", e.sub}, {"profile", e.server.remarks}, {"name", e.server.name}, {"flag", e.server.flag},
                                  {"subTitle", e.subTitle}, {"ms", e.ms()}, {"score", pick.scores.value(i)}, {"whitelist", e.whitelist()},
                                  {"learned", e.yes}};
    }
    QVariantList probes;
    for (const auto &p : run->probes)
        probes << QVariantMap{{"host", p.host}, {"ms", p.ms}, {"home", p.home}};
    const QVariantMap prev = m_autoInfo;
    QVariantMap info{{"at", nowMillis()}, {"mode", AutoSelect::netKey(run->mode)}, {"network", run->network}, {"probes", probes},
                     {"candidates", candidates}, {"total", int(all.size())}, {"running", false}, {"next", 0}};
    if (pick.chosen)
        info["chosen"] = QVariantMap{{"sub", all[*pick.chosen].sub}, {"profile", all[*pick.chosen].server.remarks}};
    publishAuto(info);
    if (run->look) {
        m_autoState.reset();
        m_autoPicking = false;
        return;
    }
    if (!run->initial) {
        m_autoFullAt = nowMillis();
        if (!run->light)
            m_autoDeepAt = m_autoFullAt;
    }

    const auto settle = [this] {
        m_autoState.reset();
        m_autoPicking = false;
    };
    if (!pick.chosen) {
        settle();
        if (run->initial) {
            m_failure = t("Нет доступных серверов", "No reachable servers");
            setPhase(3);
        } else if (run->failed) {
            reconnectAfter(t("Нет доступных серверов", "No reachable servers"));
        } else {
            autoSchedule();
        }
        return;
    }
    const auto &c = all[*pick.chosen];
    const Target target = c.target();
    if (!run->initial && !run->failed && target == run->current) {
        settle();
        autoSchedule();
        return;
    }
    if (!run->initial && m_settings.autoNotify && run->current)
        autoNotify(prev, info, *run->current, c, run->failed);
    if (m_settings.sub)
        m_pingsBySub[*m_settings.sub] = m_pings;
    Settings next = m_settings;
    next.sub = c.sub;
    next.profile = c.server.remarks;
    changeSettings(next);
    m_pings = m_pingsBySub.value(c.sub);
    emit pingsChanged();
    emit subsChanged();
    loadServers([this, run] {
        if (m_autoState != run || !m_settings.autoSelect || m_closed)
            return;
        m_autoState.reset();
        m_autoPicking = false;
        if (run->initial)
            m_phase = 0;
        connectSelected();
    });
}

void App::autoNotify(const QVariantMap &prev, const QVariantMap &now, const QPair<QString, QString> &from, const AutoSelect::Entry &to, bool failed)
{
    QString fromName = from.second;
    int fromMs = -1;
    for (const QVariant &v : now.value("candidates").toList()) {
        const QVariantMap m = v.toMap();
        if (m.value("sub").toString() == from.first && m.value("profile").toString() == from.second) {
            fromName = m.value("name").toString();
            fromMs = m.value("ms").toInt();
        }
    }
    const QString toName = QStringList{to.server.flag, to.server.name}.join(u' ').trimmed();
    const QString toMs = to.ms() >= 0 ? t(" (%1 мс)", " (%1 ms)").arg(to.ms()) : QString();
    const QString was = prev.value("mode").toString(), mode = now.value("mode").toString();
    QString title, text;
    if (mode == u"whitelist" && was != u"whitelist") {
        title = t("Похоже, включили белые списки", "Looks like whitelists are on");
        text = t("Зарубежные сайты не открываются напрямую. Переключился на «%1»%2.", "Foreign sites don't open directly. Switched to \"%1\"%2.").arg(toName, toMs);
    } else if (mode == u"free" && was == u"whitelist") {
        title = t("Обычный интернет вернулся", "Open internet is back");
        text = t("Зарубежные сайты снова открываются. Переключился на «%1»%2.", "Foreign sites open again. Switched to \"%1\"%2.").arg(toName, toMs);
    } else if (failed) {
        title = t("«%1» не отвечает", "\"%1\" isn't responding").arg(fromName);
        text = t("Переключился на «%1»%2.", "Switched to \"%1\"%2.").arg(toName, toMs);
    } else {
        title = t("Нашёлся сервер быстрее", "Found a faster server");
        text = fromMs >= 0 ? t("«%1»%2 против %3 мс у «%4». Переключился.", "\"%1\"%2 vs %3 ms on \"%4\". Switched.").arg(toName, toMs).arg(fromMs).arg(fromName)
                           : t("Переключился на «%1»%2.", "Switched to \"%1\"%2.").arg(toName, toMs);
    }
    setNotice(title);
    if (m_settings.notifications && !m_focused)
        notify(title, text);
}

void App::autoChoose(const QString &sub, const QString &remarks)
{
    const bool connecting = m_phase == 1;
    if (connecting && m_settings.sub == sub && m_settings.profile == remarks && !m_settings.autoSelect)
        return;
    if (connecting)
        disconnectVpn();
    const bool reconnect = connecting || m_phase == 2 || m_phase == 3 || m_blocked;
    if (m_settings.sub)
        m_pingsBySub[*m_settings.sub] = m_pings;
    Settings next = m_settings;
    next.autoSelect = false;
    next.sub = sub;
    next.profile = remarks;
    changeSettings(next);
    m_pings = m_pingsBySub.value(sub);
    emit pingsChanged();
    emit subsChanged();
    loadServers([this, reconnect] {
        if (reconnect)
            connectVpn();
    });
}

void App::loadAutoServers()
{
    const int generation = ++m_autoServersGen;
    struct Load {
        int left = 0;
        QHash<QString, QList<Server>> bySub;
    };
    auto load = std::make_shared<Load>();
    QHash<QString, QString> bodies;
    for (const Sub &s : std::as_const(m_subs))
        if (const auto body = Store::body(s.id))
            bodies[s.id] = *body;
    load->left = int(bodies.size());
    const auto finish = [this, load, generation] {
        if (generation != m_autoServersGen)
            return;
        const auto learned = loadLearned();
        QVariantList groups;
        QSet<QString> have;
        m_autoServers.clear();
        for (const Sub &sub : std::as_const(m_subs)) {
            const QList<Server> list = load->bySub.value(sub.id);
            if (list.isEmpty())
                continue;
            QVariantList servers;
            for (const Server &s : list) {
                servers << QVariantMap{{"remarks", s.remarks}, {"name", s.name}, {"flag", s.flag}, {"passed", learned.yes.contains(AutoSelect::sig(s))}};
                have << AutoSelect::transport(s);
                m_autoServers[sub.id] << s.remarks;
            }
            groups << QVariantMap{{"sub", sub.id}, {"title", sub.name()}, {"servers", servers}};
        }
        QStringList transports;
        for (const QString &t : AutoSelect::TRANSPORTS)
            if (have.contains(t))
                transports << t;
        Settings next = m_settings;
        for (auto it = next.autoSkip.begin(); it != next.autoSkip.end();) {
            const QStringList known = m_autoServers.value(it.key());
            QStringList kept;
            for (const QString &r : std::as_const(it.value()))
                if (known.contains(r))
                    kept << r;
            if (kept.isEmpty())
                it = next.autoSkip.erase(it);
            else {
                it.value() = kept;
                ++it;
            }
        }
        if (next.autoSkip != m_settings.autoSkip)
            changeSettings(next);
        emit autoServersReady(groups, transports);
    };
    if (bodies.isEmpty()) {
        QTimer::singleShot(0, this, finish);
        return;
    }
    for (auto it = bodies.cbegin(); it != bodies.cend(); ++it) {
        const QString sub = it.key();
        m_core->request(QJsonObject{{"cmd", "describe"}, {"text", it.value()}}, [this, load, sub, finish](const QJsonObject &r) {
            for (const Server &p : parseServers(r, sub))
                for (const Server &s : p.members.isEmpty() ? QList<Server>{p} : p.members)
                    if (s.supported && !s.placeholder)
                        load->bySub[sub] << s;
            if (--load->left == 0)
                finish();
        });
    }
}

void App::autoSkipSub(const QString &sub, bool on)
{
    Settings next = m_settings;
    next.autoSkipSubs.removeAll(sub);
    if (on)
        next.autoSkipSubs << sub;
    next.autoSkip.remove(sub);
    changeSettings(next);
}

void App::autoSkipServer(const QString &sub, const QString &remarks, bool on)
{
    Settings next = m_settings;
    if (next.autoSkipSubs.contains(sub)) {
        next.autoSkipSubs.removeAll(sub);
        next.autoSkip[sub] = m_autoServers.value(sub);
    }
    QStringList now = next.autoSkip.value(sub);
    now.removeAll(remarks);
    if (on)
        now << remarks;
    if (now.isEmpty())
        next.autoSkip.remove(sub);
    else
        next.autoSkip[sub] = now;
    changeSettings(next);
}

void App::autoMarkServer(const QString &sub, const QString &remarks, bool on)
{
    Settings next = m_settings;
    QStringList now = next.autoWl.value(sub);
    if (next.autoWlSubs.contains(sub)) {
        next.autoWlSubs.removeAll(sub);
        now = m_autoServers.value(sub);
    }
    now.removeAll(remarks);
    if (on)
        now << remarks;
    if (now.isEmpty())
        next.autoWl.remove(sub);
    else
        next.autoWl[sub] = now;
    changeSettings(next);
}

void App::autoMarkSub(const QString &sub, bool on)
{
    Settings next = m_settings;
    next.autoWlSubs.removeAll(sub);
    if (on)
        next.autoWlSubs << sub;
    next.autoWl.remove(sub);
    changeSettings(next);
}

QString App::autoHosts(const QString &text) const
{
    static const QRegularExpression site(QStringLiteral(R"(^(https?://)?[\w.-]+\.[a-z]{2,}(/\S*)?$)"), QRegularExpression::CaseInsensitiveOption);
    const QStringList list = AutoSelect::hosts(text);
    if (list.isEmpty() || !std::all_of(list.begin(), list.end(), [](const QString &h) { return site.match(h).hasMatch(); }))
        return {};
    return list.join(QStringLiteral(", "));
}
