package dev.yzewe.rynet.data

import org.json.JSONArray
import org.json.JSONObject

data class SubscriptionRuleGroup(val action: RuleAction, val domains: List<String>, val ips: List<String>) {
    val size: Int get() = domains.size + ips.size
}

data class SubscriptionRules(val name: String, val groups: List<SubscriptionRuleGroup>, val rest: RuleAction, val raw: String) {
    val size: Int get() = groups.sumOf { it.size }

    companion object {
        fun parse(raw: String?): SubscriptionRules? = raw?.let { text ->
            runCatching {
                val o = JSONObject(text)
                fun field(key: String): Any? = if (o.has(key)) o.opt(key) else o.keys().asSequence().firstOrNull { it.equals(key, true) }?.let { o.opt(it) }
                fun list(key: String): List<String> {
                    val a = field(key) as? JSONArray ?: return emptyList()
                    return (0 until a.length()).mapNotNull { (a.opt(it) as? String)?.trim()?.takeIf(String::isNotEmpty) }
                }
                val defaults = listOf("block", "proxy", "direct")
                val order = (
                    (field("RouteOrder") as? String).orEmpty().lowercase().split(Regex("[-, ]"))
                        .filter { it in defaults } + defaults
                    ).distinct()
                val groups = order.map { key ->
                    val prefix = key.replaceFirstChar { it.uppercase() }
                    SubscriptionRuleGroup(RuleAction.of(key)!!, list(prefix + "Sites"), list(prefix + "Ip"))
                }.filter { it.size > 0 }
                val global = when (val value = field("GlobalProxy")) {
                    is Boolean -> value
                    is String -> value.trim().let { it.isEmpty() || !it.equals("false", true) }
                    else -> true
                }
                SubscriptionRules(
                    (field("Name") as? String)?.trim().orEmpty(),
                    groups,
                    if (global) RuleAction.Proxy else RuleAction.Direct,
                    o.toString(2)
                )
            }.getOrNull()
        }
    }
}
