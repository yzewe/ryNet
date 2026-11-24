#pragma once

#include <QByteArray>
#include <QJsonObject>
#include <QList>
#include <QString>
#include <optional>

namespace Update {
constexpr qsizetype MAX_RELEASES = 64 << 10;
constexpr qsizetype MAX_DOWNLOADS = 256 << 10;
constexpr qsizetype MAX_CHANGELOG = 4000;
constexpr qint64 EVERY_MS = 24LL * 3600 * 1000;
constexpr qint64 RETRY_MS = 3600LL * 1000;

QString releasesUrl();
QString downloadsUrl();
QString pageUrl();

std::optional<QList<int>> parseVersion(const QString &text);
bool newer(const QString &have, const QString &latest);

struct Release {
    QString version;
    QString url;
    QString changelog;
};
std::optional<Release> parseRelease(const QByteArray &body);
QString cleanText(const QString &text, qsizetype max);

enum class Install { Unknown, Msi, Setup, Portable, Deb, Rpm, Pacman, AppImage, Tar };
QString alias(Install install);
bool fileMatches(Install install, const QString &version, const QString &file);

struct Download {
    QString file;
    QString sha256;
    QString link;
};
std::optional<Download> parseDownload(const QByteArray &body, Install install, const QString &version);

Install windowsInstall(bool portable, const QString &exeDir, const QString &msiDir, const QString &innoDir);
Install linuxInstall(const QString &appImage, const QString &exe, const QString &home, const QString &owner);
Install detect();

struct Found {
    QString version;
    QString changelog;
    QString post;
    QString file;
    QString link;
    QString sha256;
    QJsonObject toJson() const;
    static std::optional<Found> fromJson(const QJsonObject &o);
};
}
