#include "platform.h"

#include "core.h"
#include "model.h"
#include "store.h"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDesktopServices>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMutex>
#include <QProcess>
#include <QSettings>
#include <QTemporaryFile>
#include <QUrl>

#ifdef RYNET_DBUS
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCall>
#endif

#ifdef Q_OS_WIN
#include <windows.h>
#include <dwmapi.h>
#include <tlhelp32.h>
#include <wininet.h>
#endif

namespace {
#ifdef Q_OS_WIN
QString fromConsole(const QByteArray &b)
{
    if (b.isEmpty())
        return {};
    const UINT cp = GetOEMCP();
    const int n = MultiByteToWideChar(cp, 0, b.constData(), int(b.size()), nullptr, 0);
    std::wstring w(size_t(n), L'\0');
    MultiByteToWideChar(cp, 0, b.constData(), int(b.size()), w.data(), n);
    return QString::fromStdWString(w);
}
#else
QString fromConsole(const QByteArray &b) { return QString::fromLocal8Bit(b); }
#endif

QString programFilesDir()
{
    QString pf = qEnvironmentVariable("ProgramFiles");
    if (pf.isEmpty())
        pf = QStringLiteral("C:\\Program Files");
    return pf + QStringLiteral("\\ryNet");
}

#ifdef Q_OS_WIN
QString selfPath()
{
    wchar_t buf[1024];
    DWORD len = DWORD(sizeof(buf) / sizeof(buf[0]));
    if (!QueryFullProcessImageNameW(GetCurrentProcess(), 0, buf, &len))
        return QDir::toNativeSeparators(QCoreApplication::applicationFilePath());
    return QString::fromWCharArray(buf, int(len));
}
#endif
}

bool Platform::windows()
{
#ifdef Q_OS_WIN
    return true;
#else
    return false;
#endif
}

QString Platform::version() { return QStringLiteral(RYNET_VERSION); }

std::optional<QString> Platform::out(const QString &program, const QStringList &args, int timeoutMs)
{
    QProcess p;
    p.setStandardErrorFile(QProcess::nullDevice());
    p.start(program, args);
    if (!p.waitForStarted(5000))
        return {};
    if (!p.waitForFinished(timeoutMs)) {
        p.kill();
        p.waitForFinished(1000);
        return {};
    }
    if (p.exitStatus() != QProcess::NormalExit || p.exitCode() != 0)
        return {};
    return fromConsole(p.readAllStandardOutput()).trimmed();
}

void Platform::open(const QString &target)
{
    const QUrl url(target);
    if (!url.scheme().isEmpty() && url.scheme().size() > 1)
        QDesktopServices::openUrl(url);
    else
        QDesktopServices::openUrl(QUrl::fromLocalFile(target));
}

QString Platform::launcher()
{
    const QString appimage = qEnvironmentVariable("APPIMAGE");
    return appimage.isEmpty() ? QCoreApplication::applicationFilePath() : appimage;
}

void Platform::autostart(bool on)
{
    const QString cmd = launcher();
#ifdef Q_OS_WIN
    QSettings run(QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"), QSettings::NativeFormat);
    if (on)
        run.setValue(QStringLiteral("ryNet"), QStringLiteral("\"%1\" --autostart").arg(QDir::toNativeSeparators(cmd)));
    else
        run.remove(QStringLiteral("ryNet"));
    run.sync();
    if (run.status() != QSettings::NoError)
        throw Fail(t("Не удалось изменить автозапуск", "Couldn't change autostart"));
#elif defined(Q_OS_MACOS)
    const QString dir = QDir::homePath() + QStringLiteral("/Library/LaunchAgents");
    const QString file = dir + QStringLiteral("/dev.yzewe.rynet.plist");
    if (!on) {
        QFile::remove(file);
        return;
    }
    QDir().mkpath(dir);
    QString exe = cmd.toHtmlEscaped();
    const QByteArray plist = QStringLiteral(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
        "<plist version=\"1.0\"><dict>\n"
        "<key>Label</key><string>dev.yzewe.rynet</string>\n"
        "<key>ProgramArguments</key><array><string>%1</string><string>--autostart</string></array>\n"
        "<key>RunAtLoad</key><true/>\n"
        "</dict></plist>\n").arg(exe).toUtf8();
    if (!Store::write(file, plist))
        throw Fail(t("Не удалось изменить автозапуск", "Couldn't change autostart"));
#else
    QString base = qEnvironmentVariable("XDG_CONFIG_HOME");
    if (base.isEmpty())
        base = QDir::homePath() + QStringLiteral("/.config");
    const QString dir = QDir(base).filePath(QStringLiteral("autostart"));
    const QString file = QDir(dir).filePath(QStringLiteral("rynet.desktop"));
    if (!on) {
        QFile::remove(file);
        return;
    }
    QDir().mkpath(dir);
    const QByteArray entry = QStringLiteral("[Desktop Entry]\nType=Application\nName=ryNet\nExec=\"%1\" --autostart\nIcon=rynet\nX-GNOME-Autostart-enabled=true\n").arg(cmd).toUtf8();
    if (!Store::write(file, entry))
        throw Fail(t("Не удалось изменить автозапуск", "Couldn't change autostart"));
#endif
}

QStringList Platform::processes()
{
    QStringList out;
#ifdef Q_OS_WIN
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snap != INVALID_HANDLE_VALUE) {
        PROCESSENTRY32W e{};
        e.dwSize = sizeof(e);
        for (BOOL ok = Process32FirstW(snap, &e); ok; ok = Process32NextW(snap, &e))
            out << QString::fromWCharArray(e.szExeFile);
        CloseHandle(snap);
    }
#elif defined(Q_OS_MACOS)
    if (const auto ps = Platform::out(QStringLiteral("ps"), {QStringLiteral("-axco"), QStringLiteral("comm=")}))
        for (const QString &l : ps->split(u'\n', Qt::SkipEmptyParts))
            out << l.trimmed();
#else
    const QDir proc(QStringLiteral("/proc"));
    for (const QString &pid : proc.entryList(QDir::Dirs | QDir::NoDotAndDotDot)) {
        if (pid.isEmpty() || !pid.front().isDigit())
            continue;
        QFile f(proc.filePath(pid + QStringLiteral("/comm")));
        if (!f.open(QIODevice::ReadOnly))
            continue;
        const QString name = QString::fromUtf8(f.readAll()).trimmed();
        if (!name.isEmpty() && !name.startsWith(QStringLiteral("kworker")))
            out << name;
    }
#endif
    out.removeDuplicates();
    out.sort(Qt::CaseInsensitive);
    return out;
}

void Platform::notify(const QString &title, const QString &text)
{
#ifdef Q_OS_WIN
    QString t2 = text;
    t2.replace(u'\'', QStringLiteral("''"));
    QString t1 = title;
    t1.replace(u'\'', QStringLiteral("''"));
    QProcess::startDetached(QStringLiteral("powershell.exe"),
                            {QStringLiteral("-NoProfile"), QStringLiteral("-WindowStyle"), QStringLiteral("Hidden"), QStringLiteral("-Command"),
                             QStringLiteral("Add-Type -AssemblyName System.Windows.Forms; $n = New-Object System.Windows.Forms.NotifyIcon; "
                                            "$n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; "
                                            "$n.ShowBalloonTip(5000, '%1', '%2', 'Info'); Start-Sleep 6; $n.Dispose()")
                                 .arg(t1, t2)});
#elif defined(Q_OS_MACOS)
    auto quoted = [](QString v) { return u'"' + v.replace(u'\\', QStringLiteral("\\\\")).replace(u'"', QStringLiteral("\\\"")) + u'"'; };
    QProcess::startDetached(QStringLiteral("osascript"),
                            {QStringLiteral("-e"), QStringLiteral("display notification %1 with title %2").arg(quoted(text), quoted(title))});
#else
#ifdef RYNET_DBUS
    QDBusMessage m = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.Notifications"), QStringLiteral("/org/freedesktop/Notifications"),
                                                    QStringLiteral("org.freedesktop.Notifications"), QStringLiteral("Notify"));
    m << QStringLiteral("ryNet") << uint(0) << QStringLiteral("network-vpn") << title << text << QStringList() << QVariantMap() << int(-1);
    if (QDBusConnection::sessionBus().isConnected()) {
        QDBusConnection::sessionBus().asyncCall(m);
        return;
    }
#endif
    QProcess::startDetached(QStringLiteral("notify-send"), {QStringLiteral("-a"), QStringLiteral("ryNet"), QStringLiteral("-i"), QStringLiteral("network-vpn"), title, text});
#endif
}

void Platform::notifyOnce(const QString &key, const QString &title, const QString &text)
{
    const QString file = Store::path(QStringLiteral("notified.json"));
    QFile f(file);
    QJsonObject o;
    if (f.open(QIODevice::ReadOnly))
        o = QJsonDocument::fromJson(f.readAll()).object();
    f.close();
    const qint64 now = nowMillis();
    if (now - qint64(o.value(key).toDouble()) < 86'400'000LL)
        return;
    o[key] = now;
    Store::write(file, QJsonDocument(o).toJson(QJsonDocument::Compact));
    notify(title, text);
}

bool Platform::firewallReady()
{
    if (windows() || out(QStringLiteral("firewall-cmd"), {QStringLiteral("--state")}) != QStringLiteral("running"))
        return true;
    QFile zone(QStringLiteral("/etc/firewalld/zones/trusted.xml"));
    if (zone.open(QIODevice::ReadOnly) && zone.readAll().contains("name=\"rynet0\""))
        return true;
    return out(QStringLiteral("firewall-cmd"), {QStringLiteral("--get-zone-of-interface=rynet0")}) == QStringLiteral("trusted");
}

static QString terminalFor()
{
    static const QList<QStringList> terms = {
        {"foot"}, {"kitty"}, {"alacritty", "-e"}, {"wezterm", "start", "--"}, {"ghostty", "-e"}, {"konsole", "-e"}, {"gnome-terminal", "--wait", "--"}, {"xterm", "-e"},
    };
    for (const QStringList &t : terms)
        if (Platform::out(QStringLiteral("sh"), {QStringLiteral("-c"), QStringLiteral("command -v ") + t[0]}))
            return t.join(QChar(0x1f));
    return {};
}

#ifdef Q_OS_WIN
static QString grantWindows()
{
    const QString core = Core::bundled();
    const QString dll = Core::resource(QStringLiteral("wintun.dll"), false);
    if (dll.isEmpty())
        return t("Нет wintun.dll в пакете", "wintun.dll is missing from the package");
    const auto coreSum = Core::digest(core), dllSum = Core::digest(dll);
    if (!coreSum || !dllSum)
        return t("Не найдено ядро приложения", "The app's core wasn't found");
    const auto who = Platform::out(QStringLiteral("whoami"), {QStringLiteral("/user"), QStringLiteral("/fo"), QStringLiteral("csv"), QStringLiteral("/nh")});
    QString sid;
    if (who) {
        const QStringList parts = who->split(u',');
        if (parts.size() > 1)
            sid = parts[1].trimmed().remove(u'"');
    }
    if (!sid.startsWith(QStringLiteral("S-1-")))
        return t("Не удалось узнать пользователя", "Couldn't identify the user");
    auto q = [](QString s) { return u'\'' + s.replace(u'\'', QStringLiteral("''")) + u'\''; };
    auto hex = [](const QByteArray &sum) { return QString::fromLatin1(sum.toHex().toUpper()); };
    const QString body = QStringLiteral(
                             "$ErrorActionPreference = 'Stop'; "
                             "$dir = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'ryNet'; "
                             "try { "
                             "New-Item -ItemType Directory -Force -Path $dir | Out-Null; "
                             "Remove-Item -Force -LiteralPath (Join-Path $dir 'grant.err') -ErrorAction SilentlyContinue; "
                             "$core = [IO.Path]::GetFullPath((Join-Path $dir 'rynet-core.exe')); "
                             "if ([IO.Path]::GetFullPath(%1) -ne $core) { "
                             "$dll = Join-Path $dir 'wintun.dll'; $new = $core + '.new'; $dllNew = $dll + '.new'; "
                             "Copy-Item -Force -LiteralPath %1 -Destination $new; "
                             "Copy-Item -Force -LiteralPath %2 -Destination $dllNew; "
                             "if ((Get-FileHash -Algorithm SHA256 -LiteralPath $new).Hash -ne %3 -or (Get-FileHash -Algorithm SHA256 -LiteralPath $dllNew).Hash -ne %4) { "
                             "Remove-Item -Force -LiteralPath $new, $dllNew -ErrorAction SilentlyContinue; "
                             "throw 'the core files changed while the permission was being granted' }; "
                             "Get-Process rynet-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ($dir + '*') } | Stop-Process -Force; "
                             "Start-Sleep -Milliseconds 300; "
                             "Move-Item -Force -LiteralPath $new -Destination $core; "
                             "Move-Item -Force -LiteralPath $dllNew -Destination $dll }; "
                             "Set-Content -Encoding UTF8 -LiteralPath (Join-Path $dir 'client.txt') -Value %5; "
                             "$a = New-ScheduledTaskAction -Execute $core -Argument '--listen'; "
                             "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel -Priority 4; "
                             "try { $p = New-ScheduledTaskPrincipal -UserId %6 -LogonType Interactive -RunLevel Highest; "
                             "Register-ScheduledTask -TaskName 'Core' -TaskPath '\\ryNet\\' -Action $a -Principal $p -Settings $s -Force | Out-Null "
                             "} catch { $p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Highest; "
                             "Register-ScheduledTask -TaskName 'Core' -TaskPath '\\ryNet\\' -Action $a -Principal $p -Settings $s -Force | Out-Null } "
                             "} catch { try { $_.Exception.Message | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $dir 'grant.err') } catch { }; exit 2 }")
                             .arg(q(QDir::toNativeSeparators(core)), q(QDir::toNativeSeparators(dll)), q(hex(*coreSum)), q(hex(*dllSum)), q(selfPath()), q(sid));
    QProcess p;
    p.start(QStringLiteral("powershell.exe"),
            {QStringLiteral("-NoProfile"), QStringLiteral("-ExecutionPolicy"), QStringLiteral("Bypass"), QStringLiteral("-WindowStyle"), QStringLiteral("Hidden"),
             QStringLiteral("-Command"),
             QStringLiteral("try { $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList "
                            "'-NoProfile','-ExecutionPolicy','Bypass','-Command',%1; exit $p.ExitCode } catch { exit 1223 }")
                 .arg(q(u'"' + body + u'"'))});
    p.waitForFinished(300000);
    const int code = p.exitCode();
    QString why;
    if (code == 2) {
        QFile err(programFilesDir() + QStringLiteral("\\grant.err"));
        if (err.open(QIODevice::ReadOnly))
            why = QString::fromUtf8(err.readAll()).remove(QChar(0xfeff)).trimmed();
    }
    if (code == 1223)
        return t("Разрешение отменено", "Permission canceled");
    if (code != 0 || !Platform::checkWinTun())
        return why.isEmpty() ? t("Не удалось установить ядро (код %1)", "Couldn't install the core (code %1)").arg(code)
                             : t("Не удалось установить ядро: %1", "Couldn't install the core: %1").arg(why);
    return {};
}
#endif

#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
QString Platform::polkitScript()
{
    struct { const char *file, *dir; } rules[] = {{"49-rynet-resolved.rules", "/etc/polkit-1/rules.d"}, {"49-rynet-resolved.pkla", "/etc/polkit-1/localauthority/50-local.d"}};
    QStringList parts;
    for (const auto &r : rules) {
        QFile f(QStringLiteral(":/polkit/") + QLatin1String(r.file));
        if (!f.open(QIODevice::ReadOnly))
            continue;
        const QString path = QLatin1String(r.dir) + u'/' + QLatin1String(r.file);
        parts << QStringLiteral("if [ -d %1 ]; then echo %2 | base64 -d > %3 && chmod 644 %3; fi").arg(QLatin1String(r.dir), QString::fromLatin1(f.readAll().toBase64()), path);
    }
    return parts.isEmpty() ? QString() : QStringLiteral("{ %1; true; }").arg(parts.join(QStringLiteral("; ")));
}
#endif

QString Platform::grantTun()
{
#ifdef Q_OS_WIN
    return grantWindows();
#elif defined(Q_OS_MACOS)
    return t("Режим «Весь трафик» на macOS пока не поддерживается: работает режим прокси",
             "\"All traffic\" mode isn't supported on macOS yet: proxy mode works");
#else
    if (!qEnvironmentVariableIsEmpty("RYNET_CORE"))
        return t("Задана переменная RYNET_CORE: права выдаются только ядру из комплекта приложения. Уберите её и повторите",
                 "RYNET_CORE is set: the permission is granted only to the core shipped with the app. Unset it and try again");
    const QString core = QFileInfo(Core::bundled()).canonicalFilePath();
    const auto sum = Core::digest(core);
    if (core.isEmpty() || !sum)
        return t("Не найдено ядро приложения", "The app's core wasn't found");
    auto q = [](QString s) { return u'\'' + s.replace(u'\'', QStringLiteral("'\\''")) + u'\''; };
    const QString dst = Core::grantedCore(), dir = QFileInfo(dst).path(), tmp = dst + QStringLiteral(".new");
    QStringList steps{
        QStringLiteral("{ mkdir -p %1 && chown 0:0 %1 && chmod 755 %1 && rm -f %2 && cp -- %3 %2 && test \"$(sha256sum %2 | cut -c1-64)\" = %4 "
                       "&& chown 0:0 %2 && chmod 755 %2 && setcap cap_net_admin,cap_net_raw+ep %2 && mv -f %2 %5; } "
                       "|| { rm -f %2; echo 'ryNet: the core was not installed'; exit 1; }")
            .arg(q(dir), q(tmp), q(core), q(QString::fromLatin1(sum->toHex())), q(dst)),
        polkitScript(),
        QStringLiteral("if command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then firewall-cmd --permanent --zone=trusted --add-interface=rynet0 >/dev/null && firewall-cmd --reload >/dev/null; fi"),
        QStringLiteral("if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q 'Status: active'; then ufw allow in on rynet0 >/dev/null; fi"),
    };
    steps.removeAll(QString());
    const QString script = steps.join(QStringLiteral(" && "));
    const QString notGranted = t("Разрешение не выдано", "Permission not granted");
    QProcess p;
    p.setProcessChannelMode(QProcess::MergedChannels);
    p.start(QStringLiteral("pkexec"), {QStringLiteral("sh"), QStringLiteral("-c"), script});
    const bool started = p.waitForStarted(5000);
    if (started)
        p.waitForFinished(300000);
    const QString output = QString::fromLocal8Bit(p.readAll());
    const int code = started ? p.exitCode() : 127;
    if (started && code == 0)
        return Core::granted() ? QString() : notGranted;
    if (code == 127 || output.toLower().contains(QStringLiteral("authentication agent"))) {
        const QString term = terminalFor();
        if (term.isEmpty())
            return t("Нет агента polkit и терминала — выполните в терминале: sudo sh -c %1", "No polkit agent or terminal — run in a terminal: sudo sh -c %1").arg(q(script));
        QStringList args = term.split(QChar(0x1f));
        const QString program = args.takeFirst();
        args << QStringLiteral("sh") << QStringLiteral("-c")
             << QStringLiteral("echo %1; sudo sh -c %2 && echo %3 || echo %4; sleep 2")
                    .arg(q(t("ryNet: разрешение режима «Весь трафик»", "ryNet: allowing \"All traffic\" mode")), q(script), q(t("Готово", "Done")), q(t("Не удалось", "Failed")));
        QProcess term2;
        term2.start(program, args);
        term2.waitForFinished(600000);
        return firewallReady() && Core::granted() ? QString() : notGranted;
    }
    const QString why = output.trimmed();
    return why.isEmpty() ? t("Разрешение отменено", "Permission canceled") : why;
#endif
}

bool Platform::checkWinTun()
{
#ifdef Q_OS_WIN
    QProcess q;
    q.setProcessChannelMode(QProcess::MergedChannels);
    q.start(QStringLiteral("schtasks"), {QStringLiteral("/query"), QStringLiteral("/tn"), QStringLiteral("\\ryNet\\Core")});
    if (!q.waitForFinished(10000) || q.exitCode() != 0)
        return false;
    const QString dll = Core::resource(QStringLiteral("wintun.dll"), false);
    if (dll.isEmpty())
        return false;
    const QString dir = programFilesDir();
    QFile client(dir + QStringLiteral("\\client.txt"));
    if (!client.open(QIODevice::ReadOnly) || QString::fromUtf8(client.readAll()).remove(QChar(0xfeff)).trimmed().compare(selfPath(), Qt::CaseInsensitive) != 0)
        return false;
    const auto a = Core::digest(Core::bundled()), b = Core::digest(dir + QStringLiteral("\\rynet-core.exe"));
    const auto c = Core::digest(dll), d = Core::digest(dir + QStringLiteral("\\wintun.dll"));
    return a && b && c && d && *a == *b && *c == *d;
#else
    return false;
#endif
}

#ifdef Q_OS_WIN
void Platform::titlebar(quintptr hwnd, bool dark, unsigned rgb)
{
    if (!hwnd)
        return;
    HWND h = reinterpret_cast<HWND>(hwnd);
    const BOOL on = dark;
    DwmSetWindowAttribute(h, 20, &on, sizeof(on));
    const COLORREF bgr = RGB((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    DwmSetWindowAttribute(h, 35, &bgr, sizeof(bgr));
    const COLORREF text = dark ? RGB(255, 255, 255) : RGB(0, 0, 0);
    DwmSetWindowAttribute(h, 36, &text, sizeof(text));
}
#endif

namespace {
QMutex proxyLock;
#ifdef Q_OS_WIN
std::optional<QVariantMap> winPrevious;
const QString kInternet = QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");

const QString kProxyBackup = QStringLiteral("HKEY_CURRENT_USER\\Software\\ryNet\\ProxyBackup");

void notifyInternet()
{
    InternetSetOptionW(nullptr, INTERNET_OPTION_SETTINGS_CHANGED, nullptr, 0);
    InternetSetOptionW(nullptr, INTERNET_OPTION_REFRESH, nullptr, 0);
}

void restoreWin(QSettings &reg, const QVariantMap &prev)
{
    for (const char *n : {"ProxyEnable", "ProxyServer", "ProxyOverride"}) {
        const QString key = QLatin1String(n);
        if (prev.contains(key))
            reg.setValue(key, prev.value(key));
        else
            reg.remove(key);
    }
    reg.sync();
    notifyInternet();
}
#elif defined(Q_OS_MACOS)
struct MacProxy {
    QString service, kind, enabled, server, port;
};
std::optional<QList<MacProxy>> macPrevious;
const QStringList kMacKinds = {QStringLiteral("webproxy"), QStringLiteral("securewebproxy"), QStringLiteral("socksfirewallproxy")};

QStringList macServices()
{
    QStringList out;
    const auto list = Platform::out(QStringLiteral("networksetup"), {QStringLiteral("-listallnetworkservices")});
    if (!list)
        return out;
    const QStringList lines = list->split(u'\n', Qt::SkipEmptyParts);
    for (int i = 1; i < lines.size(); ++i)
        if (!lines[i].startsWith(u'*'))
            out << lines[i].trimmed();
    return out;
}

bool ns(const QStringList &args) { return Platform::out(QStringLiteral("networksetup"), args).has_value(); }
#else
std::optional<QMap<QString, QString>> gnomePrevious;
const QStringList kProxyKeys = [] {
    QStringList k{QStringLiteral("org.gnome.system.proxy/mode"), QStringLiteral("org.gnome.system.proxy/ignore-hosts")};
    for (const char *p : {"http", "https", "socks"}) {
        k << QStringLiteral("org.gnome.system.proxy.%1/host").arg(QLatin1String(p));
        k << QStringLiteral("org.gnome.system.proxy.%1/port").arg(QLatin1String(p));
    }
    return k;
}();

bool gs(const QString &schemaKey, const QString &value)
{
    const QStringList p = schemaKey.split(u'/');
    return Platform::out(QStringLiteral("gsettings"), {QStringLiteral("set"), p[0], p[1], value}).has_value();
}
#endif
}

bool SysProxy::available()
{
    static const bool ok = [] {
#ifdef Q_OS_WIN
        return true;
#elif defined(Q_OS_MACOS)
        return QFile::exists(QStringLiteral("/usr/sbin/networksetup"));
#else
        return Platform::out(QStringLiteral("gsettings"), {QStringLiteral("get"), QStringLiteral("org.gnome.system.proxy"), QStringLiteral("mode")}).has_value();
#endif
    }();
    return ok;
}

bool SysProxy::enable(int port)
{
    if (port < 1 || port > 65535)
        return false;
    QMutexLocker lock(&proxyLock);
#ifdef Q_OS_WIN
    QSettings reg(kInternet, QSettings::NativeFormat);
    QSettings backup(kProxyBackup, QSettings::NativeFormat);
    const QString addr = QStringLiteral("127.0.0.1:%1").arg(port);
    const QString ours = QStringLiteral("http=%1;https=%1;socks=%1").arg(addr);
    if (!winPrevious) {
        QVariantMap prev;
        if (backup.contains(QStringLiteral("ours"))) {
            for (const char *n : {"ProxyEnable", "ProxyServer", "ProxyOverride"})
                if (backup.contains(QLatin1String(n)))
                    prev[QLatin1String(n)] = backup.value(QLatin1String(n));
        } else {
            for (const char *n : {"ProxyEnable", "ProxyServer", "ProxyOverride"})
                if (reg.contains(QLatin1String(n)))
                    prev[QLatin1String(n)] = reg.value(QLatin1String(n));
        }
        winPrevious = prev;
    }
    backup.clear();
    for (auto it = winPrevious->cbegin(); it != winPrevious->cend(); ++it)
        backup.setValue(it.key(), it.value());
    backup.setValue(QStringLiteral("ours"), ours);
    backup.sync();
    reg.setValue(QStringLiteral("ProxyServer"), ours);
    reg.setValue(QStringLiteral("ProxyOverride"), QStringLiteral("<local>"));
    reg.setValue(QStringLiteral("ProxyEnable"), 1);
    reg.sync();
    notifyInternet();
    return reg.status() == QSettings::NoError;
#elif defined(Q_OS_MACOS)
    const QStringList services = macServices();
    if (services.isEmpty())
        return false;
    if (!macPrevious) {
        QList<MacProxy> prev;
        for (const QString &svc : services)
            for (const QString &kind : kMacKinds) {
                const auto v = Platform::out(QStringLiteral("networksetup"), {QStringLiteral("-get") + kind, svc});
                if (!v)
                    return false;
                MacProxy p{svc, kind, {}, {}, {}};
                for (const QString &l : v->split(u'\n')) {
                    const QString key = l.section(u':', 0, 0).trimmed(), value = l.section(u':', 1).trimmed();
                    if (key == u"Enabled")
                        p.enabled = value;
                    else if (key == u"Server")
                        p.server = value;
                    else if (key == u"Port")
                        p.port = value;
                }
                prev << p;
            }
        macPrevious = prev;
    }
    bool ok = true;
    for (const QString &svc : services) {
        for (const QString &kind : kMacKinds)
            ok = ok && ns({QStringLiteral("-set") + kind, svc, QStringLiteral("127.0.0.1"), QString::number(port)});
        ok = ok && ns({QStringLiteral("-setproxybypassdomains"), svc, QStringLiteral("localhost"), QStringLiteral("127.0.0.0/8"), QStringLiteral("::1"),
                       QStringLiteral("10.0.0.0/8"), QStringLiteral("172.16.0.0/12"), QStringLiteral("192.168.0.0/16"), QStringLiteral("*.local")});
    }
    lock.unlock();
    if (!ok)
        disable();
    return ok;
#else
    if (!gnomePrevious) {
        QMap<QString, QString> snap;
        for (const QString &k : kProxyKeys) {
            const QStringList p = k.split(u'/');
            const auto v = Platform::out(QStringLiteral("gsettings"), {QStringLiteral("get"), p[0], p[1]});
            if (!v)
                return false;
            snap[k] = *v;
        }
        gnomePrevious = snap;
    }
    bool ok = true;
    for (const char *p : {"http", "https", "socks"}) {
        ok = ok && gs(QStringLiteral("org.gnome.system.proxy.%1/host").arg(QLatin1String(p)), QStringLiteral("127.0.0.1"));
        ok = ok && gs(QStringLiteral("org.gnome.system.proxy.%1/port").arg(QLatin1String(p)), QString::number(port));
    }
    ok = ok && gs(QStringLiteral("org.gnome.system.proxy/ignore-hosts"), QStringLiteral("['localhost', '127.0.0.0/8', '::1', '10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16']"));
    ok = ok && gs(QStringLiteral("org.gnome.system.proxy/mode"), QStringLiteral("manual"));
    lock.unlock();
    if (!ok)
        disable();
    return ok;
#endif
}

void SysProxy::disable()
{
    QMutexLocker lock(&proxyLock);
#ifdef Q_OS_WIN
    if (!winPrevious)
        return;
    QSettings reg(kInternet, QSettings::NativeFormat);
    restoreWin(reg, *winPrevious);
    if (reg.status() == QSettings::NoError) {
        winPrevious.reset();
        QSettings(kProxyBackup, QSettings::NativeFormat).clear();
    }
#elif defined(Q_OS_MACOS)
    if (!macPrevious)
        return;
    bool ok = true;
    for (const MacProxy &p : std::as_const(*macPrevious)) {
        if (p.enabled == u"Yes" && !p.server.isEmpty())
            ok = ns({QStringLiteral("-set") + p.kind, p.service, p.server, p.port}) && ok;
        else
            ok = ns({QStringLiteral("-set") + p.kind + QStringLiteral("state"), p.service, QStringLiteral("off")}) && ok;
    }
    if (ok)
        macPrevious.reset();
#else
    if (!gnomePrevious)
        return;
    bool ok = true;
    for (auto it = kProxyKeys.crbegin(); it != kProxyKeys.crend(); ++it)
        if (!gs(*it, gnomePrevious->value(*it)))
            ok = false;
    if (ok)
        gnomePrevious.reset();
#endif
}

void SysProxy::recover()
{
#ifdef Q_OS_WIN
    QMutexLocker lock(&proxyLock);
    if (winPrevious)
        return;
    QSettings backup(kProxyBackup, QSettings::NativeFormat);
    if (!backup.contains(QStringLiteral("ours")))
        return;
    QSettings reg(kInternet, QSettings::NativeFormat);
    if (reg.value(QStringLiteral("ProxyServer")).toString() == backup.value(QStringLiteral("ours")).toString()) {
        QVariantMap prev;
        for (const char *n : {"ProxyEnable", "ProxyServer", "ProxyOverride"})
            if (backup.contains(QLatin1String(n)))
                prev[QLatin1String(n)] = backup.value(QLatin1String(n));
        restoreWin(reg, prev);
        if (reg.status() != QSettings::NoError)
            return;
    }
    backup.clear();
#endif
}
