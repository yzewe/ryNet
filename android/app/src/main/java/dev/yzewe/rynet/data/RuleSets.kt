package dev.yzewe.rynet.data

import android.util.Base64
import java.io.ByteArrayOutputStream
import java.util.zip.Deflater
import java.util.zip.Inflater
import org.json.JSONArray
import org.json.JSONObject

data class RuleSet(
    val id: String,
    val name: String = "",
    val shape: Int = 0,
    val icon: Int = 0,
    val action: RuleAction = RuleAction.Proxy,
    val server: Selection? = null,
    val enabled: Boolean = true,
    val domains: List<String> = emptyList(),
    val apps: List<String> = emptyList(),
    val ips: List<String> = emptyList(),
    val geosite: List<String> = emptyList()
) {
    val size: Int get() = domains.size + apps.size + ips.size + geosite.size

    fun mihomo(target: String?): List<String> {
        val to = if (action == RuleAction.Server) target ?: return emptyList() else action.mihomo
        fun rule(kind: RuleKind, v: String) = Rule(kind, v, action, server).mihomo(target)
        return domains.mapNotNull { rule(RuleKind.Domain, it) } +
            apps.mapNotNull { rule(RuleKind.App, it) } +
            ips.mapNotNull { rule(RuleKind.Ip, it) } +
            geosite.map { "GEOSITE,$it,$to" }
    }
}

object RuleSets {
    private const val LINK = "rynet://set/"
    private const val MAX_BYTES = 512 * 1024
    private const val MAX_ITEMS = 20_000
    private const val MAX_NAME = 40
    private val GEOSITE = Regex("^[a-z0-9!@._-]+$")

    fun newId(): String = java.util.UUID.randomUUID().toString().take(8)

    fun tailscale(server: Selection) = RuleSet(
        newId(),
        name = "Tailscale",
        action = RuleAction.Server,
        server = server,
        domains = listOf("ts.net"),
        ips = listOf("100.64.0.0/10", "fd7a:115c:a1e0::/48")
    )

    fun toJson(sets: List<RuleSet>): JSONArray = JSONArray(
        sets.map { s ->
            body(s).put("id", s.id).put("enabled", s.enabled)
                .apply { s.server?.let { put("sub", it.sub).put("profile", it.profile) } }
        }
    )

    fun fromJson(a: JSONArray?): List<RuleSet> = (0 until (a?.length() ?: 0)).mapNotNull { i ->
        val o = a!!.optJSONObject(i) ?: return@mapNotNull null
        val server = o.optString("sub").takeIf { it.isNotEmpty() }?.let { sub -> o.optString("profile").takeIf { it.isNotEmpty() }?.let { Selection(sub, it) } }
        read(o)?.copy(
            id = o.optString("id").ifEmpty { newId() },
            enabled = o.optBoolean("enabled", true),
            server = server
        )?.let { if (it.action == RuleAction.Server && it.server == null) it.copy(action = RuleAction.Proxy) else it }
    }

    fun link(s: RuleSet): String {
        val raw = body(s).put("v", 1).toString().toByteArray()
        val out = ByteArrayOutputStream()
        val d = Deflater(Deflater.BEST_COMPRESSION, true)
        d.setInput(raw)
        d.finish()
        val buf = ByteArray(8192)
        while (!d.finished()) out.write(buf, 0, d.deflate(buf))
        d.end()
        return LINK + Base64.encodeToString(out.toByteArray(), Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
    }

    fun isLink(text: String): Boolean = text.trim().startsWith(LINK, ignoreCase = true)

    private const val PART = "${LINK}p/"
    private val PART_RE = Regex("""^rynet://set/p/([0-9a-z]{1,8})/(\d{1,3})/(\d{1,3})/([A-Za-z0-9_-]+)$""", RegexOption.IGNORE_CASE)

    fun parts(link: String, size: Int = 400): List<String> {
        val body = link.trim().removePrefix(LINK)
        val chunks = body.chunked(size)
        val tag = (body.hashCode().toLong() and 0xffffffffL).toString(36)
        return chunks.mapIndexed { i, c -> "$PART$tag/${i + 1}/${chunks.size}/$c" }
    }

    fun isPart(text: String): Boolean = PART_RE.matches(text.trim())

    class Assembler {
        private var tag: String? = null
        private val got = sortedMapOf<Int, String>()
        var total = 0
            private set
        val count: Int get() = got.size

        fun add(text: String): String? {
            val m = PART_RE.matchEntire(text.trim()) ?: return null
            val (t, i, n, chunk) = m.destructured
            val index = i.toInt()
            val all = n.toInt()
            if (all !in 1..999 || index !in 1..all) return null
            if (t != tag || all != total) {
                tag = t
                total = all
                got.clear()
            }
            got[index] = chunk
            return if (got.size == total) LINK + got.values.joinToString("") else null
        }
    }

    fun parse(text: String): RuleSet? = runCatching {
        val t = text.trim()
        if (!isLink(t)) return null
        val packed = Base64.decode(t.substring(LINK.length).substringBefore('#').substringBefore('?'), Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
        val inf = Inflater(true)
        inf.setInput(packed)
        val out = ByteArrayOutputStream()
        val buf = ByteArray(8192)
        while (!inf.finished()) {
            val n = inf.inflate(buf)
            if (n == 0 && (inf.needsInput() || inf.needsDictionary())) break
            out.write(buf, 0, n)
            if (out.size() > MAX_BYTES) return null
        }
        inf.end()
        read(JSONObject(out.toString(Charsets.UTF_8.name())))?.let {
            it.copy(id = newId(), action = if (it.action == RuleAction.Server) RuleAction.Proxy else it.action)
        }
    }.getOrNull()

    private fun body(s: RuleSet): JSONObject = JSONObject()
        .put("name", s.name).put("shape", s.shape).put("icon", s.icon).put("action", s.action.key)
        .put("domains", JSONArray(s.domains)).put("apps", JSONArray(s.apps))
        .put("ips", JSONArray(s.ips)).put("geosite", JSONArray(s.geosite))

    private fun read(o: JSONObject): RuleSet? {
        fun list(key: String, norm: (String) -> String?): List<String> {
            val a = o.optJSONArray(key) ?: return emptyList()
            return (0 until minOf(a.length(), MAX_ITEMS)).mapNotNull { a.optString(it).let(norm) }.distinct()
        }
        return RuleSet(
            id = "",
            name = o.optString("name").trim().take(MAX_NAME),
            shape = o.optInt("shape").coerceAtLeast(0),
            icon = o.optInt("icon").coerceAtLeast(0),
            action = RuleAction.of(o.optString("action")) ?: RuleAction.Proxy,
            domains = list("domains") { Rules.domain(it) },
            apps = list("apps") { it.trim().takeIf(Rules::validPackage) },
            ips = list("ips") { v -> Rules.cidr(v)?.first },
            geosite = list("geosite") { geosite(it) }
        )
    }

    fun geosite(input: String): String? = input.trim().lowercase().removePrefix("geosite:").takeIf { GEOSITE.matches(it) }

    fun split(text: String): List<String> = text.split(Regex("[\\s,;]+")).filter { it.isNotEmpty() }
}
