#include "endpoints.h"
#include "update.h"

#include "platform.h"
#include "store.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QRegularExpression>
#include <QSettings>
#include <QSysInfo>
#include <QUrl>
#include <tuple>

namespace Update {
namespace {
const QString kHost = Endpoints::DownloadsHost;

bool isHex64(const QString &s)
{
    static const QRegularExpression re(QStringLiteral("^[0-9a-f]{64}$"));
    return re.match(s).hasMatch();
}

QString postLink(const QString &text)
{
    const QString t = text.trimmed();
    const QUrl u(t, QUrl::StrictMode);
    if (t.size() > 300 || !u.isValid() || u.scheme() != u"https" || u.host() != u"t.me" || !u.userInfo().isEmpty() || u.port() != -1 || u.path().size() < 2)
        return {};
    for (const QChar c : t)
        if (c.unicode() <= 0x20 || c.unicode() >= 0x7f)
            return {};
    return t;
}

bool downloadHost(const QUrl &u)
{
    return u.isValid() && u.scheme() == u"https" && u.host() == kHost && u.userInfo().isEmpty() && u.port() == -1 && !u.hasQuery() && !u.hasFragment();
}

bool sameDir(const QString &a, const QString &b)
{
    if (a.isEmpty() || b.isEmpty())
        return false;
    const auto clean = [](QString p) { return QDir::cleanPath(p.replace(u'\\', u'/')); };
    return clean(a).compare(clean(b), Qt::CaseInsensitive) == 0;
}
}

QString releasesUrl() { return Endpoints::Api + QStringLiteral("/v1/releases"); }
QString downloadsUrl() { return Endpoints::Api + QStringLiteral("/v1/downloads"); }
QString pageUrl() { return Endpoints::DownloadPage; }

std::optional<QList<int>> parseVersion(const QString &text)
{
    static const QRegularExpression re(QStringLiteral("^[0-9]{1,6}(\\.[0-9]{1,6}){0,3}$"));
    const QString t = text.trimmed();
    if (!re.match(t).hasMatch())
        return {};
    QList<int> out;
    for (const QString &p : t.split('.'))
        out << p.toInt();
    return out;
}

bool newer(const QString &have, const QString &latest)
{
    const auto a = parseVersion(have), b = parseVersion(latest);
    if (!a || !b)
        return false;
    const qsizetype n = std::max(a->size(), b->size());
    for (qsizetype i = 0; i < n; ++i) {
        const int x = i < a->size() ? a->at(i) : 0, y = i < b->size() ? b->at(i) : 0;
        if (x != y)
            return y > x;
    }
    return false;
}

QString cleanText(const QString &text, qsizetype max)
{
    QString out;
    out.reserve(std::min(text.size(), max + 2));
    for (qsizetype i = 0; i < text.size(); ++i) {
        const QChar c = text.at(i);
        if (c == u'\r') {
            if (i + 1 < text.size() && text.at(i + 1) == u'\n')
                continue;
            out += u'\n';
        } else if (c == u'\n' || c == QChar::LineSeparator || c == QChar::ParagraphSeparator) {
            out += u'\n';
        } else if (c == u'\t') {
            out += u' ';
        } else if (c.isHighSurrogate() || c.isLowSurrogate()) {
            out += c;
        } else if (c.category() == QChar::Other_Control || c.category() == QChar::Other_Format || c.category() == QChar::Other_PrivateUse
                   || c.category() == QChar::Other_NotAssigned) {
            continue;
        } else {
            out += c;
        }
    }
    out = out.trimmed();
    static const QRegularExpression gaps(QStringLiteral("\\n{3,}"));
    out.replace(gaps, QStringLiteral("\n\n"));
    if (out.size() > max) {
        qsizetype cut = max;
        if (out.at(cut - 1).isHighSurrogate())
            --cut;
        out = out.left(cut).trimmed() + QStringLiteral("…");
    }
    return out;
}

std::optional<Release> parseRelease(const QByteArray &body)
{
    if (body.isEmpty() || body.size() > MAX_RELEASES)
        return {};
    QJsonParseError err;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &err);
    if (err.error != QJsonParseError::NoError || !doc.isObject())
        return {};
    const QJsonValue pc = doc.object().value("pc");
    if (!pc.isObject())
        return {};
    const QJsonObject o = pc.toObject();
    const QJsonValue v = o.value("version");
    if (!v.isString() || !parseVersion(v.toString()))
        return {};
    Release r;
    r.version = v.toString().trimmed();
    if (o.value("url").isString())
        r.url = postLink(o.value("url").toString());
    if (o.value("changelog").isString())
        r.changelog = cleanText(o.value("changelog").toString(), MAX_CHANGELOG);
    return r;
}

QString alias(Install install)
{
    switch (install) {
    case Install::Msi: return QStringLiteral("/windows/latest.msi");
    case Install::Setup: return QStringLiteral("/windows/latest.exe");
    case Install::Portable: return QStringLiteral("/windows/portable/latest.zip");
    case Install::Deb: return QStringLiteral("/linux/latest.deb");
    case Install::Rpm: return QStringLiteral("/linux/latest.rpm");
    case Install::Pacman: return QStringLiteral("/linux/latest.pkg.tar.zst");
    case Install::AppImage: return QStringLiteral("/linux/latest.AppImage");
    case Install::Tar: return QStringLiteral("/linux/latest.tar.gz");
    case Install::Unknown: break;
    }
    return {};
}

bool fileMatches(Install install, const QString &version, const QString &file)
{
    const QString v = QRegularExpression::escape(version);
    QString re;
    switch (install) {
    case Install::Msi: re = QStringLiteral("ryNet-%1\\.msi").arg(v); break;
    case Install::Setup: re = QStringLiteral("ryNet-%1-setup\\.exe").arg(v); break;
    case Install::Portable: re = QStringLiteral("ryNet-%1-portable\\.zip").arg(v); break;
    case Install::Deb: re = QStringLiteral("rynet_%1-[0-9]+_amd64\\.deb").arg(v); break;
    case Install::Rpm: re = QStringLiteral("rynet-%1-[0-9]+\\.x86_64\\.rpm").arg(v); break;
    case Install::Pacman: re = QStringLiteral("rynet-%1-[0-9]+-x86_64\\.pkg\\.tar\\.zst").arg(v); break;
    case Install::AppImage: re = QStringLiteral("ryNet-%1-x86_64\\.AppImage").arg(v); break;
    case Install::Tar: re = QStringLiteral("ryNet-linux-%1\\.tar\\.gz").arg(v); break;
    case Install::Unknown: return false;
    }
    return QRegularExpression(QStringLiteral("^%1$").arg(re)).match(file).hasMatch();
}

std::optional<Download> parseDownload(const QByteArray &body, Install install, const QString &version)
{
    const QString want = alias(install);
    if (want.isEmpty() || body.isEmpty() || body.size() > MAX_DOWNLOADS || !parseVersion(version))
        return {};
    const QJsonDocument doc = QJsonDocument::fromJson(body);
    if (!doc.isArray())
        return {};
    for (const QJsonValue &item : doc.array()) {
        if (!item.isObject())
            continue;
        const QJsonObject o = item.toObject();
        if (o.value("alias").toString() != want)
            continue;
        const QJsonValue file = o.value("file"), sha = o.value("sha256"), path = o.value("path"), url = o.value("url");
        if (!file.isString() || !sha.isString() || !fileMatches(install, version, file.toString()))
            return {};
        const QString hash = sha.toString().toLower();
        if (!isHex64(hash))
            return {};
        const QString direct = QStringLiteral("https://%1/releases/%2/%3").arg(kHost, hash, file.toString());
        const QString viaAlias = QStringLiteral("https://%1%2").arg(kHost, want);
        QString link;
        if (path.isString() && path.toString() == QStringLiteral("/releases/%1/%2").arg(hash, file.toString()) && downloadHost(QUrl(direct, QUrl::StrictMode)))
            link = direct;
        else if (url.isString() && url.toString() == viaAlias && downloadHost(QUrl(viaAlias, QUrl::StrictMode)))
            link = viaAlias;
        if (link.isEmpty())
            return {};
        return Download{file.toString(), hash, link};
    }
    return {};
}

Install windowsInstall(bool portable, const QString &exeDir, const QString &msiDir, const QString &innoDir)
{
    if (portable)
        return Install::Portable;
    if (sameDir(msiDir, exeDir))
        return Install::Msi;
    if (sameDir(innoDir, exeDir))
        return Install::Setup;
    return Install::Unknown;
}

Install linuxInstall(const QString &appImage, const QString &exe, const QString &home, const QString &owner)
{
    if (!appImage.isEmpty())
        return Install::AppImage;
    if (!home.isEmpty() && exe.startsWith(QDir::cleanPath(home) + QStringLiteral("/.local/opt/rynet/")))
        return Install::Tar;
    if (exe.startsWith(QStringLiteral("/opt/rynet/"))) {
        if (owner == u"deb")
            return Install::Deb;
        if (owner == u"rpm")
            return Install::Rpm;
        if (owner == u"pacman")
            return Install::Pacman;
    }
    return Install::Unknown;
}

Install detect()
{
    const QString exe = QCoreApplication::applicationFilePath();
#ifdef Q_OS_WIN
    const QString msi = QSettings(QStringLiteral("HKEY_CURRENT_USER\\Software\\yzewe\\ryNet"), QSettings::NativeFormat).value(QStringLiteral("InstallDir")).toString();
    const QString key = QStringLiteral("\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{6D3CF723-5E4D-4F5F-B44D-FB1F5669DC88}_is1");
    QString inno = QSettings(QStringLiteral("HKEY_CURRENT_USER") + key, QSettings::NativeFormat).value(QStringLiteral("Inno Setup: App Path")).toString();
    if (inno.isEmpty())
        inno = QSettings(QStringLiteral("HKEY_LOCAL_MACHINE") + key, QSettings::NativeFormat).value(QStringLiteral("Inno Setup: App Path")).toString();
    return windowsInstall(!Store::portableDir().isEmpty(), QFileInfo(exe).absolutePath(), msi, inno);
#else
    if (QSysInfo::currentCpuArchitecture() != u"x86_64")
        return Install::Unknown;
    QString owner;
    if (qEnvironmentVariableIsEmpty("APPIMAGE") && exe.startsWith(QStringLiteral("/opt/rynet/"))) {
        const std::tuple<const char *, QStringList, const char *> tools[] = {
            {"dpkg-query", {QStringLiteral("-S"), exe}, "deb"}, {"rpm", {QStringLiteral("-qf"), exe}, "rpm"}, {"pacman", {QStringLiteral("-Qo"), exe}, "pacman"}};
        for (const auto &[program, args, name] : tools)
            if (Platform::out(QString::fromLatin1(program), args, 5000)) {
                owner = QString::fromLatin1(name);
                break;
            }
    }
    return linuxInstall(qEnvironmentVariable("APPIMAGE"), exe, QDir::homePath(), owner);
#endif
}

QJsonObject Found::toJson() const
{
    return {{"version", version}, {"changelog", changelog}, {"post", post}, {"file", file}, {"link", link}, {"sha256", sha256}};
}

std::optional<Found> Found::fromJson(const QJsonObject &o)
{
    Found f;
    f.version = o.value("version").toString();
    if (!parseVersion(f.version))
        return {};
    f.changelog = cleanText(o.value("changelog").toString(), MAX_CHANGELOG);
    f.post = postLink(o.value("post").toString());
    f.link = o.value("link").toString();
    if (f.link == pageUrl())
        return f;
    const QString hash = o.value("sha256").toString(), file = o.value("file").toString();
    const QString direct = QStringLiteral("https://%1/releases/%2/%3").arg(kHost, hash, file);
    if (!isHex64(hash) || file.isEmpty() || !downloadHost(QUrl(f.link, QUrl::StrictMode)) || (f.link != direct && !f.link.startsWith(QStringLiteral("https://%1/").arg(kHost))))
        return {};
    f.file = file;
    f.sha256 = hash;
    return f;
}
}
