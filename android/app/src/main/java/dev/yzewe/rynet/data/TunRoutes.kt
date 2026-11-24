package dev.yzewe.rynet.data

import java.math.BigInteger
import java.net.InetAddress

internal data class TunRoutes(val excluded: List<Pair<String, Int>>, val ipv6: List<Pair<String, Int>>) {
    companion object {
        private val LAN = listOf(
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "224.0.0.0/4"
        ).mapNotNull { Prefix.parse(it) }

        fun of(settings: Settings): TunRoutes = of(settings.effectiveRules())

        const val V6_REST = "IP-CIDR6,::/0,REJECT,no-resolve"

        fun guarded(rules: String): String {
            if (of(rules).ipv6.isEmpty()) return rules
            val lines = rules.lines()
            val at = lines.indexOfFirst { it.trim().removePrefix("-").trim().startsWith("MATCH", ignoreCase = true) }.let { if (it < 0) lines.size else it }
            return (lines.take(at) + V6_REST + lines.drop(at)).joinToString("\n")
        }

        fun of(rules: String): TunRoutes {
            val keep = rules.lineSequence().mapNotNull { line ->
                val parts = line.trim().removePrefix("-").split(',').map { it.trim() }
                if (parts.size < 3 || parts[0].uppercase() !in setOf("IP-CIDR", "IP-CIDR6") ||
                    parts[2].isEmpty() || parts[2].equals("DIRECT", true)
                ) {
                    null
                } else {
                    Prefix.parse(parts[1])
                }
            }.distinct().toList()
            fun subtract(prefix: Prefix): List<Prefix> {
                val overlaps = keep.filter { it.bits == prefix.bits && it.overlaps(prefix) }
                if (overlaps.isEmpty()) return listOf(prefix)
                if (overlaps.any { it.length <= prefix.length }) return emptyList()
                return prefix.children().flatMap { subtract(it) }
            }
            return TunRoutes(LAN.flatMap { subtract(it) }.map { it.route() }, keep.filter { it.bits == 128 }.map { it.route() })
        }
    }

    private data class Prefix(val base: BigInteger, val length: Int, val bits: Int) {
        private val end: BigInteger get() = base + BigInteger.ONE.shiftLeft(bits - length)
        fun overlaps(other: Prefix): Boolean = base < other.end && other.base < end
        fun children() = listOf(copy(length = length + 1), copy(base = base + BigInteger.ONE.shiftLeft(bits - length - 1), length = length + 1))
        fun route(): Pair<String, Int> {
            val raw = base.toByteArray()
            val bytes = ByteArray(bits / 8)
            val count = minOf(raw.size, bytes.size)
            raw.copyInto(bytes, bytes.size - count, raw.size - count)
            return InetAddress.getByAddress(bytes).hostAddress!! to length
        }

        companion object {
            fun parse(input: String): Prefix? = runCatching {
                val cidr = Rules.cidr(input) ?: return null
                val addr = cidr.first.substringBefore('/')
                val bits = if (cidr.second) 128 else 32
                val length = cidr.first.substringAfter('/').toInt()
                val bytes = InetAddress.getByName(addr).address
                if (bytes.size * 8 != bits) return null
                val ip = BigInteger(1, bytes)
                Prefix(ip.shiftRight(bits - length).shiftLeft(bits - length), length, bits)
            }.getOrNull()
        }
    }
}
