#pragma once

#include <QJsonObject>
#include <QString>
#include <functional>

class QNetworkAccessManager;
class QObject;

namespace Reports {
constexpr qint64 EVERY_MS = 2LL * 60 * 60 * 1000;
constexpr qint64 RETRY_MS = 30LL * 60 * 1000;
constexpr qint64 MAX_BYTES = 12LL << 20;
constexpr qint64 SOON_BYTES = 5LL << 19;
constexpr qint64 GAP_MS = 15LL * 60 * 1000;
constexpr qint64 DAY_MS = 24LL * 60 * 60 * 1000;
constexpr int PER_DAY = 16;

struct State {
    QString install;
    qint64 workedMs = 0, offset = 0, sentAt = 0;
    QByteArray mark;
    QList<qint64> sends;
};

State state(const QString &dir);
bool worked(qint64 ms, const QString &dir);
bool allowed(const State &s, qint64 now);
qint64 unsent(const QString &dir);
void skipToEnd(const QString &dir);
QByteArray zip(const QList<QPair<QString, QByteArray>> &files);
void send(QNetworkAccessManager *net, const QJsonObject &info, const QString &dir, const QString &url, std::function<void(bool)> done);
}
