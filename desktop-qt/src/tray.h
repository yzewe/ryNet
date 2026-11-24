#pragma once

#include <QJsonObject>
#include <QObject>
#include <QStringList>

class QProcess;

class Tray : public QObject {
    Q_OBJECT
public:
    explicit Tray(QObject *parent = nullptr);
    ~Tray() override;

    void start();
    void update(bool on, bool busy, const QString &title, const QString &server, const QStringList &servers, const QString &current);
    void notify(const QString &title, const QString &text);
    void close();
    bool available() const { return m_available; }

signals:
    void event(const QJsonObject &e);
    void availableChanged();

private:
    void send(const QJsonObject &cmd);

    QProcess *m_proc = nullptr;
    QByteArray m_buffer;
    QByteArray m_last;
    bool m_available = false;
};
