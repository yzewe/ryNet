#include "reports.h"

#include "model.h"
#include "platform.h"
#include "store.h"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QMutex>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QRegularExpression>
#include <QUuid>
#include <QtEndian>

namespace {
QMutex stateLock;

void put16(QByteArray &b, quint16 v)
{
    char c[2];
    qToLittleEndian(v, c);
    b.append(c, 2);
}

void put32(QByteArray &b, quint32 v)
{
    char c[4];
    qToLittleEndian(v, c);
    b.append(c, 4);
}

void saveState(const QString &dir, const Reports::State &s)
{
    QJsonArray sends;
    for (qint64 at : s.sends)
        sends.append(at);
    Store::write(QDir(dir).filePath(QStringLiteral("reports.json")),
                 QJsonDocument(QJsonObject{{"install", s.install},
                                           {"worked", s.workedMs},
                                           {"offset", s.offset},
                                           {"sentAt", s.sentAt},
                                           {"mark", QString::fromLatin1(s.mark.toBase64())},
                                           {"sends", sends}})
                     .toJson(QJsonDocument::Compact));
}

constexpr qsizetype MARK = 160;

QByteArray markOf(const QByteArray &log) { return log.right(MARK); }

qint64 resumeAt(const QByteArray &log, qint64 offset, const QByteArray &mark)
{
    if (mark.isEmpty())
        return offset >= 0 && offset <= log.size() ? offset : 0;
    if (offset >= mark.size() && offset <= log.size() && log.mid(offset - mark.size(), mark.size()) == mark)
        return offset;
    const qsizetype at = log.lastIndexOf(mark);
    return at >= 0 ? at + mark.size() : 0;
}
}

Reports::State Reports::state(const QString &dir)
{
    QMutexLocker lock(&stateLock);
    QFile f(QDir(dir).filePath(QStringLiteral("reports.json")));
    QJsonObject o;
    if (f.open(QIODevice::ReadOnly))
        o = QJsonDocument::fromJson(f.readAll()).object();
    static const QRegularExpression hex(QStringLiteral("^[0-9a-f]{32}$"));
    State s;
    s.install = o.value("install").toString();
    if (!hex.match(s.install).hasMatch())
        s.install = QUuid::createUuid().toString(QUuid::Id128);
    s.workedMs = qint64(o.value("worked").toDouble());
    s.offset = qint64(o.value("offset").toDouble());
    s.sentAt = qint64(o.value("sentAt").toDouble());
    s.mark = QByteArray::fromBase64(o.value("mark").toString().toLatin1());
    for (const QJsonValue &v : o.value("sends").toArray())
        s.sends.append(qint64(v.toDouble()));
    return s;
}

bool Reports::worked(qint64 ms, const QString &dir)
{
    State s = state(dir);
    s.workedMs += std::clamp<qint64>(ms, 0, EVERY_MS);
    QMutexLocker lock(&stateLock);
    saveState(dir, s);
    return s.workedMs >= EVERY_MS;
}

bool Reports::allowed(const State &s, qint64 now)
{
    const qsizetype today = std::count_if(s.sends.begin(), s.sends.end(), [now](qint64 at) { return at <= now && now - at < DAY_MS; });
    return (now - s.sentAt >= GAP_MS || now < s.sentAt) && today < PER_DAY;
}

qint64 Reports::unsent(const QString &dir)
{
    State s = state(dir);
    QFile log(QDir(dir).filePath(QStringLiteral("core.log")));
    if (!log.open(QIODevice::ReadOnly))
        return 0;
    const qint64 size = log.size();
    if (s.offset <= size && (s.mark.isEmpty() || (s.offset >= s.mark.size() && log.seek(s.offset - s.mark.size()) && log.read(s.mark.size()) == s.mark)))
        return size - s.offset;
    if (!log.seek(0))
        return 0;
    const QByteArray all = log.readAll();
    const qint64 from = resumeAt(all, s.offset, s.mark);
    if (from > 0 || s.mark.isEmpty()) {
        s.offset = from;
        QMutexLocker lock(&stateLock);
        saveState(dir, s);
    }
    return all.size() - from;
}

void Reports::skipToEnd(const QString &dir)
{
    State s = state(dir);
    s.workedMs = 0;
    QFile log(QDir(dir).filePath(QStringLiteral("core.log")));
    s.offset = log.size();
    s.mark.clear();
    if (s.offset > 0 && log.open(QIODevice::ReadOnly) && log.seek(std::max<qint64>(0, s.offset - MARK)))
        s.mark = log.readAll();
    QMutexLocker lock(&stateLock);
    saveState(dir, s);
}

QByteArray Reports::zip(const QList<QPair<QString, QByteArray>> &files)
{
    QByteArray out, central;
    const QDateTime now = QDateTime::currentDateTime();
    const quint16 time = quint16((now.time().hour() << 11) | (now.time().minute() << 5) | (now.time().second() / 2));
    const quint16 date = quint16(((now.date().year() - 1980) << 9) | (now.date().month() << 5) | now.date().day());
    for (const auto &[name, data] : files) {
        const QByteArray packed = Zlib::deflateRaw(data);
        const quint32 crc = Zlib::crc32(data);
        const QByteArray n = name.toUtf8();
        const quint32 offset = quint32(out.size());
        put32(out, 0x04034b50);
        put16(out, 20);
        put16(out, 0x0800);
        put16(out, 8);
        put16(out, time);
        put16(out, date);
        put32(out, crc);
        put32(out, quint32(packed.size()));
        put32(out, quint32(data.size()));
        put16(out, quint16(n.size()));
        put16(out, 0);
        out += n;
        out += packed;
        put32(central, 0x02014b50);
        put16(central, 20);
        put16(central, 20);
        put16(central, 0x0800);
        put16(central, 8);
        put16(central, time);
        put16(central, date);
        put32(central, crc);
        put32(central, quint32(packed.size()));
        put32(central, quint32(data.size()));
        put16(central, quint16(n.size()));
        put16(central, 0);
        put16(central, 0);
        put16(central, 0);
        put16(central, 0);
        put32(central, 0);
        put32(central, offset);
        central += n;
    }
    const quint32 start = quint32(out.size());
    out += central;
    put32(out, 0x06054b50);
    put16(out, 0);
    put16(out, 0);
    put16(out, quint16(files.size()));
    put16(out, quint16(files.size()));
    put32(out, quint32(central.size()));
    put32(out, start);
    put16(out, 0);
    return out;
}

void Reports::send(QNetworkAccessManager *net, const QJsonObject &info, const QString &dir, const QString &url, std::function<void(bool)> done)
{
    struct Packet {
        QByteArray zip;
        State state;
        qint64 length = 0;
        QByteArray mark;
        bool empty = false;
        bool failed = false;
    };
    Platform::background<Packet>(
        net,
        [dir, info] {
            Packet p;
            p.state = state(dir);
            QFile log(QDir(dir).filePath(QStringLiteral("core.log")));
            if (log.exists() && !log.open(QIODevice::ReadOnly)) {
                p.failed = true;
                return p;
            }
            const QByteArray all = log.isOpen() ? log.readAll() : QByteArray();
            p.length = all.size();
            p.mark = markOf(all);
            const qint64 from = resumeAt(all, p.state.offset, p.state.mark);
            if (p.length - from <= 0) {
                p.empty = true;
                return p;
            }
            const qint64 start = std::max(from, p.length - MAX_BYTES);
            const QByteArray bytes = all.mid(start);
            p.zip = zip({{QStringLiteral("core.log"), bytes}, {QStringLiteral("info.json"), QJsonDocument(info).toJson(QJsonDocument::Indented)}});
            return p;
        },
        [net, dir, url, done](Packet p) {
            auto finish = [dir, done, mark = p.mark](const State &s, qint64 offset, bool sent) {
                State next = s;
                next.workedMs = 0;
                next.offset = offset;
                next.mark = mark;
                if (sent) {
                    next.sentAt = nowMillis();
                    next.sends.removeIf([at = next.sentAt](qint64 t) { return t > at || at - t >= DAY_MS; });
                    next.sends.append(next.sentAt);
                }
                {
                    QMutexLocker lock(&stateLock);
                    saveState(dir, next);
                }
                done(true);
            };
            if (p.failed) {
                done(false);
                return;
            }
            if (p.empty) {
                finish(p.state, p.length, false);
                return;
            }
            QNetworkRequest req{QUrl(url)};
            req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/zip"));
            req.setRawHeader("X-Rynet-Install", p.state.install.toLatin1());
            req.setRawHeader("X-Rynet-Version",
                             QStringLiteral("desktop %1 (%2)").arg(Platform::version(), Platform::windows() ? QStringLiteral("windows") : QStringLiteral("linux")).toUtf8());
            req.setTransferTimeout(60000);
            QNetworkReply *reply = net->post(req, p.zip);
            QObject::connect(reply, &QNetworkReply::finished, reply, [reply, p, finish, done] {
                reply->deleteLater();
                const int code = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
                if (code == 201)
                    finish(p.state, p.length, true);
                else if (code == 400 || code == 413 || code == 415)
                    finish(p.state, p.length, false);
                else
                    done(false);
            });
        });
}
