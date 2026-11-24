package dev.yzewe.rynet.data

import android.content.Context
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.widget.ToggleWidget
import java.io.File
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

data class Sub(val id: String, val info: SubInfo, val name: String? = null, val panelTitle: String? = info.title) {
    val stored: SubInfo get() = if (name != null) info.copy(title = panelTitle) else info

    val title: String get() = info.title ?: info.url.substringAfter("://").substringBefore('/')
}

data class Selection(val sub: String, val profile: String)

fun hostOf(url: String): String = url.substringAfter("://").substringBefore('/').substringBefore('?')

object Subs {
    private const val INDEX = "subs.json"
    private const val SELECTION = "selection.json"
    private const val LAST = "last-servers.json"
    private const val NAMES = "sub-names.json"
    private const val SPLIT = "sub-split.json"

    private fun bodyFile(ctx: Context, id: String) = File(File(ctx.filesDir, "subs").apply { mkdirs() }, "$id.json")

    fun list(ctx: Context): List<Sub> {
        migrate(ctx)
        val arr = Store.read(ctx, INDEX)?.let { runCatching { JSONArray(it) }.getOrNull() } ?: return emptyList()
        val names = names(ctx)
        return (0 until arr.length()).mapNotNull { i ->
            val o = arr.getJSONObject(i)
            runCatching {
                val info = Subscription.fromJson(o.getJSONObject("info")).let { if (it.url == Local.URL) it.copy(title = Local.TITLE) else it }
                val id = o.getString("id")
                val name = names[id]?.takeIf { info.url != Local.URL }
                if (name != null) Sub(id, info.copy(title = name), name, info.title) else Sub(id, info)
            }.getOrNull()
        }
    }

    private fun save(ctx: Context, subs: List<Sub>) {
        val arr = JSONArray()
        subs.forEach { arr.put(JSONObject().put("id", it.id).put("info", Subscription.toJson(it.stored))) }
        Store.write(ctx, INDEX, arr.toString())
    }

    private fun names(ctx: Context): Map<String, String> = Store.read(ctx, NAMES)?.let { t ->
        runCatching { JSONObject(t).let { o -> o.keys().asSequence().associateWith(o::optString).filterValues(String::isNotBlank) } }.getOrNull()
    }.orEmpty()

    fun rename(ctx: Context, id: String, name: String?) {
        val all = names(ctx)
        val now = name?.trim()?.takeIf { it.isNotEmpty() }?.let { all + (id to it) } ?: (all - id)
        if (now == all) return
        if (now.isEmpty()) Store.delete(ctx, NAMES) else Store.write(ctx, NAMES, JSONObject(now).toString())
    }

    fun edit(ctx: Context, id: String, name: String?, link: String): Sub {
        val sub = list(ctx).firstOrNull { it.id == id } ?: throw IllegalStateException(tr("Подписка не найдена", "Subscription not found"))
        val url = withScheme(link)
        if (!sameUrl(url, sub.info.url) && !Local.isLocal(sub)) {
            existing(ctx, url)?.takeIf { it.id != id }?.let { throw IllegalStateException(tr("Эта подписка уже добавлена: ${it.info.title ?: "без названия"}", "This subscription is already added: ${it.info.title ?: "untitled"}")) }
            fetch(ctx, url, id)
        }
        rename(ctx, id, name)
        return list(ctx).first { it.id == id }
    }

    fun validUrl(link: String): Boolean = runCatching {
        val u = java.net.URI(withScheme(link))
        (u.scheme.equals("https", true) || u.scheme.equals("http", true)) && !u.host.isNullOrBlank()
    }.getOrDefault(false)

    fun body(ctx: Context, id: String): String? = bodyFile(ctx, id).takeIf { it.exists() }?.readText()

    fun profiles(ctx: Context, id: String): List<Profile> = body(ctx, id)?.let { runCatching { Subscription.parse(it) }.getOrNull() }?.let { if (split(ctx, id)) Subscription.split(it) else it }
        ?.let { Countries.flags(ctx, id, it) } ?: emptyList()

    private fun splitIds(ctx: Context): Set<String> = Store.read(ctx, SPLIT)?.let { t ->
        runCatching { JSONArray(t).let { a -> (0 until a.length()).map(a::getString).toSet() } }.getOrNull()
    }.orEmpty()

    fun split(ctx: Context, id: String): Boolean = id in splitIds(ctx)

    fun setSplit(ctx: Context, id: String, on: Boolean) {
        val all = splitIds(ctx)
        val now = if (on) all + id else all - id
        if (now == all) return
        if (now.isEmpty()) Store.delete(ctx, SPLIT) else Store.write(ctx, SPLIT, JSONArray(now.toList()).toString())
    }

    fun sameUrl(a: String, b: String): Boolean {
        fun norm(u: String): String {
            val t = u.trim().trimEnd('/')
            val i = t.indexOf("://")
            if (i < 0) return t
            val host = t.indexOf('/', i + 3).let { if (it < 0) t.length else it }
            return t.substring(0, host).lowercase() + t.substring(host)
        }
        return norm(a) == norm(b)
    }

    fun withScheme(link: String): String {
        val t = link.trim()
        return if (t.contains("://")) t else "https://" + t.removePrefix("//")
    }

    fun existing(ctx: Context, url: String): Sub? = list(ctx).firstOrNull { sameUrl(it.info.url, url) }

    fun fetch(ctx: Context, link: String, id: String? = null): Sub {
        if (link == Local.URL) return list(ctx).first { it.id == Local.ID }
        val url = withScheme(link)
        if (id == null) existing(ctx, url)?.let { throw IllegalStateException(tr("Эта подписка уже добавлена: ${it.info.title ?: "без названия"}", "This subscription is already added: ${it.info.title ?: "untitled"}")) }
        val (info, text) = Subscription.download(ctx, url)
        if (id == null) existing(ctx, info.url)?.let { throw IllegalStateException(tr("Эта подписка уже добавлена: ${it.info.title ?: "без названия"}", "This subscription is already added: ${it.info.title ?: "untitled"}")) }
        val sub = Sub(id ?: UUID.randomUUID().toString(), info)
        val tmp = File(bodyFile(ctx, sub.id).path + ".tmp")
        tmp.writeText(text)
        tmp.renameTo(bodyFile(ctx, sub.id))
        val all = list(ctx).filter { it.id != sub.id } + sub
        save(ctx, all.sortedBy { s -> list(ctx).indexOfFirst { it.id == s.id }.let { if (it < 0) Int.MAX_VALUE else it } })
        val extras = Extras.run.submit {
            runCatching { info.routing?.let { GeoDat.update(ctx, it) } }
            runCatching { Geo.update(ctx, text + "\n" + info.routing?.let { GeoDat.localize(ctx, it) }.orEmpty() + "\n" + Settings.load(ctx).effectiveRules()) }
            runCatching { if (id == null || !SubIcon.fresh(ctx, sub.id)) SubIcon.fetch(ctx, sub.id, info) }
        }
        if (id == null) runCatching { extras.get(EXTRAS_WAIT_MS, java.util.concurrent.TimeUnit.MILLISECONDS) }
        return sub
    }

    private const val EXTRAS_WAIT_MS = 20_000L

    private object Extras {
        val run: java.util.concurrent.ExecutorService =
            java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "rynet-sub-extras").apply { isDaemon = true } }
    }

    fun import(ctx: Context, sub: Sub, body: String) {
        val tmp = File(bodyFile(ctx, sub.id).path + ".tmp")
        tmp.writeText(body)
        tmp.renameTo(bodyFile(ctx, sub.id))
        val old = list(ctx)
        val same = old.filter { it.id == sub.id || it.info.url == sub.info.url }
        same.filter { it.id != sub.id }.forEach {
            bodyFile(ctx, it.id).delete()
            SubIcon.remove(ctx, it.id)
            rename(ctx, it.id, null)
        }
        val at = old.indexOfFirst { it in same }
        val rest = old.filter { it !in same }.toMutableList()
        rest.add(if (at < 0) rest.size else at.coerceAtMost(rest.size), sub)
        save(ctx, rest)
    }

    fun remove(ctx: Context, id: String) {
        save(ctx, list(ctx).filter { it.id != id })
        rename(ctx, id, null)
        bodyFile(ctx, id).delete()
        SubIcon.remove(ctx, id)
        Pings.forget(ctx, id)
        Countries.forget(ctx, id)
        if (selection(ctx)?.sub == id) Store.delete(ctx, SELECTION)
        lastAll(ctx).takeIf { it.has(id) }?.let { o ->
            o.remove(id)
            Store.write(ctx, LAST, o.toString())
        }
        Settings.pruneSubs(ctx)
    }

    fun selection(ctx: Context): Selection? = Store.read(ctx, SELECTION)?.let {
        runCatching { JSONObject(it).let { o -> Selection(o.getString("sub"), o.getString("profile")) } }.getOrNull()
    }

    private fun lastAll(ctx: Context): JSONObject = Store.read(ctx, LAST)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: JSONObject()

    fun last(ctx: Context, sub: String): String? = lastAll(ctx).optString(sub).takeIf { it.isNotEmpty() }

    fun select(ctx: Context, sel: Selection) {
        val prev = selection(ctx)
        Store.write(ctx, SELECTION, JSONObject().put("sub", sel.sub).put("profile", sel.profile).toString())
        val all = lastAll(ctx)
        val keepPrev = prev != null && prev.sub != sel.sub && !all.has(prev.sub)
        if (keepPrev) all.put(prev!!.sub, prev.profile)
        if (keepPrev || all.optString(sel.sub) != sel.profile) Store.write(ctx, LAST, all.put(sel.sub, sel.profile).toString())
        ToggleWidget.refresh(ctx)
    }

    private fun migrate(ctx: Context) {
        if (Store.read(ctx, INDEX) != null) return
        val body = Store.read(ctx, Store.SUB_BODY) ?: return
        val info = Store.read(ctx, Store.SUB_META)?.let { runCatching { Subscription.fromJson(JSONObject(it)) }.getOrNull() } ?: return
        val id = UUID.randomUUID().toString()
        bodyFile(ctx, id).writeText(body)
        save(ctx, listOf(Sub(id, info)))
        Store.read(ctx, Store.SELECTED)?.trim()?.takeIf { it.isNotEmpty() }?.let { select(ctx, Selection(id, it)) }
        Store.delete(ctx, Store.SUB_BODY)
        Store.delete(ctx, Store.SUB_META)
        Store.delete(ctx, Store.SELECTED)
    }
}
