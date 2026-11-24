package dev.yzewe.rynet.data

object Endpoints {
    const val API = "https://api.rynet.app"
    const val PRIVACY = "https://rynet.app/privacy"
    const val DOWNLOADS_HOST = "dl.rynet.app"
    const val DOWNLOADS = "https://$DOWNLOADS_HOST"
    const val IP_CHECK = "https://ipwho.is/?fields=success,ip,country_code"
    const val GEO_FILES = "https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo"
    const val GEO_PAGES = "https://github.com/MetaCubeX/meta-rules-dat/blob/meta/geo"
    const val SPEED_DOWN = "https://speed.cloudflare.com/__down?bytes="
    const val DNS_GOOGLE = "https://8.8.8.8/dns-query"
    val DNS = listOf(
        "Cloudflare" to "https://1.1.1.1/dns-query",
        "AdGuard" to "https://94.140.14.14/dns-query"
    )
}
