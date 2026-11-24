#pragma once

#include <QHash>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QStringList>
#include <functional>
#include <optional>

class QProcess;
class QTcpSocket;
class QTimer;

class Core : public QObject {
    Q_OBJECT
public:
    using Reply = std::function<void(const QJsonObject &)>;

    Core(bool debug, bool elevated, QObject *parent = nullptr);
    ~Core() override;

    void send(const QJsonObject &cmd);
    void sendQuiet(const QJsonObject &cmd);
    void request(QJsonObject cmd, Reply reply);
    void close();
    bool elevated() const { return m_elevated; }
    bool running() const { return alive(); }

    static QString binary();
    static QString bundled();
    static QString resource(const QString &name, bool env = true);
    static QString coreName();
    static std::optional<QByteArray> digest(const QString &file);
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
    static QString grantedCore();
    static bool granted();
#endif

signals:
    void event(const QJsonObject &e);

private:
    void startChild(bool debug);
    void startElevated(bool debug);
    void pollPort();
    void connectPort(int port, const QByteArray &token);
    void readLines(const QByteArray &chunk);
    void write(const QByteArray &line);
    void finish();
    bool alive() const;

    QProcess *m_proc = nullptr;
    QTcpSocket *m_socket = nullptr;
    QTimer *m_poll = nullptr;
    qint64 m_deadline = 0;
    QByteArray m_buffer;
    QList<QByteArray> m_pending;
    QHash<int, Reply> m_replies;
    int m_next = 0;
    bool m_closed = false;
    bool m_finished = false;
    bool m_elevated = false;
    bool m_ready = false;
};
