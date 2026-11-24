#include "tray.h"

#include "core.h"
#include "model.h"

#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>

namespace {
QJsonArray icons(const QString &name)
{
    QJsonArray a;
    for (const int size : {16, 20, 24, 32, 40, 48, 64}) {
        const QImage img = QImage(QStringLiteral(":/icons/%1-%2.png").arg(name).arg(size)).convertToFormat(QImage::Format_ARGB32);
        if (img.isNull())
            continue;
        QByteArray bytes;
        bytes.reserve(img.width() * img.height() * 4);
        for (int y = 0; y < img.height(); ++y) {
            const QRgb *line = reinterpret_cast<const QRgb *>(img.constScanLine(y));
            for (int x = 0; x < img.width(); ++x) {
                const QRgb p = line[x];
                bytes.append(char(qAlpha(p)));
                bytes.append(char(qRed(p)));
                bytes.append(char(qGreen(p)));
                bytes.append(char(qBlue(p)));
            }
        }
        a.append(QJsonObject{{"w", img.width()}, {"h", img.height()}, {"argb", QString::fromLatin1(bytes.toBase64())}});
    }
    return a;
}
}

Tray::Tray(QObject *parent)
    : QObject(parent)
{
}

Tray::~Tray() { close(); }

void Tray::start()
{
    m_proc = new QProcess(this);
    QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
    if (!Lang::ru())
        env.insert(QStringLiteral("RYNET_LANG"), QStringLiteral("en"));
    m_proc->setProcessEnvironment(env);
    m_proc->setStandardErrorFile(QProcess::nullDevice());
    connect(m_proc, &QProcess::readyReadStandardOutput, this, [this] {
        m_buffer += m_proc->readAllStandardOutput();
        qsizetype nl;
        while ((nl = m_buffer.indexOf('\n')) >= 0) {
            const QJsonObject e = QJsonDocument::fromJson(m_buffer.left(nl)).object();
            m_buffer.remove(0, nl + 1);
            if (e.value("ev").toString() == u"ready") {
                m_available = e.value("ok").toBool();
                emit availableChanged();
            } else if (!e.isEmpty()) {
                emit event(e);
            }
        }
    });
    connect(m_proc, &QProcess::finished, this, [this] {
        if (m_available) {
            m_available = false;
            emit availableChanged();
        }
    });
    m_proc->start(Core::binary(), {QStringLiteral("--tray")});
    send(QJsonObject{{"cmd", "icons"}, {"on", icons(QStringLiteral("app"))}, {"off", icons(QStringLiteral("tray-off"))}});
}

void Tray::update(bool on, bool busy, const QString &title, const QString &server, const QStringList &servers, const QString &current)
{
    const QJsonObject cmd{{"cmd", "state"},   {"on", on},     {"busy", busy}, {"title", title}, {"server", server},
                          {"servers", QJsonArray::fromStringList(servers)}, {"current", current}};
    const QByteArray line = QJsonDocument(cmd).toJson(QJsonDocument::Compact);
    if (line == m_last)
        return;
    m_last = line;
    send(cmd);
}

void Tray::notify(const QString &title, const QString &text) { send(QJsonObject{{"cmd", "notify"}, {"title", title}, {"text", text}}); }

void Tray::send(const QJsonObject &cmd)
{
    if (m_proc && m_proc->state() != QProcess::NotRunning)
        m_proc->write(QJsonDocument(cmd).toJson(QJsonDocument::Compact) + '\n');
}

void Tray::close()
{
    if (!m_proc)
        return;
    m_proc->closeWriteChannel();
    if (!m_proc->waitForFinished(1000))
        m_proc->kill();
    m_proc = nullptr;
}
