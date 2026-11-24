package dev.yzewe.rynet.data

import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

data class ExitIp(val ip: String, val country: String) {
    val flag: String
        get() = country.uppercase().takeIf { it.length == 2 && it.all { c -> c in 'A'..'Z' } }
            ?.map { String(Character.toChars(0x1F1E6 + (it - 'A'))) }?.joinToString("").orEmpty()

    companion object {
        const val DEFAULT_URL = Endpoints.IP_CHECK
        private const val MAX_BODY = 16 * 1024

        fun lookup(url: String = ""): ExitIp? = runCatching {
            val c = URL(url.trim().ifEmpty { DEFAULT_URL }).openConnection() as HttpURLConnection
            c.connectTimeout = 6000
            c.readTimeout = 6000
            c.useCaches = false
            c.setRequestProperty("Connection", "close")
            c.setRequestProperty("Accept", "application/json, text/plain")
            try {
                if (c.responseCode != 200) return null
                val body = c.inputStream.use { it.readNBytes(MAX_BODY) }.toString(Charsets.UTF_8)
                parse(body)
            } finally {
                c.disconnect()
            }
        }.getOrNull()

        private val IPV4 = Regex("""^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$""")
        private val IPV6 = Regex("""^[0-9A-Fa-f:.]+$""")

        fun isIp(s: String): Boolean {
            if (IPV4.matches(s)) return true
            if (!s.contains(':') || !IPV6.matches(s)) return false
            return runCatching { java.net.InetAddress.getByName(s) is java.net.Inet6Address }.getOrDefault(false)
        }

        fun parse(body: String): ExitIp? {
            val t = body.trim()
            if (t.isEmpty()) return null
            if (t.startsWith("{")) {
                val o = runCatching { JSONObject(t) }.getOrNull() ?: return null
                if (!o.optBoolean("success", true)) return null
                val ip = listOf("ip", "query", "origin", "ip_addr").firstNotNullOfOrNull { k ->
                    o.optString(k).trim().takeIf { it.isNotEmpty() && isIp(it) }
                } ?: return null
                val cc = listOf("country_code", "countryCode", "country").firstNotNullOfOrNull { k ->
                    o.optString(k).trim().uppercase().takeIf { it.length == 2 && it.all { c -> c in 'A'..'Z' } }
                }.orEmpty()
                return ExitIp(ip, cc)
            }
            return if (isIp(t)) ExitIp(t, "") else null
        }
    }
}
