#pragma once

#include <QObject>
#include <QVariantMap>
#include <functional>

namespace FilePick {
void pick(QObject *ctx, bool save, const QString &name, const QString &title, const QString &ext, std::function<void(const QString &)> done);
}

#ifdef RYNET_DBUS
class PortalRequest : public QObject {
    Q_OBJECT
public:
    PortalRequest(std::function<void(uint, const QVariantMap &)> done, QObject *parent);

public slots:
    void response(uint code, const QVariantMap &results);

private:
    std::function<void(uint, const QVariantMap &)> m_done;
};
#endif
