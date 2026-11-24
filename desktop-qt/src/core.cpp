#include "core.h"

#include "model.h"
#include "store.h"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHostAddress>
#include <QJsonDocument>
#include <QProcess>
#include <QStandardPaths>
#include <QTcpSocket>
#include <QTimer>

Core::Core(bool debug, bool elevated, QObject *parent)
    : QObject(parent)
    , m_elevated(elevated)
{
    if (elevated)
        startElevated(debug);
    else
        startChild(debug);
}

Core::~Core()
{
    const bool died = m_finished && !m_closed && !m_elevated && m_proc;
    m_closed = true;
    if (m_proc && m_proc->state() != QProcess::NotRunning) {
        m_proc->closeWriteChannel();
        if (!m_proc->waitForFinished(2000))
            m_proc->kill();
    } else if (died) {
        QProcess p;
        p.setProcessEnvironment(m_proc->processEnvironment());
        p.setStandardOutputFile(QProcess::nullDevice());
        p.setStandardErrorFile(Store::path(QStringLiteral("core.log")), QIODevice::Append);
        p.start(binary(), {});
        if (p.waitForStarted(3000)) {
            p.closeWriteChannel();
            if (!p.waitForFinished(3000))
                p.kill();
        }
    }
}

static void trimLog(const QString &file)
{
    constexpr qint64 max = 8LL << 20, keep = 5LL << 20;
    QFile f(file);
    if (f.size() <= max || !f.open(QIODevice::ReadWrite))
        return;
    f.seek(f.size() - keep);
    QByteArray tail = f.read(keep);
    tail.remove(0, tail.indexOf('\n') + 1);
    f.resize(0);
    f.seek(0);
    f.write(tail);
}

QString Core::coreName()
{
#ifdef Q_OS_WIN
    return QStringLiteral("rynet-core.exe");
#else
    return QStringLiteral("rynet-core");
#endif
}

void Core::startChild(bool debug)
{
    m_proc = new QProcess(this);
    QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
    if (debug)
        env.insert(QStringLiteral("RUST_LOG"), QStringLiteral("info,rv=debug"));
    if (!Store::portableDir().isEmpty())
        env.insert(QStringLiteral("RYNET_HOME"), Store::portableDir());
    if (!Lang::ru())
        env.insert(QStringLiteral("RYNET_LANG"), QStringLiteral("en"));
    m_proc->setProcessEnvironment(env);
    trimLog(Store::path(QStringLiteral("core.log")));
    m_proc->setStandardErrorFile(Store::path(QStringLiteral("core.log")), QIODevice::Append);
    connect(m_proc, &QProcess::readyReadStandardOutput, this, [this] { readLines(m_proc->readAllStandardOutput()); });
    connect(m_proc, &QProcess::finished, this, [this] { finish(); });
    connect(m_proc, &QProcess::errorOccurred, this, [this](QProcess::ProcessError e) {
        if (e == QProcess::FailedToStart)
            finish();
    });
    m_proc->start(binary(), {});
}

void Core::startElevated(bool debug)
{
    const QDir dir(Store::userDir());
    QFile::remove(dir.filePath(QStringLiteral("core.port")));
    auto flag = [&](const char *name, const QByteArray &value) {
        if (value.isEmpty()) {
            QFile::remove(dir.filePath(QLatin1String(name)));
        } else {
            QFile f(dir.filePath(QLatin1String(name)));
            if (f.open(QIODevice::WriteOnly))
                f.write(value);
        }
    };
    flag("core.debug", debug ? QByteArray("1") : QByteArray());
    flag("core.lang", Lang::ru() ? QByteArray() : QByteArray("en"));
    auto *run = new QProcess(this);
    run->setProcessChannelMode(QProcess::MergedChannels);
    auto failed = [this](const QString &why) {
        if (!m_closed)
            emit event(QJsonObject{{"ev", "error"},
                                   {"message", t("Ядро с правами не запустилось: %1", "The elevated core didn't start: %1").arg(why)},
                                   {"elevatedFailed", true}});
        finish();
    };
    connect(run, &QProcess::finished, this, [this, run, failed](int code, QProcess::ExitStatus status) {
        const QString text = QString::fromLocal8Bit(run->readAll()).trimmed();
        run->deleteLater();
        if (m_closed)
            return;
        if (status != QProcess::NormalExit || code != 0) {
            failed(text.isEmpty() ? t("задача не запустилась", "the task did not start") : text);
            return;
        }
        m_deadline = nowMillis() + 20000;
        m_poll = new QTimer(this);
        m_poll->setInterval(100);
        connect(m_poll, &QTimer::timeout, this, &Core::pollPort);
        m_poll->start();
    });
    connect(run, &QProcess::errorOccurred, this, [run, failed](QProcess::ProcessError e) {
        if (e == QProcess::FailedToStart) {
            run->deleteLater();
            failed(run->errorString());
        }
    });
    run->start(QStringLiteral("schtasks"), {QStringLiteral("/run"), QStringLiteral("/tn"), QStringLiteral("\\ryNet\\Core")});
    QTimer::singleShot(15000, run, [run] {
        if (run->state() != QProcess::NotRunning)
            run->kill();
    });
}

void Core::pollPort()
{
    QFile f(QDir(Store::userDir()).filePath(QStringLiteral("core.port")));
    if (f.open(QIODevice::ReadOnly)) {
        const QList<QByteArray> parts = f.readAll().trimmed().split(' ');
        if (parts.size() == 2 && parts[0].toInt() > 0) {
            m_poll->stop();
            connectPort(parts[0].toInt(), parts[1]);
            return;
        }
    }
    if (m_closed || nowMillis() > m_deadline) {
        m_poll->stop();
        if (!m_closed)
            emit event(QJsonObject{{"ev", "error"},
                                   {"message", t("Ядро с правами не запустилось: ядро не ответило", "The elevated core didn't start: the core did not respond")},
                                   {"elevatedFailed", true}});
        finish();
    }
}

void Core::connectPort(int port, const QByteArray &token)
{
    m_socket = new QTcpSocket(this);
    connect(m_socket, &QTcpSocket::connected, this, [this, token] {
        m_socket->setSocketOption(QAbstractSocket::LowDelayOption, 1);
        m_socket->write(token + '\n');
        m_ready = true;
        for (const QByteArray &l : std::as_const(m_pending))
            write(l);
        m_pending.clear();
    });
    connect(m_socket, &QTcpSocket::readyRead, this, [this] { readLines(m_socket->readAll()); });
    connect(m_socket, &QTcpSocket::disconnected, this, [this] { finish(); });
    connect(m_socket, &QTcpSocket::errorOccurred, this, [this](QAbstractSocket::SocketError) {
        if (!m_ready && !m_closed)
            emit event(QJsonObject{{"ev", "error"},
                                   {"message", t("Ядро с правами не запустилось: %1", "The elevated core didn't start: %1").arg(m_socket->errorString())},
                                   {"elevatedFailed", true}});
        finish();
    });
    m_socket->connectToHost(QHostAddress(QHostAddress::LocalHost), quint16(port));
}

void Core::readLines(const QByteArray &chunk)
{
    m_buffer += chunk;
    qsizetype nl;
    while ((nl = m_buffer.indexOf('\n')) >= 0) {
        const QByteArray line = m_buffer.left(nl);
        m_buffer.remove(0, nl + 1);
        const QJsonDocument doc = QJsonDocument::fromJson(line);
        if (!doc.isObject())
            continue;
        const QJsonObject e = doc.object();
        const int req = e.value("req").toInt(-1);
        if (req >= 0 && m_replies.contains(req)) {
            m_replies.take(req)(e);
            continue;
        }
        emit event(e);
    }
}

bool Core::alive() const
{
    if (m_finished || m_closed)
        return false;
    if (m_elevated)
        return !m_socket || m_socket->state() != QAbstractSocket::UnconnectedState;
    return m_proc && m_proc->state() != QProcess::NotRunning;
}

void Core::write(const QByteArray &line)
{
    if (m_elevated) {
        if (m_socket)
            m_socket->write(line + '\n');
    } else if (m_proc) {
        m_proc->write(line + '\n');
    }
}

void Core::send(const QJsonObject &cmd)
{
    const QByteArray line = QJsonDocument(cmd).toJson(QJsonDocument::Compact);
    if (!alive()) {
        const QString why = t("Не удалось отправить команду ядру", "Couldn't send a command to the core");
        if (const int req = cmd.value("req").toInt(-1); req >= 0 && m_replies.contains(req))
            m_replies.take(req)(QJsonObject{{"error", why}});
        if (!m_closed)
            emit event(QJsonObject{{"ev", "error"}, {"message", why}});
        return;
    }
    if (m_elevated && !m_ready) {
        m_pending << line;
        return;
    }
    write(line);
}

void Core::sendQuiet(const QJsonObject &cmd)
{
    if (!alive())
        return;
    const QByteArray line = QJsonDocument(cmd).toJson(QJsonDocument::Compact);
    if (m_elevated && !m_ready) {
        m_pending << line;
        return;
    }
    write(line);
}

void Core::request(QJsonObject cmd, Reply reply)
{
    const int id = ++m_next;
    m_replies.insert(id, std::move(reply));
    QTimer::singleShot(60000, this, [this, id] {
        if (m_replies.contains(id))
            m_replies.take(id)(QJsonObject{{"error", t("Ядро не ответило за 60 секунд", "The core did not respond within 60 seconds")}});
    });
    cmd["req"] = id;
    send(cmd);
}

void Core::finish()
{
    if (m_finished)
        return;
    m_finished = true;
    const auto replies = std::exchange(m_replies, {});
    for (const Reply &r : replies)
        r(QJsonObject{{"error", t("Ядро остановилось", "Core stopped")}});
    if (!m_closed)
        emit event(QJsonObject{{"ev", "exited"}});
}

void Core::close()
{
    m_closed = true;
    m_replies.clear();
    if (m_poll)
        m_poll->stop();
    if (m_socket)
        m_socket->disconnectFromHost();
    if (m_proc && m_proc->state() != QProcess::NotRunning) {
        m_proc->closeWriteChannel();
        QProcess *p = m_proc;
        m_proc = nullptr;
        p->setParent(nullptr);
        disconnect(p, nullptr, this, nullptr);
        QObject::connect(p, &QProcess::finished, p, &QObject::deleteLater);
        QTimer::singleShot(3000, p, [p] {
            if (p->state() != QProcess::NotRunning)
                p->kill();
        });
    }
}

QString Core::resource(const QString &name, bool env_)
{
    const QString here = QDir(QCoreApplication::applicationDirPath()).filePath(name);
    if (QFileInfo::exists(here))
        return here;
    const QString env = env_ ? qEnvironmentVariable("RYNET_CORE") : QString();
    if (!env.isEmpty()) {
        const QString beside = QFileInfo(env).dir().filePath(name);
        if (QFileInfo::exists(beside))
            return beside;
    }
    return {};
}

static QString stableCopy(const QString &src, const QString &base)
{
    const QString dir = QDir(base).filePath(QStringLiteral("rynet/bin"));
    QDir().mkpath(dir);
    const QString copy = QDir(dir).filePath(Core::coreName());
    QFile a(src), b(copy);
    bool same = false;
    if (a.open(QIODevice::ReadOnly) && b.open(QIODevice::ReadOnly) && a.size() == b.size())
        same = a.readAll() == b.readAll();
    a.close();
    b.close();
    if (!same) {
        const QString tmp = copy + QStringLiteral(".new");
        QFile::remove(tmp);
        if (QFile::copy(src, tmp)) {
            QFile::setPermissions(tmp, QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner | QFile::ReadGroup | QFile::ExeGroup | QFile::ReadOther | QFile::ExeOther);
            QFile::remove(copy);
            QFile::rename(tmp, copy);
        }
    }
    return copy;
}

std::optional<QByteArray> Core::digest(const QString &file)
{
    QFile f(file);
    if (!f.open(QIODevice::ReadOnly))
        return {};
    QCryptographicHash h(QCryptographicHash::Sha256);
    if (!h.addData(&f))
        return {};
    return h.result();
}

#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
QString Core::grantedCore() { return QStringLiteral("/usr/local/lib/rynet/rynet-core"); }

bool Core::granted()
{
    const QString own = bundled(), sys = grantedCore();
    if (!QFileInfo::exists(sys) || QFileInfo(sys).size() != QFileInfo(own).size())
        return false;
    const auto a = digest(own), b = digest(sys);
    return a && b && *a == *b;
}
#endif

QString Core::binary()
{
    const QString env = qEnvironmentVariable("RYNET_CORE");
    if (!env.isEmpty())
        return env;
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
    if (granted())
        return grantedCore();
#endif
    return bundled();
}

QString Core::bundled()
{
    const QString res = resource(coreName(), false);
    if (!res.isEmpty()) {
        if (!qEnvironmentVariableIsEmpty("APPIMAGE")) {
            QString data = qEnvironmentVariable("XDG_DATA_HOME");
            if (data.isEmpty())
                data = QDir::homePath() + QStringLiteral("/.local/share");
            return stableCopy(res, data);
        }
        if (QFileInfo(res).isExecutable())
            return res;
        if (QFile::setPermissions(res, QFile::permissions(res) | QFile::ExeOwner | QFile::ExeUser) && QFileInfo(res).isExecutable())
            return res;
        QString cache = qEnvironmentVariable("XDG_CACHE_HOME");
        if (cache.isEmpty())
            cache = QDir::homePath() + QStringLiteral("/.cache");
        return stableCopy(res, cache);
    }
    for (const QString &start : {QDir::currentPath(), QCoreApplication::applicationDirPath()}) {
        QDir d(start);
        do {
            const QString f = d.filePath(QStringLiteral("core/target/release/") + coreName());
            if (QFileInfo(f).isExecutable())
                return f;
        } while (d.cdUp());
    }
    return coreName();
}
