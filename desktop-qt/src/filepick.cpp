#include "filepick.h"

#include "model.h"
#include "platform.h"

#include <QDir>
#include <QGuiApplication>
#include <QTimer>
#include <QUrl>
#include <QUuid>
#include <QWindow>

#ifdef RYNET_DBUS
#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusObjectPath>
#endif

#ifdef Q_OS_WIN
#include <windows.h>
#include <shobjidl.h>
#endif

#ifdef RYNET_DBUS
struct FilterRule {
    uint type = 0;
    QString pattern;
};
struct Filter {
    QString name;
    QList<FilterRule> rules;
};
using Filters = QList<Filter>;
Q_DECLARE_METATYPE(FilterRule)
Q_DECLARE_METATYPE(Filter)
Q_DECLARE_METATYPE(Filters)

QDBusArgument &operator<<(QDBusArgument &a, const FilterRule &r)
{
    a.beginStructure();
    a << r.type << r.pattern;
    a.endStructure();
    return a;
}
const QDBusArgument &operator>>(const QDBusArgument &a, FilterRule &r)
{
    a.beginStructure();
    a >> r.type >> r.pattern;
    a.endStructure();
    return a;
}
QDBusArgument &operator<<(QDBusArgument &a, const Filter &f)
{
    a.beginStructure();
    a << f.name << f.rules;
    a.endStructure();
    return a;
}
const QDBusArgument &operator>>(const QDBusArgument &a, Filter &f)
{
    a.beginStructure();
    a >> f.name >> f.rules;
    a.endStructure();
    return a;
}

PortalRequest::PortalRequest(std::function<void(uint, const QVariantMap &)> done, QObject *parent)
    : QObject(parent)
    , m_done(std::move(done))
{
}

void PortalRequest::response(uint code, const QVariantMap &results)
{
    if (m_done)
        std::exchange(m_done, nullptr)(code, results);
    deleteLater();
}
#endif

namespace {
#ifndef Q_OS_WIN
QString fallback(bool save, const QString &name, const QString &title, const QString &ext)
{
    const QString home = QDir::homePath();
    auto has = [](const char *cmd) { return Platform::out(QStringLiteral("sh"), {QStringLiteral("-c"), QStringLiteral("command -v ") + QLatin1String(cmd)}).has_value(); };
    const bool kde = qEnvironmentVariable("XDG_CURRENT_DESKTOP").contains(QStringLiteral("KDE"), Qt::CaseInsensitive);
    const QString filter = QStringLiteral("ryNet (*.%1)").arg(ext);
    std::optional<QString> picked;
    if ((kde && has("kdialog")) || (!has("zenity") && has("kdialog"))) {
        if (save)
            picked = Platform::out(QStringLiteral("kdialog"), {QStringLiteral("--title"), title, QStringLiteral("--getsavefilename"), QDir(home).filePath(name),
                                                               QStringLiteral("*.%1|%2").arg(ext, filter)}, 3600000);
        else
            picked = Platform::out(QStringLiteral("kdialog"), {QStringLiteral("--title"), title, QStringLiteral("--getopenfilename"), home,
                                                               QStringLiteral("*.%1|%2\n*|%3").arg(ext, filter, t("Все файлы", "All files"))}, 3600000);
    } else if (has("zenity")) {
        if (save)
            picked = Platform::out(QStringLiteral("zenity"), {QStringLiteral("--file-selection"), QStringLiteral("--save"), QStringLiteral("--title=") + title,
                                                              QStringLiteral("--filename=") + QDir(home).filePath(name), QStringLiteral("--file-filter=%1 | *.%2").arg(filter, ext)}, 3600000);
        else
            picked = Platform::out(QStringLiteral("zenity"), {QStringLiteral("--file-selection"), QStringLiteral("--title=") + title, QStringLiteral("--filename=") + home + u'/',
                                                              QStringLiteral("--file-filter=%1 | *.%2").arg(filter, ext), QStringLiteral("--file-filter=%1 | *").arg(t("Все файлы", "All files"))}, 3600000);
    }
    if (!picked)
        return {};
    const QStringList lines = picked->split(u'\n');
    for (auto it = lines.crbegin(); it != lines.crend(); ++it)
        if (it->startsWith(u'/'))
            return it->trimmed();
    return {};
}

void viaFallback(QObject *ctx, bool save, const QString &name, const QString &title, const QString &ext, std::function<void(const QString &)> done)
{
    Platform::background<QString>(ctx, [=] { return fallback(save, name, title, ext); }, [done](QString path) { done(path); });
}
#endif
}

void FilePick::pick(QObject *ctx, bool save, const QString &name, const QString &title, const QString &ext, std::function<void(const QString &)> done)
{
#ifdef Q_OS_WIN
    QTimer::singleShot(0, ctx, [=] {
        QString result;
        IFileDialog *dlg = nullptr;
        if (SUCCEEDED(CoCreateInstance(save ? CLSID_FileSaveDialog : CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dlg)))) {
            const std::wstring spec = (QStringLiteral("*.") + ext).toStdWString();
            const std::wstring label = QStringLiteral("ryNet (*.%1)").arg(ext).toStdWString();
            const std::wstring all = t("Все файлы", "All files").toStdWString();
            COMDLG_FILTERSPEC filters[] = {{label.c_str(), spec.c_str()}, {all.c_str(), L"*.*"}};
            dlg->SetFileTypes(2, filters);
            dlg->SetTitle(title.toStdWString().c_str());
            if (save) {
                dlg->SetFileName(name.toStdWString().c_str());
                dlg->SetDefaultExtension(ext.toStdWString().c_str());
            }
            QWindow *w = QGuiApplication::focusWindow();
            if (SUCCEEDED(dlg->Show(w ? reinterpret_cast<HWND>(w->winId()) : nullptr))) {
                IShellItem *item = nullptr;
                if (SUCCEEDED(dlg->GetResult(&item))) {
                    PWSTR path = nullptr;
                    if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &path))) {
                        result = QString::fromWCharArray(path);
                        CoTaskMemFree(path);
                    }
                    item->Release();
                }
            }
            dlg->Release();
        }
        done(result);
    });
#else
#ifdef RYNET_DBUS
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (bus.isConnected()) {
        static const bool registered = [] {
            qDBusRegisterMetaType<FilterRule>();
            qDBusRegisterMetaType<QList<FilterRule>>();
            qDBusRegisterMetaType<Filter>();
            qDBusRegisterMetaType<Filters>();
            return true;
        }();
        Q_UNUSED(registered);
        const QString token = QStringLiteral("rynet") + QUuid::createUuid().toString(QUuid::Id128).left(12);
        QString sender = bus.baseService();
        sender.remove(0, 1).replace(u'.', u'_');
        const QString path = QStringLiteral("/org/freedesktop/portal/desktop/request/%1/%2").arg(sender, token);
        auto handled = std::make_shared<bool>(false);
        auto *req = new PortalRequest([=](uint code, const QVariantMap &results) {
            *handled = true;
            if (code > 1) {
                viaFallback(ctx, save, name, title, ext, done);
                return;
            }
            const QStringList uris = results.value(QStringLiteral("uris")).toStringList();
            done(code == 0 && !uris.isEmpty() ? QUrl(uris.first()).toLocalFile() : QString());
        }, ctx);
        bus.connect(QStringLiteral("org.freedesktop.portal.Desktop"), path, QStringLiteral("org.freedesktop.portal.Request"), QStringLiteral("Response"), req,
                    SLOT(response(uint, QVariantMap)));
        QVariantMap options{{QStringLiteral("handle_token"), token}};
        const Filters filters{{QStringLiteral("ryNet (*.%1)").arg(ext), {{0, QStringLiteral("*.") + ext}}}, {t("Все файлы", "All files"), {{0, QStringLiteral("*")}}}};
        options[QStringLiteral("filters")] = QVariant::fromValue(filters);
        if (save)
            options[QStringLiteral("current_name")] = name;
        QDBusMessage m = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.portal.Desktop"), QStringLiteral("/org/freedesktop/portal/desktop"),
                                                        QStringLiteral("org.freedesktop.portal.FileChooser"), save ? QStringLiteral("SaveFile") : QStringLiteral("OpenFile"));
        m << QString() << title << options;
        auto *watch = new QDBusPendingCallWatcher(bus.asyncCall(m, 10000), ctx);
        QObject::connect(watch, &QDBusPendingCallWatcher::finished, ctx, [=](QDBusPendingCallWatcher *w) {
            w->deleteLater();
            if (w->isError() && !*handled) {
                req->deleteLater();
                viaFallback(ctx, save, name, title, ext, done);
            }
        });
        return;
    }
#endif
    viaFallback(ctx, save, name, title, ext, std::move(done));
#endif
}
