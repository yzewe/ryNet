package dev.yzewe.rynet.data

import dev.yzewe.rynet.core.Native
import org.json.JSONArray
import org.json.JSONObject

data class ModeEntry(val kind: Kind, val value: String, val action: RuleAction) {
    enum class Kind { Suffix, Full, Keyword, Regex, GeoSite, Cidr, GeoIp, App, Protocol }

    val geo: Boolean get() = kind == Kind.GeoSite || kind == Kind.GeoIp

    val text: String
        get() = when (kind) {
            Kind.Suffix -> "domain:$value"
            Kind.Full -> "full:$value"
            Kind.Keyword -> "keyword:$value"
            Kind.Regex -> "regexp:$value"
            Kind.GeoSite -> "geosite:$value"
            Kind.GeoIp -> "geoip:$value"
            Kind.Protocol -> "protocol:$value"
            Kind.Cidr, Kind.App -> value
        }
}

object ModeRules {
    private fun json(): String = runCatching { Native.smartRules() }.getOrNull()?.takeIf { it.isNotBlank() } ?: FALLBACK

    fun smart(): List<ModeEntry> = runCatching { fromXray(json()) }.getOrNull() ?: emptyList()

    fun privateNets(): List<String> = runCatching { JSONObject(json()).getJSONArray("private").let { a -> (0 until a.length()).map { a.getString(it) } } }
        .getOrDefault(emptyList())

    fun fromXray(json: String): List<ModeEntry> {
        val rules = JSONObject(json).optJSONArray("rules") ?: JSONArray()
        val out = mutableListOf<ModeEntry>()
        for (i in 0 until rules.length()) {
            val r = rules.optJSONObject(i) ?: continue
            val action = when (r.optString("outboundTag")) {
                "direct" -> RuleAction.Direct
                "block" -> RuleAction.Block
                else -> RuleAction.Proxy
            }
            fun list(key: String): List<String> = r.optJSONArray(key)?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()
            list("protocol").forEach { out += ModeEntry(ModeEntry.Kind.Protocol, it, action) }
            list("ip").forEach { out += ip(it, action) }
            list("domain").forEach { out += domain(it, action) }
        }
        return out
    }

    fun domain(raw: String, action: RuleAction): ModeEntry {
        val v = raw.trim()
        val p = if (':' in v) v.substringBefore(':').lowercase() else ""
        val rest = v.substringAfter(':', v)
        return when (p) {
            "geosite" -> ModeEntry(ModeEntry.Kind.GeoSite, rest.lowercase(), action)
            "domain" -> ModeEntry(ModeEntry.Kind.Suffix, rest.lowercase(), action)
            "full" -> ModeEntry(ModeEntry.Kind.Full, rest.lowercase(), action)
            "keyword" -> ModeEntry(ModeEntry.Kind.Keyword, rest.lowercase(), action)
            "regexp" -> ModeEntry(ModeEntry.Kind.Regex, rest, action)
            else -> ModeEntry(ModeEntry.Kind.Suffix, v.lowercase(), action)
        }
    }

    fun ip(raw: String, action: RuleAction): ModeEntry {
        val v = raw.trim()
        return if (v.startsWith("geoip:", true)) {
            ModeEntry(ModeEntry.Kind.GeoIp, v.substring(6).lowercase().removePrefix("!"), action)
        } else {
            ModeEntry(ModeEntry.Kind.Cidr, v, action)
        }
    }

    fun fromConfig(body: String?, remarks: String?): Pair<List<ModeEntry>, RuleAction?>? {
        val text = body?.trim()?.takeIf { it.startsWith("[") || it.startsWith("{") } ?: return null
        val configs = runCatching {
            if (text.startsWith("[")) JSONArray(text).let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) } } else listOf(JSONObject(text))
        }.getOrNull() ?: return null
        fun rulesOf(c: JSONObject) = c.optJSONObject("routing")?.optJSONArray("rules")?.takeIf { it.length() > 0 }
        val config = (if (remarks == null) configs.firstOrNull() else configs.firstOrNull { it.optString("remarks") == remarks }) ?: return null
        val rules = rulesOf(config) ?: return null
        val kinds = HashMap<String, String>()
        config.optJSONArray("outbounds")?.let { o -> for (i in 0 until o.length()) o.optJSONObject(i)?.let { kinds[it.optString("tag")] = it.optString("protocol") } }
        fun action(r: JSONObject): RuleAction = when (kinds[r.optString("outboundTag")]) {
            "freedom" -> RuleAction.Direct

            "blackhole" -> RuleAction.Block

            null -> when (r.optString("outboundTag")) {
                "direct" -> RuleAction.Direct
                "block" -> RuleAction.Block
                else -> RuleAction.Proxy
            }

            else -> RuleAction.Proxy
        }
        val out = mutableListOf<ModeEntry>()
        var rest: RuleAction? = null
        for (i in 0 until rules.length()) {
            val r = rules.optJSONObject(i) ?: continue
            if (r.has("inboundTag") && !r.has("domain") && !r.has("ip")) continue
            val act = action(r)
            fun list(key: String): List<String> = when (val v = r.opt(key)) {
                is JSONArray -> (0 until v.length()).map { v.optString(it) }
                is String -> v.split(',').map { it.trim() }.filter { it.isNotEmpty() }
                else -> emptyList()
            }
            val protocols = list("protocol")
            val ips = list("ip")
            val domains = list("domain")
            protocols.forEach { out += ModeEntry(ModeEntry.Kind.Protocol, it, act) }
            ips.forEach { out += ip(it, act) }
            domains.forEach { out += domain(it, act) }
            if (protocols.isEmpty() && ips.isEmpty() && domains.isEmpty() && !r.has("port") && rest == null) rest = act
        }
        return out to rest
    }

    fun subscription(rules: SubscriptionRules?): List<ModeEntry> = rules?.groups.orEmpty().flatMap { g -> g.domains.map { domain(it, g.action) } + g.ips.map { ip(it, g.action) } }

    internal const val FALLBACK = """{"rules":[
{"protocol":["bittorrent"],"outboundTag":"direct"},
{"ip":["geoip:private"],"outboundTag":"direct"},
{"domain":["geosite:category-ru","domain:ru","domain:su","domain:xn--p1ai","domain:yandex.net","domain:yandex.com","domain:vk.com","domain:userapi.com","domain:mycdn.me","domain:ok.ru","domain:mail.ru"],"outboundTag":"direct"},
{"ip":["geoip:ru"],"outboundTag":"direct"},
{"network":"tcp,udp","outboundTag":"proxy"}],
"private":["10.0.0.0/8","172.16.0.0/12","192.168.0.0/16","169.254.0.0/16","127.0.0.0/8","224.0.0.0/4","fc00::/7","fe80::/10","ff00::/8"],"dns":"77.88.8.8:53"}"""
}
