#pragma once

#include <QMetaObject>
#include <QObject>
#include <QPointer>
#include <QStringList>
#include <QThread>
#include <functional>
#include <optional>

namespace Platform {
bool windows();
QString version();

std::optional<QString> out(const QString &program, const QStringList &args, int timeoutMs = 10000);

void open(const QString &target);
QString launcher();
void autostart(bool on);
QStringList processes();
void notify(const QString &title, const QString &text);
void notifyOnce(const QString &key, const QString &title, const QString &text);

bool firewallReady();
QString grantTun();
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
QString polkitScript();
#endif
bool checkWinTun();

#ifdef Q_OS_WIN
void titlebar(quintptr hwnd, bool dark, unsigned rgb);
#endif

template <class T>
void background(QObject *ctx, std::function<T()> work, std::function<void(T)> done)
{
    QPointer<QObject> guard(ctx);
    QThread *th = QThread::create([guard, work = std::move(work), done = std::move(done)]() mutable {
        T result = work();
        if (!guard)
            return;
        QMetaObject::invokeMethod(guard, [guard, done = std::move(done), result = std::move(result)]() mutable {
            if (guard)
                done(std::move(result));
        }, Qt::QueuedConnection);
    });
    QObject::connect(th, &QThread::finished, th, &QObject::deleteLater);
    th->start();
}
}

namespace SysProxy {
bool available();
bool enable(int port);
void disable();
void recover();
}
