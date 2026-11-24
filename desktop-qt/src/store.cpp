#include "store.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>

namespace {
QJsonObject readIndex()
{
    QFile f(Store::path(QStringLiteral("desktop.json")));
    if (!f.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(f.readAll()).object();
}

QString bodyFile(const QString &id)
{
    static const QRegularExpression re(QStringLiteral("^[A-Za-z0-9_-]{1,128}$"));
    if (!re.match(id).hasMatch())
        throw Fail(t("Неверный идентификатор подписки", "Invalid subscription ID"));
    return Store::path(QStringLiteral("subs/%1.txt").arg(id));
}
}

namespace {
void ownerOnly(const QString &dir)
{
#ifndef Q_OS_WIN
    QFile::setPermissions(dir, QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner);
#else
    Q_UNUSED(dir);
#endif
}
}

QString Store::userDir()
{
    static const QString dir = [] {
#ifdef Q_OS_WIN
        QString base = qEnvironmentVariable("APPDATA");
        if (base.isEmpty())
            base = QDir::homePath() + QStringLiteral("/AppData/Roaming");
#else
        QString base = qEnvironmentVariable("XDG_CONFIG_HOME");
        if (base.isEmpty())
            base = QDir::homePath() + QStringLiteral("/.config");
#endif
        const QString d = QDir(base).filePath(QStringLiteral("rynet"));
        QDir().mkpath(d);
        ownerOnly(d);
        return d;
    }();
    return dir;
}

namespace {
QString exeDir;
}

void Store::setExeDir(const QString &dir) { exeDir = dir; }

QString Store::portableDir()
{
    static const QString dir = [] {
        const QString d = QDir(exeDir.isEmpty() ? QCoreApplication::applicationDirPath() : exeDir).filePath(QStringLiteral("data"));
        const QFileInfo fi(d);
        return fi.isDir() && fi.isWritable() ? fi.absoluteFilePath() : QString();
    }();
    return dir;
}

QString Store::dir()
{
    static const QString d = [] {
        const QString p = portableDir();
        const QString base = p.isEmpty() ? userDir() : p;
        QDir().mkpath(QDir(base).filePath(QStringLiteral("subs")));
        ownerOnly(base);
        return base;
    }();
    return d;
}

QString Store::path(const QString &name) { return QDir(dir()).filePath(name); }

bool Store::write(const QString &file, const QByteArray &data)
{
    QSaveFile f(file);
    if (!f.open(QIODevice::WriteOnly))
        return false;
#ifndef Q_OS_WIN
    f.setPermissions(QFile::ReadOwner | QFile::WriteOwner);
#endif
    f.write(data);
    return f.commit();
}

QList<Sub> Store::subs()
{
    QList<Sub> out;
    for (const QJsonValue &v : readIndex().value("subs").toArray()) {
        try {
            Sub s = Sub::from(v.toObject());
            if (Local::isLocal(s))
                s.title = Local::title();
            out << s;
        } catch (const Fail &) {
        }
    }
    return out;
}

Settings Store::settings()
{
    const QJsonObject o = readIndex();
    return o.value("settings").isObject() ? Settings::from(o.value("settings").toObject()) : Settings();
}

void Store::save(const QList<Sub> &subs, const Settings &settings)
{
    QJsonArray a;
    for (const Sub &s : subs)
        a.append(s.toJson());
    write(path(QStringLiteral("desktop.json")), QJsonDocument(QJsonObject{{"subs", a}, {"settings", settings.toJson()}}).toJson(QJsonDocument::Indented));
}

std::optional<QString> Store::body(const QString &id)
{
    QFile f(bodyFile(id));
    if (!f.open(QIODevice::ReadOnly))
        return {};
    return QString::fromUtf8(f.readAll());
}

void Store::saveBody(const QString &id, const QString &text)
{
    if (!write(bodyFile(id), text.toUtf8()))
        throw Fail(t("Не удалось сохранить подписку", "Couldn't save the subscription"));
}

void Store::removeBody(const QString &id) { QFile::remove(bodyFile(id)); }

std::optional<Store::Window> Store::window()
{
    QFile f(path(QStringLiteral("window.json")));
    if (!f.open(QIODevice::ReadOnly))
        return {};
    const QJsonObject o = QJsonDocument::fromJson(f.readAll()).object();
    Window w{o.value("w").toInt(), o.value("h").toInt(), o.value("max").toBool()};
    if (w.w < 300 || w.w > 10000 || w.h < 300 || w.h > 10000)
        return {};
    return w;
}

void Store::saveWindow(const Window &w)
{
    write(path(QStringLiteral("window.json")), QJsonDocument(QJsonObject{{"w", w.w}, {"h", w.h}, {"max", w.max}}).toJson(QJsonDocument::Compact));
}
