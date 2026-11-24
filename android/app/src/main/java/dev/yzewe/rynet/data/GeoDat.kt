package dev.yzewe.rynet.data

import android.content.Context
import java.io.EOFException
import java.io.File
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

object GeoDat {
    private val OWN = Regex("^d[0-9a-f]{8}-")
    private val REF = Regex("\"(geosite|geoip):(!?)([A-Za-z0-9._@!-]+)\"", RegexOption.IGNORE_CASE)
    private val KINDS = listOf("geosite" to "Geositeurl", "geoip" to "Geoipurl")
    private const val MAX_ENTRY = 64 shl 20

    fun isOwn(name: String) = OWN.containsMatchIn(name)

    private fun url(o: JSONObject, key: String): String? {
        val k = o.keys().asSequence().firstOrNull { it.equals(key, ignoreCase = true) } ?: return null
        return o.optString(k).trim().takeIf { it.startsWith("https://") || it.startsWith("http://") }
    }

    private fun tag(url: String): String {
        val d = java.security.MessageDigest.getInstance("SHA-256").digest(url.toByteArray())
        return "d" + d.take(4).joinToString("") { "%02x".format(it) }
    }

    private fun refs(text: String, kind: String): Set<String> = REF.findAll(text).filter { it.groupValues[1].equals(kind, true) }
        .map { it.groupValues[3].lowercase() }
        .filter { !isOwn(it) && !(kind == "geoip" && it == "private") }
        .toSet()

    private fun file(ctx: Context, kind: String, tag: String, name: String) = File(Geo.dir(ctx), "$kind-$tag-$name.list")

    fun localize(ctx: Context, routing: String): String {
        val o = runCatching { JSONObject(routing) }.getOrNull() ?: return routing
        val tags = KINDS.mapNotNull { (kind, key) -> url(o, key)?.let { kind to tag(it) } }.toMap()
        if (tags.isEmpty()) return routing
        return REF.replace(routing) { m ->
            val kind = m.groupValues[1].lowercase()
            val name = m.groupValues[3].lowercase()
            val tag = tags[kind]
            if (tag != null && file(ctx, kind, tag, name).exists()) "\"$kind:${m.groupValues[2]}$tag-$name\"" else m.value
        }
    }

    fun update(ctx: Context, routing: String) {
        val o = runCatching { JSONObject(routing) }.getOrNull() ?: return
        for ((kind, key) in KINDS) {
            val url = url(o, key) ?: continue
            val wanted = refs(routing, kind)
            if (wanted.isEmpty()) continue
            runCatching { fetch(ctx, kind, url, tag(url), wanted) }
        }
    }

    private fun fetch(ctx: Context, kind: String, url: String, tag: String, wanted: Set<String>) {
        val etag = File(Geo.dir(ctx), "$kind-$tag.etag")
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 60_000
        if (etag.exists() && wanted.all { file(ctx, kind, tag, it).exists() }) conn.setRequestProperty("If-None-Match", etag.readText())
        try {
            val lists = withDeadline(conn, 5 * 60_000L) { d ->
                if (conn.responseCode != 200) return
                d.wrap(conn.inputStream).buffered(64 * 1024).use { if (kind == "geosite") sites(it, wanted) else ips(it, wanted) }
            }
            for ((name, lines) in lists) {
                val f = file(ctx, kind, tag, name)
                val tmp = File(f.path + ".tmp")
                tmp.writeText(lines.joinToString("\n", postfix = "\n"))
                tmp.renameTo(f)
            }
            conn.getHeaderField("ETag")?.let { etag.writeText(it) }
        } finally {
            conn.disconnect()
        }
    }

    private fun entries(input: InputStream, each: (Proto) -> Unit) {
        while (true) {
            val key = try {
                varint(input)
            } catch (_: EOFException) {
                return
            }
            val len = when (key and 7) {
                0L -> {
                    varint(input)
                    continue
                }

                2L -> varint(input)

                5L -> 4L

                1L -> 8L

                else -> error("bad .dat")
            }
            if (len !in 0..MAX_ENTRY) error("bad .dat")
            val buf = ByteArray(len.toInt())
            var got = 0
            while (got < buf.size) {
                val n = input.read(buf, got, buf.size - got)
                if (n < 0) throw EOFException()
                got += n
            }
            if (key == (1L shl 3 or 2)) each(Proto(buf))
        }
    }

    private fun varint(input: InputStream): Long {
        var r = 0L
        var shift = 0
        while (true) {
            val b = input.read()
            if (b < 0) throw EOFException()
            r = r or ((b and 0x7f).toLong() shl shift)
            if (b and 0x80 == 0) return r
            shift += 7
            if (shift > 63) error("bad varint")
        }
    }

    private class Proto(val b: ByteArray, var p: Int = 0, val end: Int = b.size) {
        fun fields(): Sequence<Pair<Int, Any>> = sequence {
            while (p < end) {
                val key = varint()
                when ((key and 7).toInt()) {
                    0 -> yield((key ushr 3).toInt() to varint())

                    2 -> {
                        val n = varint().toInt()
                        yield((key ushr 3).toInt() to b.copyOfRange(p, p + n))
                        p += n
                    }

                    1 -> p += 8

                    5 -> p += 4

                    else -> error("bad .dat")
                }
            }
        }

        private fun varint(): Long {
            var r = 0L
            var shift = 0
            while (true) {
                val x = b[p++].toInt() and 0xff
                r = r or ((x and 0x7f).toLong() shl shift)
                if (x and 0x80 == 0) return r
                shift += 7
            }
        }
    }

    private fun spec(ref: String): Triple<String, String?, Boolean> {
        val base = ref.substringBefore('@')
        val attr = ref.substringAfter('@', "").takeIf { it.isNotEmpty() }
        return Triple(base, attr?.removePrefix("!"), attr?.startsWith("!") == true)
    }

    internal fun sites(input: InputStream, wanted: Set<String>): Map<String, List<String>> {
        val byBase = wanted.groupBy { spec(it).first }
        val out = HashMap<String, List<String>>()
        entries(input) { e ->
            var code: String? = null
            val domains = ArrayList<Triple<Long, String, Set<String>>>()
            for ((f, v) in e.fields()) {
                if (f == 1 && v is ByteArray) {
                    code = String(v).lowercase()
                    if (code !in byBase) return@entries
                } else if (f == 2 && v is ByteArray) {
                    var type = 0L
                    var value = ""
                    val attrs = HashSet<String>()
                    for ((g, w) in Proto(v).fields()) {
                        when {
                            g == 1 && w is Long -> type = w
                            g == 2 && w is ByteArray -> value = String(w)
                            g == 3 && w is ByteArray -> Proto(w).fields().firstOrNull { it.first == 1 }?.let { attrs.add(String(it.second as ByteArray).lowercase()) }
                        }
                    }
                    domains.add(Triple(type, value, attrs))
                }
            }
            for (ref in byBase[code ?: return@entries].orEmpty()) {
                val (_, attr, not) = spec(ref)
                out[ref] = domains.filter { attr == null || (attr in it.third) != not }.mapNotNull { (type, value) ->
                    when (type) {
                        0L -> "keyword:$value"
                        1L -> "regexp:$value"
                        2L -> "domain:$value"
                        3L -> "full:$value"
                        else -> null
                    }
                }
            }
        }
        return out
    }

    internal fun ips(input: InputStream, wanted: Set<String>): Map<String, List<String>> {
        val out = HashMap<String, List<String>>()
        entries(input) { e ->
            var code: String? = null
            val cidrs = ArrayList<String>()
            var inverse = false
            for ((f, v) in e.fields()) {
                if (f == 1 && v is ByteArray) {
                    code = String(v).lowercase()
                    if (code !in wanted) return@entries
                } else if (f == 2 && v is ByteArray) {
                    var ip: ByteArray? = null
                    var prefix = -1L
                    for ((g, w) in Proto(v).fields()) {
                        if (g == 1 && w is ByteArray) ip = w
                        if (g == 2 && w is Long) prefix = w
                    }
                    val a = ip?.takeIf { it.size == 4 || it.size == 16 } ?: continue
                    val addr = java.net.InetAddress.getByAddress(a).hostAddress ?: continue
                    cidrs.add(if (prefix >= 0) "$addr/$prefix" else addr)
                } else if (f == 3 && v is Long) {
                    inverse = v != 0L
                }
            }
            if (!inverse) code?.let { out[it] = cidrs }
        }
        return out
    }
}
