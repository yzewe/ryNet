#pragma once

#include <QList>
#include <QPair>
#include <QString>

namespace Endpoints {
inline const QString Api = QStringLiteral("https://api.rynet.app");
inline const QString DownloadsHost = QStringLiteral("dl.rynet.app");
inline const QString DownloadPage = QStringLiteral("https://rynet.app/download");
inline const QString IpCheck = QStringLiteral("https://ipwho.is/?fields=success,ip,country_code");
inline const QString GeoPages = QStringLiteral("https://github.com/MetaCubeX/meta-rules-dat/blob/meta/geo");
inline const QString SpeedDown = QStringLiteral("https://speed.cloudflare.com/__down?bytes=");
inline const QPair<QString, QString> DnsGoogle = {QStringLiteral("Google"), QStringLiteral("https://8.8.8.8/dns-query")};
inline const QList<QPair<QString, QString>> Dns = {{QStringLiteral("Cloudflare"), QStringLiteral("https://1.1.1.1/dns-query")},
                                                   {QStringLiteral("AdGuard"), QStringLiteral("https://94.140.14.14/dns-query")}};
}
