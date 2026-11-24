#include "app.h"
#include "platform.h"
#include "store.h"
#include "theme.h"
#include "ui.h"

#include <QCryptographicHash>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFontDatabase>
#include <QScreen>
#include <QGuiApplication>
#include <QHostAddress>
#include <QIcon>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalServer>
#include <QLocalSocket>
#include <QLocale>
#include <QPointer>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QStyleHints>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTimer>
#include <QUrl>
#include <QtGui/private/qhighdpiscaling_p.h>
#include <qpa/qplatformwindow.h>
#include <qpa/qwindowsysteminterface.h>

#ifdef Q_OS_UNIX
#include <QSocketNotifier>
#include <cerrno>
#include <csignal>
#include <fcntl.h>
#include <sys/socket.h>
#include <unistd.h>
#endif
#ifdef __GLIBC__
#include <malloc.h>
#endif
#ifdef Q_OS_WIN
#include <windows.h>
#endif
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
#define RYNET_FRAME_SCHEME
#endif
#ifdef Q_OS_MACOS
#include <mach-o/dyld.h>
#endif

namespace {
constexpr int kReleaseMs = 30000;

QString exeDir()
{
#ifdef Q_OS_WIN
    wchar_t buf[MAX_PATH * 4];
    const DWORD n = GetModuleFileNameW(nullptr, buf, DWORD(std::size(buf)));
    return n ? QFileInfo(QString::fromWCharArray(buf, int(n))).absolutePath() : QString();
#elif defined(Q_OS_MACOS)
    char buf[4096];
    uint32_t size = sizeof(buf);
    return _NSGetExecutablePath(buf, &size) == 0 ? QFileInfo(QFile::decodeName(buf)).canonicalPath() : QString();
#else
    const QString exe = QFile::symLinkTarget(QStringLiteral("/proc/self/exe"));
    return exe.isEmpty() ? QString() : QFileInfo(exe).absolutePath();
#endif
}

int savedScale()
{
    QFile f(Store::path(QStringLiteral("desktop.json")));
    if (!f.open(QIODevice::ReadOnly))
        return 0;
    const QJsonObject s = QJsonDocument::fromJson(f.readAll()).object().value("settings").toObject();
    return Settings::from(s).uiScale;
}

QString savedFont()
{
    QFile f(Store::path(QStringLiteral("desktop.json")));
    if (!f.open(QIODevice::ReadOnly))
        return Settings().font;
    return Settings::from(QJsonDocument::fromJson(f.readAll()).object().value("settings").toObject()).font;
}

bool externalScale = false;

void applyFont(const QString &choice)
{
    static QString roboto;
    QFont font = QFontDatabase::systemFont(QFontDatabase::GeneralFont);
    if (choice == u"roboto") {
        if (roboto.isEmpty()) {
            const QStringList families = QFontDatabase::applicationFontFamilies(QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/RobotoFlex.ttf")));
            if (!families.isEmpty())
                roboto = families.first();
        }
        if (!roboto.isEmpty())
            font = QFont(roboto);
    }
    font.setPixelSize(14);
    QGuiApplication::setFont(font);
    Ui::instance()->setFontFamily(font.family());
}

QString instanceKey()
{
    return QStringLiteral("rynet-") + QString::fromLatin1(QCryptographicHash::hash(Store::dir().toUtf8(), QCryptographicHash::Sha1).toHex().left(12));
}

bool systemRussian()
{
    const QLocale system = QLocale::system();
    const QStringList languages = system.uiLanguages();
    const QString first = languages.isEmpty() ? system.name() : languages.constFirst();
    const QString code = first.section(u'-', 0, 0).section(u'_', 0, 0).toLower();
    return code == u"ru" || code == u"be" || code == u"kk";
}

bool lockInstance()
{
    const QString file = Store::path(QStringLiteral("instance"));
#ifdef Q_OS_WIN
    const QString native = QDir::toNativeSeparators(file);
    const HANDLE h = CreateFileW(reinterpret_cast<const wchar_t *>(native.utf16()), GENERIC_READ | GENERIC_WRITE,
                                 FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE)
        return true;
    OVERLAPPED o = {};
    if (LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, MAXDWORD, 0x7fffffff, &o))
        return true;
    const DWORD error = GetLastError();
    CloseHandle(h);
    return error != ERROR_LOCK_VIOLATION;
#elif defined(Q_OS_UNIX)
    const int fd = ::open(QFile::encodeName(file).constData(), O_RDWR | O_CREAT | O_CLOEXEC, 0666);
    if (fd < 0)
        return true;
    struct flock lock = {};
    lock.l_type = F_WRLCK;
    lock.l_whence = SEEK_SET;
    if (::fcntl(fd, F_SETLK, &lock) == 0)
        return true;
    const int error = errno;
    ::close(fd);
    return error != EACCES && error != EAGAIN;
#else
    return true;
#endif
}

void showOther()
{
    QFile f(Store::path(QStringLiteral("instance.port")));
    if (!f.open(QIODevice::ReadOnly))
        return;
    bool ok = false;
    const int port = QString::fromLatin1(f.read(16)).trimmed().toInt(&ok);
    if (!ok || port <= 0 || port > 65535)
        return;
    for (const auto host : {QHostAddress::LocalHost, QHostAddress::LocalHostIPv6}) {
        QTcpSocket s;
        s.connectToHost(QHostAddress(host), quint16(port));
        if (!s.waitForConnected(800))
            continue;
        s.write("show\n");
        s.waitForBytesWritten(800);
        s.disconnectFromHost();
        return;
    }
}

#ifdef Q_OS_UNIX
int signalPipe[2] = {-1, -1};
void onSignal(int)
{
    const char b = 1;
    [[maybe_unused]] const auto n = ::write(signalPipe[0], &b, 1);
}
#endif

class Host : public QObject {
public:
    explicit Host(QObject *parent = nullptr)
        : QObject(parent)
    {
        m_release.setSingleShot(true);
        m_release.setInterval(kReleaseMs);
        connect(&m_release, &QTimer::timeout, this, [this] { release(); });
        if (const int scale = savedScale(); scale > 0 && !externalScale)
            m_factor = scale / 100.0;
        m_save.setSingleShot(true);
        m_save.setInterval(800);
        connect(&m_save, &QTimer::timeout, this, [this] { saveGeometry(); });
        App *app = App::instance();
        connect(app, &App::showRequested, this, [this] { show(); });
        connect(app, &App::quitRequested, this, [this] { quit(); });
#ifdef Q_OS_WIN
        connect(Theme::instance(), &Theme::changed, this, [this] { titlebar(); });
#endif
#if defined(RYNET_FRAME_SCHEME)
        connect(Theme::instance(), &Theme::changed, this, [] { frameScheme(); });
        frameScheme();
#endif
    }

    void show()
    {
        m_release.stop();
        if (m_quitting)
            return;
        if (!m_window && !create())
            return;
        if (!m_window->isVisible()) {
            if (m_maximized)
                m_window->showMaximized();
            else
                m_window->show();
        }
        if (m_window->windowStates() & Qt::WindowMinimized)
            m_window->setWindowStates(m_window->windowStates() & ~Qt::WindowMinimized);
        m_window->raise();
        m_window->requestActivate();
        App::instance()->setWindowVisible(true);
        if (qEnvironmentVariableIsSet("RYNET_TRACE"))
            qInfo() << "shown" << m_window->geometry() << m_window->isVisible() << m_window->windowStates();
    }

    void rescale(int percent)
    {
        if (externalScale)
            return;
        const qreal factor = percent > 0 ? percent / 100.0 : 1.0;
        if (qFuzzyCompare(factor, m_factor))
            return;
        const qreal k = m_factor / factor;
        m_factor = factor;
        QHighDpiScaling::setGlobalFactor(factor);
        m_normal = QSize(qRound(m_normal.width() * k), qRound(m_normal.height() * k));
        if (!m_window || !m_window->handle())
            return;
        QWindowSystemInterface::handleWindowDevicePixelRatioChanged<QWindowSystemInterface::SynchronousDelivery>(m_window);
        QWindowSystemInterface::handleGeometryChange<QWindowSystemInterface::SynchronousDelivery>(m_window, m_window->handle()->geometry());
        m_window->requestUpdate();
    }

    void hideToTray()
    {
        if (!m_window)
            return;
        saveGeometry();
        m_window->hide();
        App::instance()->setWindowVisible(false);
        App::instance()->setWindowFocused(false);
        m_release.start();
    }

    void quit()
    {
        if (m_quitting)
            return;
        m_quitting = true;
        if (m_window) {
            saveGeometry();
            m_window->hide();
        }
        App::instance()->shutdown();
        QCoreApplication::quit();
    }

protected:
    bool eventFilter(QObject *o, QEvent *e) override
    {
        if (e->type() == QEvent::FileOpen) {
            const QUrl url = static_cast<QFileOpenEvent *>(e)->url();
            if (url.scheme().compare(u"rynet", Qt::CaseInsensitive) == 0) {
                App::instance()->openLink(url.toString(QUrl::FullyEncoded));
                return true;
            }
            if (url.isLocalFile() && url.toLocalFile().endsWith(u".rnp", Qt::CaseInsensitive)) {
                App::instance()->openPluginFile(url.toLocalFile());
                return true;
            }
        }
        if (o == m_window && e->type() == QEvent::Close && !m_quitting) {
            e->ignore();
            if (App::instance()->trayAvailable())
                hideToTray();
            else
                quit();
            return true;
        }
        return QObject::eventFilter(o, e);
    }

private:
    bool create()
    {
        m_engine = new QQmlApplicationEngine(this);
        m_engine->loadFromModule("Rynet", "Main");
        m_window = m_engine->rootObjects().isEmpty() ? nullptr : qobject_cast<QQuickWindow *>(m_engine->rootObjects().constFirst());
        if (!m_window) {
            qCritical("ryNet: the window failed to load");
            delete m_engine;
            m_engine = nullptr;
            return false;
        }
        m_window->installEventFilter(this);
        m_window->setMinimumSize(QSize(340, 460));
        QSize size(1100, 720);
        const QStringList forced = qEnvironmentVariable("RYNET_SIZE").split(u'x');
        if (forced.size() == 2 && forced[0].toInt() > 0 && forced[1].toInt() > 0) {
            size = QSize(forced[0].toInt(), forced[1].toInt());
        } else if (const auto w = Store::window()) {
            size = QSize(w->w, w->h);
            m_maximized = w->max;
        }
        m_normal = size;
        m_window->resize(size);
        auto changed = [this] {
            if (!m_window)
                return;
            const bool max = m_window->windowStates() & (Qt::WindowMaximized | Qt::WindowFullScreen);
            if (!max && !(m_window->windowStates() & Qt::WindowMinimized))
                m_normal = m_window->size();
            m_maximized = max;
            m_save.start();
        };
        connect(m_window, &QWindow::widthChanged, this, changed);
        connect(m_window, &QWindow::heightChanged, this, changed);
        connect(m_window, &QWindow::windowStateChanged, this, changed);
        auto visibility = [this] {
            App::instance()->setWindowVisible(m_window && m_window->isVisible()
                                              && !(m_window->windowStates() & Qt::WindowMinimized));
        };
        connect(m_window, &QWindow::visibleChanged, this, visibility);
        connect(m_window, &QWindow::windowStateChanged, this, visibility);
        connect(m_window, &QWindow::activeChanged, this, [this] { App::instance()->setWindowFocused(m_window && m_window->isActive()); });
#ifdef Q_OS_WIN
        connect(m_window, &QWindow::visibleChanged, this, [this] { titlebar(); });
#endif
        return true;
    }

    void release()
    {
        if (!m_engine || (m_window && m_window->isVisible()))
            return;
        saveGeometry();
        delete m_engine;
        m_engine = nullptr;
        m_window = nullptr;
#ifdef __GLIBC__
        malloc_trim(0);
#endif
    }

    void saveGeometry()
    {
        if (!m_window || !qEnvironmentVariableIsEmpty("RYNET_SIZE"))
            return;
        m_save.stop();
        if (m_normal.width() >= 300 && m_normal.height() >= 300)
            Store::saveWindow({m_normal.width(), m_normal.height(), m_maximized});
    }

#if defined(RYNET_FRAME_SCHEME)
    static void frameScheme()
    {
        QGuiApplication::styleHints()->setColorScheme(Theme::instance()->dark() ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
    }
#endif

#ifdef Q_OS_WIN
    void titlebar()
    {
        if (!m_window || !m_window->isVisible())
            return;
        Theme *t = Theme::instance();
        Platform::titlebar(quintptr(m_window->winId()), t->dark(), t->surface().rgb() & 0xffffff);
    }
#endif

    QQmlApplicationEngine *m_engine = nullptr;
    QPointer<QQuickWindow> m_window;
    QTimer m_release, m_save;
    QSize m_normal;
    bool m_maximized = false;
    qreal m_factor = 1.0;
    bool m_quitting = false;
};
}

int main(int argc, char **argv)
{
    bool autostart = false;
    QList<QByteArray> links;
    for (int i = 1; i < argc; ++i) {
        if (qstrcmp(argv[i], "--autostart") == 0)
            autostart = true;
        else if (qstrnicmp(argv[i], "rynet:", 6) == 0)
            links << QByteArray(argv[i]).trimmed();
    }

    Store::setExeDir(exeDir());
    if (!qEnvironmentVariableIsEmpty("RYNET_PLATFORMTHEME"))
        qputenv("QT_QPA_PLATFORMTHEME", qgetenv("RYNET_PLATFORMTHEME"));
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
    else
        qputenv("QT_QPA_PLATFORMTHEME", "generic");
#endif
    externalScale = !qEnvironmentVariableIsEmpty("QT_SCALE_FACTOR");
    if (const int scale = savedScale(); scale > 0 && !externalScale)
        qputenv("QT_SCALE_FACTOR", QByteArray::number(scale / 100.0));

    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("rynet"));
    QGuiApplication::setDesktopFileName(QStringLiteral("rynet"));
    QGuiApplication::setApplicationVersion(Platform::version());
    QGuiApplication::setQuitOnLastWindowClosed(false);
    Lang::setRu(systemRussian());

    QStringList pluginFiles;
    for (const QString &arg : app.arguments().mid(1)) {
        const QString path = arg.startsWith(u"file:", Qt::CaseInsensitive) ? QUrl(arg).toLocalFile() : arg;
        if (path.endsWith(u".rnp", Qt::CaseInsensitive) && !path.contains(u'\n'))
            pluginFiles << QFileInfo(path).absoluteFilePath();
    }

    const QString key = instanceKey();
    {
        QLocalSocket probe;
        probe.connectToServer(key);
        if (probe.waitForConnected(800)) {
            for (const QByteArray &link : std::as_const(links))
                probe.write("open " + link + "\n");
            for (const QString &file : std::as_const(pluginFiles))
                probe.write("plugin " + file.toUtf8() + "\n");
            probe.write("show " + qgetenv("XDG_ACTIVATION_TOKEN") + "\n");
            probe.waitForBytesWritten(800);
            probe.disconnectFromServer();
            return 0;
        }
    }
    if (!lockInstance()) {
        showOther();
        return 0;
    }
    QLocalServer::removeServer(key);
    QLocalServer instance;
    instance.setSocketOptions(QLocalServer::UserAccessOption);
    instance.listen(key);
    QTcpServer legacy;
    if (legacy.listen(QHostAddress::LocalHost))
        Store::write(Store::path(QStringLiteral("instance.port")), QByteArray::number(legacy.serverPort()));

    QIcon icon;
    for (int s : {16, 20, 24, 32, 40, 48, 64, 128, 256})
        icon.addFile(QStringLiteral(":/icons/app-%1.png").arg(s), QSize(s, s));
    QGuiApplication::setWindowIcon(icon);
    const int symbols = QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/MaterialSymbolsRounded.ttf"));
    if (qEnvironmentVariableIsSet("RYNET_TRACE"))
        qInfo() << "font" << symbols << QFontDatabase::applicationFontFamilies(symbols);
    applyFont(savedFont());
#ifdef Q_OS_WIN
    if (const qreal dpr = app.primaryScreen() ? app.primaryScreen()->devicePixelRatio() : 1.0; qFuzzyCompare(dpr, qRound(dpr) * 1.0))
        QQuickWindow::setTextRenderType(QQuickWindow::NativeTextRendering);
    QFont::insertSubstitutions(QStringLiteral("monospace"), {QStringLiteral("Cascadia Mono"), QStringLiteral("Consolas"), QStringLiteral("Courier New")});
#endif

    App *a = App::instance();
    Host host;
    QObject::connect(a, &App::settingsChanged, &host, [&host, a, font = a->settingsRef().font, scale = a->settingsRef().uiScale]() mutable {
        if (a->settingsRef().font != font) {
            font = a->settingsRef().font;
            applyFont(font);
        }
        if (a->settingsRef().uiScale != scale) {
            scale = a->settingsRef().uiScale;
            host.rescale(scale);
        }
    });
    app.installEventFilter(&host);
    QObject::connect(&instance, &QLocalServer::newConnection, &host, [&instance, &host] {
        while (QLocalSocket *s = instance.nextPendingConnection()) {
            QObject::connect(s, &QLocalSocket::readyRead, s, [s, &host] {
                while (s->canReadLine()) {
                    const QByteArray line = s->readLine().trimmed();
                    if (line.startsWith("open ")) {
                        App::instance()->openLink(QString::fromUtf8(line.mid(5)));
                        continue;
                    }
                    if (line.startsWith("plugin ")) {
                        App::instance()->openPluginFile(QString::fromUtf8(line.mid(7)));
                        continue;
                    }
                    if (line.startsWith("show")) {
                        const QByteArray token = line.mid(4).trimmed();
                        if (!token.isEmpty())
                            qputenv("XDG_ACTIVATION_TOKEN", token);
                        host.show();
                    }
                    s->disconnectFromServer();
                    return;
                }
            });
            QObject::connect(s, &QLocalSocket::disconnected, s, &QObject::deleteLater);
        }
    });
    QObject::connect(&legacy, &QTcpServer::newConnection, &host, [&legacy, &host] {
        while (QTcpSocket *s = legacy.nextPendingConnection()) {
            QObject::connect(s, &QTcpSocket::readyRead, s, [s, &host] {
                if (!s->canReadLine()) {
                    if (s->bytesAvailable() > 64) {
                        s->abort();
                        s->deleteLater();
                    }
                    return;
                }
                if (s->readLine().trimmed() == "show")
                    host.show();
                s->disconnectFromHost();
            });
            QObject::connect(s, &QTcpSocket::disconnected, s, &QObject::deleteLater);
        }
    });

#ifdef Q_OS_UNIX
    if (::socketpair(AF_UNIX, SOCK_STREAM, 0, signalPipe) == 0) {
        auto *notifier = new QSocketNotifier(signalPipe[1], QSocketNotifier::Read, &app);
        QObject::connect(notifier, &QSocketNotifier::activated, &host, [&host] {
            char b;
            [[maybe_unused]] const auto n = ::read(signalPipe[1], &b, 1);
            host.quit();
        });
        struct sigaction sa = {};
        sa.sa_handler = onSignal;
        sigemptyset(&sa.sa_mask);
        sa.sa_flags = SA_RESTART;
        sigaction(SIGTERM, &sa, nullptr);
        sigaction(SIGINT, &sa, nullptr);
        sigaction(SIGHUP, &sa, nullptr);
    }
#endif
    QObject::connect(&app, &QCoreApplication::aboutToQuit, a, [a] { a->shutdown(); });

    a->start(autostart);
    for (const QByteArray &link : std::as_const(links))
        a->openLink(QString::fromUtf8(link));
    for (const QString &file : std::as_const(pluginFiles))
        a->openPluginFile(file);
    if (autostart) {
        QTimer::singleShot(3000, &host, [&host, a] {
            if (!a->trayAvailable())
                host.show();
        });
    } else {
        host.show();
    }
    return app.exec();
}
