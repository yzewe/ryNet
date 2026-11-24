package dev.yzewe.rynet.data

import dev.yzewe.rynet.tr
import java.math.BigInteger
import java.net.InetAddress

data class CoverSource(val entry: ModeEntry, val origin: String, val user: Boolean = false)

fun interface GeoHas {
    fun has(kind: ModeEntry.Kind, name: String, value: String): Boolean?

    companion object {
        val None = GeoHas { _, _, _ -> null }
    }
}

object RuleCover {
    fun find(kind: RuleKind, value: String, sources: List<CoverSource>, geo: GeoHas = GeoHas.None): CoverSource? {
        val v = value.trim().lowercase()
        if (v.isEmpty()) return null
        return when (kind) {
            RuleKind.Domain -> sources.firstOrNull { domainCovers(it.entry, v, geo) }

            RuleKind.Ip -> {
                val net = parse(v) ?: return null
                sources.firstOrNull { ipCovers(it.entry, net, v, geo) }
            }

            RuleKind.App -> sources.firstOrNull { it.entry.kind == ModeEntry.Kind.App && it.entry.value.equals(value.trim(), true) }
        }
    }

    private fun domainCovers(e: ModeEntry, d: String, geo: GeoHas): Boolean = when (e.kind) {
        ModeEntry.Kind.Suffix -> d == e.value || d.endsWith("." + e.value)
        ModeEntry.Kind.Full -> d == e.value
        ModeEntry.Kind.Keyword -> d.contains(e.value)
        ModeEntry.Kind.GeoSite -> geo.has(e.kind, e.value, d) == true
        else -> false
    }

    private fun ipCovers(e: ModeEntry, net: Net, v: String, geo: GeoHas): Boolean = when (e.kind) {
        ModeEntry.Kind.Cidr -> parse(e.value)?.contains(net) == true
        ModeEntry.Kind.GeoIp -> geo.has(e.kind, e.value, v) == true
        else -> false
    }

    class Net(val v6: Boolean, val addr: BigInteger, val prefix: Int) {
        fun contains(o: Net): Boolean {
            if (o.v6 != v6 || o.prefix < prefix) return false
            val shift = (if (v6) 128 else 32) - prefix
            return addr.shiftRight(shift) == o.addr.shiftRight(shift)
        }
    }

    fun parse(cidr: String): Net? {
        val a = cidr.trim().substringBefore('/')
        val m = if ('/' in cidr) cidr.substringAfter('/') else null
        if (a.isEmpty() || !a.all { it.isDigit() || it == '.' || it == ':' || it in 'a'..'f' || it in 'A'..'F' }) return null
        val bytes = runCatching { InetAddress.getByName(a).address }.getOrNull() ?: return null
        val prefix = m?.toIntOrNull() ?: if (m == null) bytes.size * 8 else return null
        if (prefix !in 0..bytes.size * 8) return null
        return Net(bytes.size == 16, BigInteger(1, bytes), prefix)
    }

    private fun does(a: RuleAction) = when (a) {
        RuleAction.Direct -> tr("идёт напрямую", "goes direct")
        RuleAction.Proxy -> tr("идёт через VPN", "goes through the VPN")
        RuleAction.Block -> tr("блокируется", "is blocked")
        RuleAction.Server -> tr("идёт через сервер", "goes through a server")
    }

    fun message(what: String, action: RuleAction, src: CoverSource): String {
        val rule = "${src.entry.text} (${src.origin})"
        val now = src.entry.action
        return when {
            now == action -> tr("$what уже ${does(action)} по правилу $rule", "$what already ${does(action)} by the rule $rule")

            src.user -> tr(
                "Конфликт: $what ${does(now)} по правилу $rule, оно сработает раньше нового",
                "Conflict: $what ${does(now)} by the rule $rule, which is applied before the new one"
            )

            else -> tr(
                "$what ${does(now)} по правилу $rule, но новое правило сработает раньше",
                "$what ${does(now)} by the rule $rule, but the new rule is applied first"
            )
        }
    }

    fun sources(settings: Settings, mode: List<ModeEntry>, subscription: List<ModeEntry>): List<CoverSource> {
        val mine = tr("ваши правила", "your rules")
        val out = mutableListOf<CoverSource>()
        for (r in settings.rules) {
            val k = when (r.kind) {
                RuleKind.Domain -> ModeEntry.Kind.Suffix
                RuleKind.Ip -> ModeEntry.Kind.Cidr
                RuleKind.App -> ModeEntry.Kind.App
            }
            out += CoverSource(ModeEntry(k, r.value, r.action), mine, user = true)
        }
        for (s in settings.sets.filter { it.enabled }) {
            val name = s.name.ifBlank { tr("Набор", "Set") }
            val o = tr("набор «$name»", "set \"$name\"")
            s.domains.forEach { out += CoverSource(ModeEntry(ModeEntry.Kind.Suffix, it, s.action), o, true) }
            s.ips.forEach { out += CoverSource(ModeEntry(ModeEntry.Kind.Cidr, it, s.action), o, true) }
            s.geosite.forEach { out += CoverSource(ModeEntry(ModeEntry.Kind.GeoSite, it, s.action), o, true) }
            s.apps.forEach { out += CoverSource(ModeEntry(ModeEntry.Kind.App, it, s.action), o, true) }
        }
        when (settings.routing) {
            RoutingMode.Smart -> mode.forEach { out += CoverSource(it, tr("режим Умный", "Smart mode")) }

            RoutingMode.Subscription -> if (out.isEmpty() && settings.userRules.isBlank()) {
                subscription.forEach { out += CoverSource(it, tr("режим Подписка", "Subscription mode")) }
            }

            else -> {}
        }
        return out
    }
}
