package dev.yzewe.rynet.data

import android.content.Context
import java.io.File

object GeoLists {
    fun file(ctx: Context, kind: String, name: String): File? {
        val dir = Geo.dir(ctx)
        val plain = File(dir, "$kind-$name.list")
        if (plain.isFile) return plain
        return dir.listFiles { f -> f.isFile && f.name.startsWith("$kind-") && f.name.endsWith("-$name.list") }?.firstOrNull()
    }

    fun value(line: String): String? {
        val l = line.trim()
        if (l.isEmpty() || l.startsWith("#")) return null
        val parts = l.split(',')
        return if (parts.size > 1 && parts[0].any { it.isLetter() }) parts[1].trim() else l
    }

    fun read(ctx: Context, kind: String, name: String): List<String>? = file(ctx, kind, name)?.let { f -> runCatching { f.useLines { ls -> ls.mapNotNull(::value).toList() } }.getOrNull() }

    private class Index(val suffix: Set<String>, val full: Set<String>, val words: List<String>, val nets: List<RuleCover.Net>)

    private val cache = object : LinkedHashMap<String, Index?>(8, 0.75f, true) {
        override fun removeEldestEntry(e: MutableMap.MutableEntry<String, Index?>) = size > 6
    }

    private fun index(ctx: Context, kind: String, name: String): Index? = synchronized(cache) {
        cache.getOrPut("$kind-$name") {
            val f = file(ctx, kind, name) ?: return@getOrPut null
            val suffix = HashSet<String>()
            val full = HashSet<String>()
            val words = mutableListOf<String>()
            val nets = mutableListOf<RuleCover.Net>()
            runCatching {
                f.useLines { ls ->
                    for (line in ls) {
                        val l = line.trim()
                        if (l.isEmpty() || l.startsWith("#")) continue
                        val v = value(l)?.lowercase() ?: continue
                        if (kind == "geoip") {
                            RuleCover.parse(v)?.let { nets += it }
                            continue
                        }
                        val tag = when {
                            ',' in l -> "m:" + l.substringBefore(',').uppercase()
                            ':' in l -> "x:" + l.substringBefore(':').lowercase()
                            else -> "x:domain"
                        }
                        when (tag) {
                            "m:DOMAIN", "x:full" -> full += v.removePrefix("full:")
                            "m:DOMAIN-KEYWORD", "x:keyword" -> words += v.removePrefix("keyword:")
                            "m:DOMAIN-SUFFIX", "x:domain" -> suffix += v.removePrefix("domain:")
                        }
                    }
                }
            }
            Index(suffix, full, words, nets)
        }
    }

    fun lookup(ctx: Context) = GeoHas { kind, name, value ->
        val k = if (kind == ModeEntry.Kind.GeoIp) "geoip" else "geosite"
        val idx = index(ctx, k, name) ?: return@GeoHas null
        if (k == "geoip") {
            val net = RuleCover.parse(value) ?: return@GeoHas false
            idx.nets.any { it.contains(net) }
        } else {
            val d = value.lowercase()
            if (d in idx.full || idx.words.any { d.contains(it) }) return@GeoHas true
            var s = d
            while (true) {
                if (s in idx.suffix) return@GeoHas true
                val dot = s.indexOf('.')
                if (dot < 0) break
                s = s.substring(dot + 1)
            }
            false
        }
    }
}
