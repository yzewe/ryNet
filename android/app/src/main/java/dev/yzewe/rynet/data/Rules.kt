package dev.yzewe.rynet.data

import dev.yzewe.rynet.tr
import org.json.JSONArray
import org.json.JSONObject

enum class RuleKind(val key: String, private val ruTitle: String, private val enTitle: String) {
    Domain("domain", "Домен", "Domain"),
    Ip("ip", "IP", "IP"),
    App("app", "Приложение", "App")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class RuleAction(val key: String, private val ruTitle: String, private val enTitle: String, val mihomo: String) {
    Proxy("proxy", "Прокси", "Proxy", "PROXY"),
    Direct("direct", "Напрямую", "Direct", "DIRECT"),
    Block("block", "Блок", "Block", "REJECT"),
    Server("server", "Сервер", "Server", "")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

data class Rule(val kind: RuleKind, val value: String, val action: RuleAction, val server: Selection? = null) {
    fun mihomo(target: String? = null): String? {
        val to = if (action == RuleAction.Server) target ?: return null else action.mihomo
        return when (kind) {
            RuleKind.Domain -> Rules.domain(value)?.let { "DOMAIN-SUFFIX,$it,$to" }
            RuleKind.Ip -> Rules.cidr(value)?.let { (c, v6) -> "${if (v6) "IP-CIDR6" else "IP-CIDR"},$c,$to,no-resolve" }
            RuleKind.App -> value.trim().takeIf { Rules.validPackage(it) }?.let { "PROCESS-NAME,$it,$to" }
        }
    }

    fun valid(): Boolean = mihomo(if (action == RuleAction.Server && server != null) "rynet-app0" else null) != null
}

object Rules {
    fun mihomo(rules: List<Rule>, extra: String, sets: List<RuleSet> = emptyList(), appsOnly: Boolean = false): String {
        val on = sets.filter { it.enabled }.map { if (appsOnly && it.action == RuleAction.Proxy) it.copy(apps = emptyList()) else it }
        val tags = servers(rules, on)
        return (
            rules.filterNot { appsOnly && it.kind == RuleKind.App && it.action == RuleAction.Proxy }
                .mapNotNull { r -> r.mihomo(r.server?.let { tags[it] }) } +
                on.flatMap { s -> s.mihomo(s.server?.let { tags[it] }) } +
                extra.lines().map { it.trim() }.filter { it.isNotEmpty() }
            ).joinToString("\n")
    }

    fun servers(rules: List<Rule>, sets: List<RuleSet> = emptyList()): Map<Selection, String> = (
        rules.filter { it.action == RuleAction.Server }.mapNotNull { it.server } +
            sets.filter { it.enabled && it.action == RuleAction.Server }.mapNotNull { it.server }
        ).distinct()
        .withIndex().associate { (i, s) -> s to "rynet-app$i" }

    fun toJson(rules: List<Rule>): JSONArray = JSONArray(
        rules.map {
            JSONObject().put("kind", it.kind.key).put("value", it.value).put("action", it.action.key)
                .apply { it.server?.let { s -> put("sub", s.sub).put("profile", s.profile) } }
        }
    )

    fun fromJson(a: JSONArray?): List<Rule> = (0 until (a?.length() ?: 0)).mapNotNull { i ->
        val o = a!!.optJSONObject(i) ?: return@mapNotNull null
        Rule(
            RuleKind.of(o.optString("kind")) ?: return@mapNotNull null,
            o.optString("value").takeIf { it.isNotBlank() } ?: return@mapNotNull null,
            RuleAction.of(o.optString("action")) ?: RuleAction.Proxy,
            o.optString("sub").takeIf { it.isNotEmpty() }?.let { sub -> o.optString("profile").takeIf { it.isNotEmpty() }?.let { Selection(sub, it) } }
        ).let { if (it.action == RuleAction.Server && it.server == null) it.copy(action = RuleAction.Proxy) else it }
    }

    fun domain(input: String): String? {
        val d = input.trim().lowercase()
            .substringAfter("://").substringBefore('/').substringBefore(':')
            .removePrefix("*.").removePrefix("+.").removePrefix(".")
        return d.takeIf { DOMAIN.matches(it) }
    }

    fun cidr(input: String): Pair<String, Boolean>? {
        val v = input.trim()
        val (addr, mask) = v.split('/', limit = 2).let { it[0] to it.getOrNull(1) }
        val v6 = addr.contains(':')
        val ok = if (v6) {
            addr.matches(Regex("^[0-9a-fA-F:.]+$")) && addr.count { it == ':' } >= 2
        } else {
            addr.split('.').let { p -> p.size == 4 && p.all { it.toIntOrNull() in 0..255 } }
        }
        if (!ok) return null
        val max = if (v6) 128 else 32
        val m = mask?.toIntOrNull() ?: if (mask == null) max else return null
        if (m !in 0..max) return null
        return "$addr/$m" to v6
    }

    fun validPackage(p: String) = PACKAGE.matches(p.trim())

    private val DOMAIN = Regex("^([a-z0-9]([a-z0-9-]*[a-z0-9])?\\.)*[a-z0-9]([a-z0-9-]*[a-z0-9])?$|^[a-z0-9-]+$")
    private val PACKAGE = Regex("^[A-Za-z][A-Za-z0-9_]*(\\.[A-Za-z0-9_]+)+$")
}
