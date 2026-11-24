#include "model_detail.h"
#include <QCollator>
#include <QDateTime>
#include <QHash>
#include <QHostAddress>
#include <QJsonDocument>
#include <QLocale>
#include <QRegularExpression>
#include <QSet>
#include <QUrl>
#include <QUuid>
#include <algorithm>
#include <zlib.h>

using namespace ModelDetail;

namespace {
constexpr int kWhitelistPenalty = 100000;
constexpr int kLossMs = 150;
constexpr int kPinMs = 60;
constexpr int kSwitchMinMs = 80;
constexpr int kSwitchShare = 30;
constexpr int kLearnedKeep = 256;
constexpr int kFreeBytes = 32 * 1024;
constexpr int kFreeMs = 1500;

int medianOf(QList<int> ok)
{
    std::sort(ok.begin(), ok.end());
    return ok[ok.size() / 2];
}

QList<int> answers(const QList<int> &samples)
{
    QList<int> ok;
    for (int ms : samples)
        if (ms >= 0)
            ok << ms;
    return ok;
}
}

QString AutoSelect::netKey(Net n)
{
    return n == Net::Whitelist ? QStringLiteral("whitelist") : n == Net::Offline ? QStringLiteral("offline") : QStringLiteral("free");
}

QStringList AutoSelect::hosts(const QString &text)
{
    static const QRegularExpression sep(QStringLiteral("[\\s,;]+"));
    return text.split(sep, Qt::SkipEmptyParts);
}

QString AutoSelect::directUrl(const QString &host, bool home)
{
    QString url;
    if (host.contains(u"://"))
        url = host;
    else if (host == u"google.com" || host == u"www.google.com")
        url = QStringLiteral("https://www.google.com/");
    else if (host == u"cloudflare.com" || host == u"www.cloudflare.com")
        url = Endpoints::SpeedDown + QString::number(kFreeBytes);
    else
        url = QStringLiteral("https://%1/").arg(host);
    return home ? url : url + QStringLiteral("#body=%1&within=%2").arg(kFreeBytes).arg(kFreeMs);
}

AutoSelect::Net AutoSelect::classify(const QList<Probe> &probes)
{
    bool home = false, free = false, homeOk = false, freeOk = false;
    for (const Probe &p : probes) {
        (p.home ? home : free) = true;
        if (p.ok())
            (p.home ? homeOk : freeOk) = true;
    }
    if (freeOk)
        return Net::Free;
    if (homeOk)
        return Net::Whitelist;
    return !home && !free ? Net::Free : Net::Offline;
}

bool AutoSelect::namedForWhitelists(const QString &remarks, const QString &subTitle)
{
    static const QRegularExpression re(QStringLiteral(R"((?<![\p{L}\p{N}])(lte|бс|б/с|wl|whitelists?|бел\p{L}*\s+спис\p{L}*|мобильн\p{L}*)(?![\p{L}\p{N}]))"),
                                       QRegularExpression::CaseInsensitiveOption | QRegularExpression::UseUnicodePropertiesOption);
    return re.match(remarks).hasMatch() || (!subTitle.isEmpty() && re.match(subTitle).hasMatch());
}

bool AutoSelect::isUdp(const Server &s)
{
    static const QRegularExpression re(QStringLiteral(R"(hysteria|tuic|wireguard|\bquic\b)"), QRegularExpression::CaseInsensitiveOption);
    return re.match(s.kind).hasMatch();
}

QString AutoSelect::transport(const Server &s)
{
    for (const QString &t : TRANSPORTS.mid(1))
        if (s.kind.contains(t, Qt::CaseInsensitive))
            return t;
    return TRANSPORTS.first();
}

QString AutoSelect::sig(const Server &s) { return (s.server.isEmpty() ? s.remarks : s.server) + u'|' + s.sni; }

AutoSelect::Learned AutoSelect::learn(const Learned &old, const QStringList &passed, const QStringList &failed, const QStringList &squeezed)
{
    if (passed.isEmpty() && squeezed.isEmpty())
        return old;
    Learned out;
    for (const QString &k : old.yes)
        if (!passed.contains(k) && !squeezed.contains(k))
            out.yes << k;
    out.yes << passed;
    for (const QString &k : old.no)
        if (!passed.contains(k) && !failed.contains(k) && !squeezed.contains(k))
            out.no << k;
    for (const QString &k : failed)
        if (!old.yes.contains(k))
            out.no << k;
    out.no << squeezed;
    out.yes = out.yes.mid(std::max<qsizetype>(0, out.yes.size() - kLearnedKeep));
    out.no = out.no.mid(std::max<qsizetype>(0, out.no.size() - kLearnedKeep));
    return out;
}

int AutoSelect::Entry::ms() const
{
    const QList<int> ok = squeezed ? QList<int>() : answers(samples);
    return ok.isEmpty() ? -1 : medianOf(ok);
}

QList<int> AutoSelect::order(const QList<Entry> &all, const QList<int> &which, const Target &current, const Prefer &prefer)
{
    QList<int> out = which;
    std::stable_sort(out.begin(), out.end(), [&](int a, int b) {
        const auto key = [&](const Entry &e) {
            return std::tuple(!(current && *current == e.target()), !prefer(e), !e.pinned, e.prior.value_or(INT_MAX));
        };
        return key(all[a]) < key(all[b]);
    });
    return out;
}

QList<int> AutoSelect::whitelistOrder(const QList<Entry> &all, const QList<int> &which, const Target &current, const Prefer &prefer)
{
    QList<int> out = which;
    std::stable_sort(out.begin(), out.end(), [&](int a, int b) {
        const auto key = [&](const Entry &e) {
            const int kind = e.whitelist() ? 0 : isUdp(e.server) ? 3 : !e.no ? 1 : 2;
            return std::tuple(kind, !prefer(e), !(current && *current == e.target()), e.prior.value_or(INT_MAX));
        };
        return key(all[a]) < key(all[b]);
    });
    return out;
}

bool AutoSelect::switches(int best, int current, int margin, int bestSamples, int currentSamples)
{
    const int need = std::max({margin, kSwitchMinMs, int(qint64(current) * kSwitchShare / 100)});
    return bestSamples >= 2 && currentSamples >= 2 && qint64(best) + need < current;
}

int AutoSelect::score(const Entry &e, Net mode, bool pins, int speed)
{
    const QList<int> ok = answers(e.samples);
    if (ok.isEmpty())
        return INT_MAX;
    const int loss = int(e.samples.size() - ok.size()) * kLossMs;
    const int pin = pins && e.pinned ? kPinMs : 0;
    const int bypass = mode == Net::Free && e.bypass() ? kWhitelistPenalty : 0;
    return medianOf(ok) + loss - speed - pin + bypass;
}

AutoSelect::Pick AutoSelect::choose(const QList<Entry> &all, Net mode, const Settings &s, const Target &current, bool failed, bool quick,
                                    const QHash<QString, int> &speeds)
{
    Pick out;
    const bool marks = s.autoWlMarked();
    const auto rank = [&](const Entry &e) {
        const int base = e.carried ? 0 : e.squeezed ? 4 : 2;
        if (mode == Net::Whitelist)
            return base + (!marks || e.whitelist() ? 0 : 1);
        return base + (mode == Net::Free && e.bypass() ? 1 : 0);
    };
    std::optional<int> top;
    for (int i = 0; i < all.size(); ++i) {
        if (!all[i].tried())
            continue;
        out.scores[i] = score(all[i], mode, s.autoPins, s.autoSpeed ? speeds.value(all[i].server.name) : 0);
        if (all[i].answered() && (!top || rank(all[i]) < *top))
            top = rank(all[i]);
    }
    if (!top)
        return out;
    QList<int> ranked;
    for (int i = 0; i < all.size(); ++i)
        if (all[i].answered() && rank(all[i]) == *top)
            ranked << i;
    std::stable_sort(ranked.begin(), ranked.end(), [&](int a, int b) { return out.scores[a] < out.scores[b]; });
    QList<int> pool;
    for (int i : ranked)
        if (s.autoTransports.contains(transport(all[i].server)))
            pool << i;
    if (pool.isEmpty())
        pool = ranked;
    const int best = pool.first();
    std::optional<int> cur;
    for (int i : pool)
        if (current && *current == all[i].target())
            cur = i;
    const auto count = [&](int i) { return int(answers(all[i].samples).size()); };
    if (!cur || failed)
        out.chosen = best;
    else if (quick)
        out.chosen = qint64(out.scores[best]) + s.autoMargin < out.scores[*cur] ? best : *cur;
    else
        out.chosen = switches(out.scores[best], out.scores[*cur], s.autoMargin, count(best), count(*cur)) ? best : *cur;
    return out;
}

bool AutoSelect::heldBack(const Settings &, Net mode, bool reserve) { return reserve && mode != Net::Whitelist; }

bool ExitIp::isIp(const QString &s)
{
    static const QRegularExpression v4(QStringLiteral("^(25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)(\\.(25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)){3}$"));
    static const QRegularExpression v6(QStringLiteral("^[0-9A-Fa-f:.]+$"));
    if (v4.match(s).hasMatch())
        return true;
    QHostAddress a;
    return s.contains(u':') && v6.match(s).hasMatch() && a.setAddress(s) && a.protocol() == QAbstractSocket::IPv6Protocol;
}

std::optional<ExitIp::Result> ExitIp::parse(const QByteArray &body)
{
    const QString t = QString::fromUtf8(body.left(16 * 1024)).trimmed();
    if (t.isEmpty())
        return std::nullopt;
    if (!t.startsWith(u'{'))
        return isIp(t) ? std::optional<Result>(Result{t, {}}) : std::nullopt;
    const QJsonDocument doc = QJsonDocument::fromJson(t.toUtf8());
    if (!doc.isObject())
        return std::nullopt;
    const QJsonObject o = doc.object();
    if (!o.value("success").toBool(true))
        return std::nullopt;
    Result r;
    for (const char *k : {"ip", "query", "address", "origin", "ipAddress", "ip_addr"})
        if (const QString v = o.value(QLatin1String(k)).toString().trimmed(); isIp(v)) {
            r.ip = v;
            break;
        }
    if (r.ip.isEmpty())
        return std::nullopt;
    static const QRegularExpression code(QStringLiteral("^[A-Z]{2}$"));
    for (const char *k : {"country_code", "countryCode", "country"})
        if (const QString v = o.value(QLatin1String(k)).toString().trimmed().toUpper(); code.match(v).hasMatch()) {
            r.country = v;
            break;
        }
    return r;
}
