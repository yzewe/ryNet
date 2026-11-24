package dev.yzewe.rynet.data

import android.content.Context
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.util.zip.GZIPInputStream
import java.util.zip.GZIPOutputStream
import org.json.JSONArray
import org.json.JSONObject

object Backup {
    enum class Part(val key: String, private val ruTitle: String, private val enTitle: String, val settingsKeys: List<String>) {
        Subs("subs", "Подписки и выбранный сервер", "Subscriptions and selected server", emptyList()),
        Routing("routing", "Маршрутизация и свои правила", "Routing and custom rules", listOf("routingMode", "rules", "sets", "rest", "userRules", "fakeDns")),
        Network("network", "Сеть: цепочка, фрагментация, MTU", "Network: chain, fragmentation, MTU", listOf("chain", "proxyMode", "proxyPort", "chainSub", "chainProfile", "fragment", "fragmentLength", "fragmentInterval", "mtu", "killSwitch", "background", "serverDns", "serverDnsCustom", "dnsServers", "perServerDns", "vpnAppsOnly")),
        Ping("ping", "Пинг и проверка сервера", "Ping and server health check", listOf("pingOnStart", "pingMode", "pingParallel", "pingTimeout", "pingUrl", "ipUrl", "healthCheck", "healthSec", "healthNotify", "autoSwitch", "healthIdle", "autoSelect", "autoFeature", "autoInterval", "reserveOnlyOnWhitelists")),
        SubSettings("subSettings", "Обновление подписок и HWID", "Subscription updates and HWID", listOf("autoUpdate", "sendHwid", "subClient", "subHeaders", "hwid", "refreshNotices")),
        App("app", "Внешний вид и приложение", "Appearance and app", listOf("theme", "wallpaperColors", "seedColor", "orbStyle", "sort", "view", "grid", "sectionRadius", "sectionDivider", "serverInfo", "exitFlags", "pins", "notifyDetails", "liveUpdate", "autostart", "wifiPause", "lockOff", "debugLog", "sendLogs"))
        ;

        val title: String get() = tr(ruTitle, enTitle)

        companion object {
            fun of(key: String) = entries.firstOrNull { it.key == key }
        }
    }

    class Snapshot(val parts: Map<Part, JSONObject>, val app: String, val created: Long) {
        fun summary(p: Part): String? = when (p) {
            Part.Subs -> parts[p]?.optJSONArray("list")?.length()?.let { n -> plural(n, "подписка", "подписки", "подписок", "subscription") }
            Part.Routing -> parts[p]?.let { o -> RoutingMode.of(o.optString("routingMode"))?.title }
            else -> null
        }
    }

    private val MAGIC = "RYNET".toByteArray()
    private const val VERSION: Byte = 1
    private const val MAX_UNPACKED = 32 shl 20
    private val SAFE_ID = Regex("[A-Za-z0-9-]{1,64}")
    private const val CUSTOM_NAME = "customName"

    fun fileName(): String = "ryNet-" + java.text.SimpleDateFormat("yyyy-MM-dd", java.util.Locale.ROOT).format(java.util.Date()) + ".rynet"

    fun available(ctx: Context): Map<Part, String?> = Part.entries.associateWith { p ->
        if (p == Part.Subs) Subs.list(ctx).size.let { n -> if (n == 0) null else plural(n, "подписка", "подписки", "подписок", "subscription") } else null
    }.filter { (p, _) -> p != Part.Subs || Subs.list(ctx).isNotEmpty() }

    fun export(ctx: Context, parts: Set<Part>, app: String): ByteArray {
        val settings = Settings.load(ctx).toJson()
        val out = JSONObject()
        for (p in parts) {
            out.put(p.key, if (p == Part.Subs) subsJson(ctx) else pick(settings, p.settingsKeys))
        }
        val json = JSONObject()
            .put("app", app)
            .put("created", System.currentTimeMillis())
            .put("parts", out)
            .toString()
        val bytes = ByteArrayOutputStream()
        bytes.write(MAGIC)
        bytes.write(VERSION.toInt())
        GZIPOutputStream(bytes).use { it.write(json.toByteArray()) }
        return bytes.toByteArray()
    }

    fun read(bytes: ByteArray): Snapshot {
        require(bytes.size > MAGIC.size + 1 && bytes.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) { tr("Это не файл настроек ryNet", "Not a ryNet settings file") }
        val version = bytes[MAGIC.size]
        require(version <= VERSION) { tr("Файл из более новой версии ryNet — обновите приложение", "File is from a newer ryNet version — update the app") }
        val json = runCatching {
            GZIPInputStream(ByteArrayInputStream(bytes, MAGIC.size + 1, bytes.size - MAGIC.size - 1)).use { z ->
                val out = ByteArrayOutputStream()
                val buf = ByteArray(64 * 1024)
                while (true) {
                    val n = z.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    require(out.size() <= MAX_UNPACKED) { tr("Файл настроек слишком большой", "Settings file is too large") }
                }
                out.toString(Charsets.UTF_8.name())
            }
        }.getOrElse { throw IllegalArgumentException(tr("Файл настроек повреждён", "Settings file is corrupted")) }
        val o = runCatching { JSONObject(json) }.getOrElse { throw IllegalArgumentException(tr("Файл настроек повреждён", "Settings file is corrupted")) }
        val parts = o.optJSONObject("parts") ?: JSONObject()
        val map = parts.keys().asSequence().mapNotNull { k -> Part.of(k)?.let { p -> parts.optJSONObject(k)?.let { p to it } } }.toMap()
        require(map.isNotEmpty()) { tr("В файле нет настроек", "No settings in the file") }
        return Snapshot(Part.entries.filter { it in map }.associateWith { map.getValue(it) }, o.optString("app"), o.optLong("created"))
    }

    fun apply(ctx: Context, snap: Snapshot, parts: Set<Part>): Pair<Settings, Settings> {
        val old = Settings.load(ctx)
        val merged = old.toJson()
        for (p in parts) {
            val src = snap.parts[p] ?: continue
            if (p == Part.Subs) {
                importSubs(ctx, src)
            } else {
                for (k in p.settingsKeys) if (src.has(k)) merged.put(k, src.get(k))
            }
        }
        return old to Settings.fromJson(merged)
    }

    private fun pick(o: JSONObject, keys: List<String>) = JSONObject().apply {
        keys.forEach { k -> if (o.has(k)) put(k, o.get(k)) }
    }

    private fun subsJson(ctx: Context): JSONObject {
        val list = JSONArray()
        for (s in Subs.list(ctx)) {
            val body = Subs.body(ctx, s.id) ?: continue
            val info = Subscription.toJson(s.stored).apply { s.name?.let { put(CUSTOM_NAME, it) } }
            list.put(JSONObject().put("id", s.id).put("info", info).put("body", body))
        }
        val o = JSONObject().put("list", list)
        Subs.selection(ctx)?.let { o.put("selection", JSONObject().put("sub", it.sub).put("profile", it.profile)) }
        return o
    }

    private fun importSubs(ctx: Context, o: JSONObject) {
        val list = o.optJSONArray("list") ?: return
        for (i in 0 until list.length()) {
            val s = list.optJSONObject(i) ?: continue
            val id = s.optString("id").takeIf { SAFE_ID.matches(it) } ?: java.util.UUID.randomUUID().toString()
            val info = s.optJSONObject("info") ?: continue
            val sub = runCatching { Sub(id, Subscription.fromJson(info)) }.getOrNull() ?: continue
            val body = s.optString("body").takeIf { it.isNotEmpty() } ?: continue
            Subs.import(ctx, sub, body)
            Subs.rename(ctx, id, info.optString(CUSTOM_NAME).takeIf { it.isNotBlank() })
        }
        o.optJSONObject("selection")?.let { sel ->
            val chosen = Selection(sel.optString("sub"), sel.optString("profile"))
            if (Subs.list(ctx).any { it.id == chosen.sub }) Subs.select(ctx, chosen)
        }
        val app = ctx.applicationContext
        Thread {
            runCatching {
                val all = Subs.list(app)
                all.forEach { s -> s.info.routing?.let { GeoDat.update(app, it) } }
                Geo.update(app, all.joinToString("\n") { Subs.body(app, it.id).orEmpty() + "\n" + it.info.routing?.let { r -> GeoDat.localize(app, r) }.orEmpty() } + "\n" + Settings.load(app).effectiveRules())
                all.filterNot(Local::isLocal).forEach { runCatching { SubIcon.fetch(app, it.id, it.info) } }
            }
        }.start()
    }
}
