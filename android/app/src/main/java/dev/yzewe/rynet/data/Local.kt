package dev.yzewe.rynet.data

import android.content.Context
import android.net.Uri
import dev.yzewe.rynet.tr
import java.util.Base64
import org.json.JSONArray
import org.json.JSONObject

object Local {
    const val ID = "local"
    const val URL = "rynet:local"
    val TITLE: String get() = tr("Свои серверы", "My servers")

    private val SCHEMES = setOf("vless", "vmess", "trojan", "ss", "hysteria2", "hy2", "hysteria", "tuic", "wireguard", "wg", "socks", "anytls")

    fun isLocal(sub: Sub) = sub.info.url == URL

    private fun scheme(line: String) = line.trim().substringBefore("://", "").lowercase()

    fun isServers(text: String): Boolean {
        val t = text.trim()
        if (t.startsWith("{") || t.startsWith("[")) return true
        val first = t.lineSequence().map { it.trim() }.firstOrNull { it.isNotEmpty() && !it.startsWith("#") } ?: return false
        if ("://" in first) return scheme(first) in SCHEMES
        return links(t).isNotEmpty()
    }

    private fun links(text: String): List<String> {
        val t = text.trim()
        val plain = if ("://" in t) {
            t
        } else {
            runCatching {
                val compact = t.filterNot { it.isWhitespace() }
                val bytes = runCatching { Base64.getDecoder().decode(compact) }.getOrElse { Base64.getUrlDecoder().decode(compact.trimEnd('=')) }
                String(bytes)
            }.getOrDefault("")
        }
        return plain.lineSequence().map { it.trim() }.filter { scheme(it) in SCHEMES }.toList()
    }

    private fun parseEntries(text: String): List<Any> {
        val t = text.trim()
        return when {
            t.startsWith("[") -> JSONArray(t).let { a -> (0 until a.length()).map { a.get(it) }.filter { it is String || it is JSONObject } }
            t.startsWith("{") -> listOf(JSONObject(t))
            else -> links(t)
        }
    }

    private fun single(entry: Any): Boolean = runCatching { Subscription.parse(JSONArray().put(entry).toString()).count { !it.placeholder } == 1 }.getOrDefault(false)

    private fun key(entry: Any): String = when (entry) {
        is JSONObject -> JSONObject(entry.toString()).apply { remove("remarks") }.toString()
        else -> entry.toString().substringBefore('#')
    }

    private fun entries(ctx: Context): MutableList<Any> = Subs.body(ctx, ID)?.let { b -> runCatching { JSONArray(b) }.getOrNull() }
        ?.let { a -> (0 until a.length()).map { a.get(it) }.toMutableList() } ?: mutableListOf()

    private fun save(ctx: Context, entries: List<Any>): Sub? {
        if (entries.isEmpty()) {
            Subs.remove(ctx, ID)
            return null
        }
        val info = SubInfo(URL, TITLE, 0, 0, 0, 0, System.currentTimeMillis(), null)
        val sub = Sub(ID, info)
        Subs.import(ctx, sub, JSONArray(entries).toString())
        return sub
    }

    fun preview(text: String): List<String> = runCatching { parseEntries(text) }.getOrDefault(emptyList()).mapNotNull { e ->
        runCatching { Subscription.parse(JSONArray().put(e).toString()).singleOrNull { !it.placeholder } }.getOrNull()
            ?.let { p -> listOfNotNull(p.name.ifBlank { p.remarks }.takeIf { it.isNotBlank() }, p.hosts.firstOrNull()).distinct().joinToString(" · ") }
    }

    fun add(ctx: Context, text: String): Pair<Sub, List<Int>> {
        val found = runCatching { parseEntries(text) }.getOrElse { error(tr("Конфиг не читается: ${it.message}", "Can't read the config: ${it.message}")) }.filter(::single)
        if (found.isEmpty()) error(tr("Не похоже на ссылку на сервер или конфиг", "Doesn't look like a server link or config"))
        val all = entries(ctx)
        val touched = found.map { e ->
            val at = all.indexOfFirst { key(it) == key(e) }
            if (at >= 0) at.also { all[it] = e } else all.size.also { all.add(e) }
        }
        val sub = save(ctx, all)!!
        Geo.update(ctx, JSONArray(all).toString() + "\n" + Settings.load(ctx).effectiveRules())
        return sub to touched.distinct()
    }

    fun rename(ctx: Context, index: Int, name: String) {
        val all = entries(ctx)
        val e = all.getOrNull(index) ?: return
        all[index] = when (e) {
            is JSONObject -> e.put("remarks", name)
            else -> renamed(e.toString(), name)
        }
        save(ctx, all)
    }

    fun remove(ctx: Context, index: Int) {
        val all = entries(ctx)
        if (index !in all.indices) return
        all.removeAt(index)
        save(ctx, all)
    }

    fun removeAll(ctx: Context, indices: Set<Int>) {
        val all = entries(ctx)
        save(ctx, all.filterIndexed { i, _ -> i !in indices })
    }

    internal fun renamed(link: String, name: String): String {
        if (link.startsWith("vmess://", true) && '@' !in link) {
            runCatching {
                val json = JSONObject(String(Base64.getDecoder().decode(link.substring(8).trim()))).put("ps", name)
                return "vmess://" + Base64.getEncoder().encodeToString(json.toString().toByteArray())
            }
        }
        val base = link.substringBefore('#')
        val tail = link.substringAfter('#', "").let { f -> f.indexOf("?serverDescription=").takeIf { it >= 0 }?.let { f.substring(it) } }.orEmpty()
        return "$base#${Uri.encode(name)}$tail"
    }
}
