package dev.yzewe.rynet.data

import android.content.Context
import android.net.Uri
import dev.yzewe.rynet.Lang
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.tr
import java.net.HttpURLConnection
import java.net.URL
import java.util.Base64
import org.json.JSONArray
import org.json.JSONObject

internal inline fun <T> withDeadline(conn: HttpURLConnection, ms: Long, block: (Deadline) -> T): T {
    val d = Deadline(System.nanoTime() + ms * 1_000_000)
    val timer = Deadline.timer.schedule({
        d.cut = true
        runCatching { conn.disconnect() }
    }, ms, java.util.concurrent.TimeUnit.MILLISECONDS)
    try {
        return block(d)
    } catch (e: java.io.IOException) {
        if (d.cut || d.passed()) throw d.late(e)
        throw e
    } finally {
        timer.cancel(false)
    }
}

internal class Deadline(private val until: Long) {
    @Volatile var cut = false

    fun passed() = System.nanoTime() - until > 0

    fun late(cause: Throwable? = null) = java.io.IOException(tr("Сервер не ответил вовремя", "The server did not answer in time"), cause)

    fun wrap(input: java.io.InputStream): java.io.InputStream = object : java.io.FilterInputStream(input) {
        override fun read(): Int {
            if (passed()) throw late()
            return super.read()
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (passed()) throw late()
            return super.read(b, off, len)
        }
    }

    companion object {
        val timer: java.util.concurrent.ScheduledExecutorService =
            java.util.concurrent.Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "rynet-http-deadline").apply { isDaemon = true } }
    }
}

data class SubInfo(
    val url: String,
    val title: String?,
    val download: Long,
    val upload: Long,
    val total: Long,
    val expire: Long,
    val updatedAt: Long,
    val updateIntervalHours: Int?,
    val announce: String? = null,
    val supportUrl: String? = null,
    val webPage: String? = null,
    val routing: String? = null
)

enum class Group(private val ruTitle: String, private val enTitle: String) {
    Auto("Автовыбор", "Auto select"),
    Main("Основные", "Main"),
    Reserve("Резерв", "Backup"),
    Games("Для игр", "Gaming"),
    Whitelist("Белые списки", "Whitelists"),
    Cascade("Каскады", "Cascades")
    ;

    val title: String get() = tr(ruTitle, enTitle)
}

data class Profile(
    val remarks: String,
    val name: String,
    val flag: String,
    val group: Group,
    val kind: String,
    val supported: Boolean,
    val hosts: List<String>,
    val description: String? = null,
    val placeholder: Boolean = false,
    val server: String? = null,
    val reason: String? = null,
    val link: String? = null,
    val sni: String? = null,
    val fingerprint: String? = null,
    val members: List<Profile> = emptyList()
) {
    val membersKey: String get() = remarks + MEMBER_SEP

    companion object {
        const val MEMBER_SEP = '\u001f'

        fun parentOf(remarks: String): String? = remarks.substringBefore(MEMBER_SEP, "").ifEmpty { null }
    }
}

fun List<Profile>.byRemarks(remarks: String?): Profile? = remarks?.let { r -> firstOrNull { it.remarks == r } ?: asSequence().flatMap { it.members }.firstOrNull { it.remarks == r } }

fun List<Profile>.pinnedBy(pinned: List<String>): List<Profile> = pinned.mapNotNull { r ->
    firstOrNull { it.remarks == r } ?: Profile.parentOf(r)?.let { parent ->
        firstOrNull { it.remarks == parent }?.let { b -> b.members.firstOrNull { it.remarks == r }?.let { m -> m.copy(name = "${b.name} [${m.name}]") } }
    }
}

data class Section(val title: String, val items: List<Profile>, val note: String? = null)

object Subscription {
    private const val HAPP_AGENT = "Happ/4.3.0"
    private const val MAX_BODY = 32 shl 20
    private const val FETCH_MS = 40_000L

    private fun readLimited(stream: java.io.InputStream, limit: Int): ByteArray = stream.use { input ->
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            if (out.size() > limit) error(tr("Подписка слишком большая", "Subscription is too large"))
        }
        out.toByteArray()
    }

    private fun appVersion(ctx: Context): String = runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull() ?: "0"

    fun download(ctx: Context, url: String): Pair<SubInfo, String> = try {
        try {
            download(ctx, url, null)
        } catch (e: Exception) {
            val retry = e is java.io.IOException || (e is HttpStatus && (e.code in RETRY_CODES || e.code in 500..599))
            if (!retry || !url.trim().startsWith("https://", ignoreCase = true)) throw e
            val direct = directNetwork(ctx) ?: throw e
            runCatching { download(ctx, url, direct) }.getOrElse { throw e }
        }
    } catch (e: Exception) {
        throw readable(e)
    }

    class NeedHwid : IllegalStateException(tr("Провайдер подписки требует HWID: включите «Отправлять HWID»", "The subscription provider requires HWID: turn on \"Send HWID\""))

    private fun readable(e: Exception): Exception = when {
        e is NeedHwid || e !is java.io.IOException && e !is HttpStatus -> e

        e is HttpStatus -> IllegalStateException(
            when (e.code) {
                401, 403 -> tr("Провайдер подписки не пустил: ссылка устарела или подписка закончилась", "The subscription provider refused: the link is outdated or the subscription has ended")
                404, 410 -> tr("Ссылка не найдена — возможно, устарела: возьмите новую у провайдера", "Link not found — it may be outdated: get a new one from the provider")
                429 -> tr("Слишком много запросов — попробуйте через минуту", "Too many requests — try again in a minute")
                in 500..599 -> tr("Сайт подписки сейчас не работает (ошибка ${e.code}) — попробуйте позже", "The subscription site is down right now (error ${e.code}) — try later")
                else -> e.message
            },
            e
        )

        e is java.net.UnknownHostException -> java.io.IOException(tr("Адрес подписки не найден — проверьте ссылку и интернет", "Subscription address not found — check the link and the internet"), e)

        e is java.net.SocketTimeoutException || e.cause is java.net.SocketTimeoutException || e.message == tr("Сервер не ответил вовремя", "The server did not answer in time") ->
            java.io.IOException(tr("Сайт подписки не ответил вовремя — возможно, он недоступен в этой сети", "The subscription site didn't answer in time — it may be unreachable on this network"), e)

        e is java.net.ConnectException || e is java.net.NoRouteToHostException ->
            java.io.IOException(tr("Сайт подписки не отвечает — возможно, он недоступен в этой сети", "The subscription site isn't responding — it may be unreachable on this network"), e)

        e is javax.net.ssl.SSLException -> java.io.IOException(tr("Не удалось защищённо подключиться к сайту подписки", "Couldn't connect securely to the subscription site"), e)

        else -> java.io.IOException(tr("Не удалось скачать подписку — проверьте интернет", "Couldn't download the subscription — check the internet"), e)
    }

    private val RETRY_CODES = setOf(403, 429, 451)

    private class HttpStatus(val code: Int) : IllegalStateException(tr("Сервер подписки ответил $code", "Subscription server responded with $code"))

    private fun directNetwork(ctx: Context): android.net.Network? {
        val cm = ctx.getSystemService(android.net.ConnectivityManager::class.java) ?: return null
        val active = cm.activeNetwork ?: return null
        if (cm.getNetworkCapabilities(active)?.hasTransport(android.net.NetworkCapabilities.TRANSPORT_VPN) != true) return null
        @Suppress("DEPRECATION")
        val physical = cm.allNetworks.mapNotNull { n ->
            cm.getNetworkCapabilities(n)?.takeIf {
                it.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_INTERNET) && !it.hasTransport(android.net.NetworkCapabilities.TRANSPORT_VPN)
            }?.let { n to it }
        }
        return (physical.firstOrNull { it.second.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_VALIDATED) } ?: physical.firstOrNull())?.first
    }

    private fun download(ctx: Context, url: String, network: android.net.Network?): Pair<SubInfo, String> {
        val client = Settings.load(ctx).subClient
        return try {
            fetch(ctx, url, if (client == SubClient.Happ) HAPP_AGENT else "ryNet/${appVersion(ctx)} (Android ${android.os.Build.VERSION.RELEASE})", network)
        } catch (e: OnlyStubs) {
            if (client != SubClient.RyNet) throw e.final()
            try {
                fetch(ctx, url, HAPP_AGENT, network)
            } catch (e2: OnlyStubs) {
                throw e2.final()
            }
        }
    }

    private class OnlyStubs(message: String, val hwid: Exception? = null) : Exception(message) {
        fun final(): Exception = hwid ?: IllegalStateException(message)
    }

    private fun hwidCleartext() = IllegalStateException(
        tr(
            "Провайдер подписки требует HWID, а ссылка без шифрования (http://): по такой ссылке HWID не отправляется. Нужна ссылка https://",
            "The subscription provider requires HWID, but the link is unencrypted (http://): HWID is not sent over such a link. An https:// link is needed"
        )
    )

    private fun fetch(ctx: Context, url: String, agent: String, network: android.net.Network? = null): Pair<SubInfo, String> {
        val u = URL(url.trim())
        val conn = (network?.openConnection(u) ?: u.openConnection()) as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 20_000
        conn.setRequestProperty("User-Agent", agent)
        conn.setRequestProperty("Accept", "application/json, text/plain, */*")
        Hwid.apply(ctx, conn)
        val withheld = Hwid.cleartext(u) && Settings.load(ctx).sendHwid
        fun hwidError(): Exception = if (withheld) hwidCleartext() else NeedHwid()
        try {
            return withDeadline(conn, FETCH_MS) { d ->
                val code = conn.responseCode
                val needHwid = conn.getHeaderField("x-hwid-not-supported") != null
                if (conn.getHeaderField("x-hwid-max-devices-reached") != null || (conn.getHeaderField("x-hwid-limit") != null && !needHwid && code != 200)) {
                    error(tr("Достигнут лимит устройств подписки: удалите лишнее устройство у провайдера (в боте или личном кабинете)", "Subscription device limit reached: remove an extra device with your provider (in the bot or account page)"))
                }
                if (code != 200) throw HttpStatus(code)
                val text = readLimited(d.wrap(conn.inputStream), MAX_BODY).decodeToString()
                val profiles = runCatching { parse(text) }.getOrElse { if (needHwid) throw hwidError() else throw it }
                if (profiles.isEmpty()) {
                    if (needHwid) throw hwidError()
                    error(tr("В подписке нет серверов", "The subscription has no servers"))
                }
                if (profiles.all { it.placeholder }) {
                    throw OnlyStubs(
                        tr("Подписка без серверов: ", "Subscription has no servers: ") + profiles.joinToString(" ") { it.remarks }.take(160),
                        hwid = if (needHwid) hwidError() else null
                    )
                }
                parseHeaders(url.trim(), conn, text) to text
            }
        } finally {
            conn.disconnect()
        }
    }

    private class Parsed(val length: Int, val hash: Int, val ru: Boolean, val profiles: List<Profile>)

    @Volatile private var lastParsed: Parsed? = null

    fun parse(text: String): List<Profile> {
        val ru = Lang.ru
        lastParsed?.takeIf { it.length == text.length && it.ru == ru && it.hash == text.hashCode() }?.let { return it.profiles }
        return parseNow(text).also { lastParsed = Parsed(text.length, text.hashCode(), ru, it) }
    }

    private fun parseNow(text: String): List<Profile> {
        Native.syncLanguage()
        val arr = runCatching { JSONArray(Native.describe(text)) }
            .getOrElse { e ->
                val m = e.message.orEmpty()
                error(
                    when {
                        "clash" in m -> tr("Подписка в формате Clash — он не поддерживается", "Clash subscriptions are not supported")
                        else -> tr("Не похоже на подписку: ни конфигов xray, ни ссылок", "Doesn't look like a subscription: no xray configs or links")
                    }
                )
            }
        return (0 until arr.length()).map { profile(arr.getJSONObject(it)) }
    }

    private fun profile(o: JSONObject): Profile {
        val remarks = o.optString("remarks")
        val (flag, full) = splitFlag(remarks)
        val name = full.replace(Regex("\\s*\\[(Резерв|Для игр|Каскад)]"), "").trim().ifEmpty { remarks.trim() }
        val group = when {
            remarks.contains("Автовыбор") -> Group.Auto
            remarks.contains("Для игр") -> Group.Games
            remarks.contains("Белые списки") -> Group.Whitelist
            remarks.contains("Каскад") -> Group.Cascade
            remarks.contains("Резерв") -> Group.Reserve
            else -> Group.Main
        }
        val hosts = o.optJSONArray("hosts")?.let { a -> (0 until a.length()).map { a.getString(it) } } ?: emptyList()
        return Profile(
            remarks = remarks,
            name = name,
            flag = flag,
            group = group,
            kind = o.optString("kind"),
            supported = o.optBoolean("supported"),
            hosts = hosts,
            description = o.optString("description").takeIf { !o.isNull("description") && it.isNotBlank() },
            placeholder = o.optBoolean("placeholder"),
            server = o.optString("server").takeIf { !o.isNull("server") && it.isNotBlank() },
            reason = o.optString("reason").takeIf { !o.isNull("reason") && it.isNotBlank() },
            link = o.optString("link").takeIf { !o.isNull("link") && it.isNotBlank() },
            sni = o.optString("sni").takeIf { !o.isNull("sni") && it.isNotBlank() },
            fingerprint = o.optString("fp").takeIf { !o.isNull("fp") && it.isNotBlank() }
        ).let { p -> p.copy(members = members(o, p)) }
    }

    private fun members(o: JSONObject, parent: Profile): List<Profile> {
        val a = o.optJSONArray("members") ?: return emptyList()
        return (0 until a.length()).map { i ->
            val m = a.getJSONObject(i)
            val tag = m.optString("tag")
            val (flag, name) = splitFlag(tag)
            Profile(
                remarks = m.optString("name"),
                name = name.ifEmpty { tag },
                flag = flag.ifEmpty { parent.flag },
                group = parent.group,
                kind = m.optString("kind"),
                supported = m.optBoolean("supported"),
                hosts = parent.hosts,
                server = m.optString("server").takeIf { !m.isNull("server") && it.isNotBlank() },
                reason = m.optString("reason").takeIf { !m.isNull("reason") && it.isNotBlank() },
                link = m.optString("link").takeIf { !m.isNull("link") && it.isNotBlank() },
                sni = m.optString("sni").takeIf { !m.isNull("sni") && it.isNotBlank() },
                fingerprint = m.optString("fp").takeIf { !m.isNull("fp") && it.isNotBlank() }
            )
        }
    }

    fun split(profiles: List<Profile>): List<Profile> = profiles.flatMap { p ->
        if (p.members.isEmpty()) listOf(p) else p.members.map { m -> m.copy(name = "${p.name} [${m.name}]") }
    }

    fun sections(profiles: List<Profile>): List<Section> {
        if (profiles.none { it.placeholder }) {
            return profiles.groupBy { it.group }.toSortedMap(compareBy { it.ordinal }).entries
                .mapIndexed { i, (g, items) -> Section(if (i == 0 && items.size == 1 && items[0].name == g.title) "" else g.title, items) }
        }
        val out = mutableListOf<Section>()
        var head: Profile? = null
        var items = mutableListOf<Profile>()
        fun flush() {
            if (items.isNotEmpty() || head != null) out += Section(head?.let { cleanTitle(it.remarks) }.orEmpty(), items, head?.description)
        }
        for (p in profiles) {
            if (p.placeholder) {
                flush()
                head = p
                items = mutableListOf()
            } else {
                items += p
            }
        }
        flush()
        return out
    }

    private fun cleanTitle(s: String): String {
        val (_, rest) = splitFlag(s.trim())
        return rest.trim().trim('━', '─', '—', '-', '=', '⬇', '️', '↓', ' ').trim()
    }

    fun splitName(s: String): Pair<String, String> = splitFlag(s)

    private fun splitFlag(s: String): Pair<String, String> {
        val t = s.trim()
        val cps = t.codePoints().toArray()
        if (cps.size >= 2 && cps[0] in 0x1F1E6..0x1F1FF && cps[1] in 0x1F1E6..0x1F1FF) {
            val flag = String(cps, 0, 2)
            return flag to String(cps, 2, cps.size - 2).trim()
        }
        return "" to t
    }

    private fun b64(t: String?): String? = t?.let {
        if (it.startsWith("base64:")) {
            decodeB64(it.removePrefix("base64:").trim())
        } else {
            it
        }
    }?.trim()?.ifEmpty { null }

    private fun decodeB64(s: String): String? {
        val t = s.replace('-', '+').replace('_', '/').trimEnd('=')
        return runCatching { String(Base64.getDecoder().decode(t + "=".repeat((4 - t.length % 4) % 4))) }.getOrNull()
    }

    private fun bodyHeaders(text: String): Map<String, String> = text.lineSequence()
        .map { it.trim() }
        .takeWhile { it.isEmpty() || it.startsWith("#") }
        .mapNotNull { l -> l.removePrefix("#").split(':', limit = 2).takeIf { it.size == 2 }?.let { it[0].trim().lowercase() to it[1].trim() } }
        .toMap()

    internal fun happRouting(link: String?): String? {
        val t = link?.trim()?.takeIf { it.isNotEmpty() } ?: return null
        val at = t.indexOf("://routing/", ignoreCase = true)
        val data = if (at < 0) {
            t
        } else {
            val rest = t.substring(at + 11)
            if (rest.substringBefore('/').substringBefore('?').lowercase() !in setOf("add", "onadd")) return null
            rest.substringAfter("?data=", "").ifEmpty { rest.substringAfter('/', "") }.let { Uri.decode(it) }
        }
        return decodeB64(data.trim())?.let { runCatching { JSONObject(it).toString() }.getOrNull() }
    }

    private val ROUTING_LINE = Regex("^[a-z0-9+.-]*://routing/", RegexOption.IGNORE_CASE)

    internal fun migrated(url: String, newUrl: String?, newDomain: String?): String {
        newUrl?.trim()?.takeIf { it.startsWith("https://") || (it.startsWith("http://") && !url.trim().startsWith("https://", ignoreCase = true)) }?.let { return it }
        val d = newDomain?.trim()?.removePrefix("https://")?.removePrefix("http://")?.trimEnd('/')
            ?.takeIf { Regex("^[A-Za-z0-9.-]+(:[0-9]{1,5})?$").matches(it) } ?: return url
        return runCatching {
            val u = java.net.URI(url)
            val host = d.substringBefore(':')
            val port = d.substringAfter(':', "").toIntOrNull() ?: -1
            java.net.URI(u.scheme, u.rawUserInfo, host, port, u.rawPath, u.rawQuery, u.rawFragment).toString()
        }.getOrDefault(url)
    }

    private fun parseHeaders(url: String, conn: HttpURLConnection, text: String): SubInfo {
        val plain = if (text.contains("://") || text.trimStart().startsWith("#")) {
            text
        } else {
            decodeB64(text.filterNot { it.isWhitespace() }) ?: text
        }
        val inBody = bodyHeaders(plain)
        fun h(name: String): String? = conn.getHeaderField(name) ?: inBody[name]
        val routingLink = h("routing") ?: plain.lineSequence().map { it.trim() }.firstOrNull { ROUTING_LINE.containsMatchIn(it) }
        val ui = h("subscription-userinfo").orEmpty().split(';')
            .mapNotNull { kv -> kv.split('=', limit = 2).takeIf { it.size == 2 }?.let { it[0].trim() to (it[1].trim().toLongOrNull() ?: 0L) } }
            .toMap()
        return SubInfo(
            url = migrated(url, h("new-url"), h("new-domain")),
            title = b64(h("profile-title")),
            download = ui["download"] ?: 0,
            upload = ui["upload"] ?: 0,
            total = ui["total"] ?: 0,
            expire = ui["expire"] ?: 0,
            updatedAt = System.currentTimeMillis(),
            updateIntervalHours = h("profile-update-interval")?.trim()?.toIntOrNull(),
            announce = b64(h("announce")),
            supportUrl = h("support-url")?.trim()?.takeIf { it.startsWith("https://") || it.startsWith("http://") },
            webPage = h("profile-web-page-url")?.trim()?.takeIf { it.startsWith("https://") || it.startsWith("http://") },
            routing = happRouting(routingLink)
        )
    }

    fun toJson(i: SubInfo): JSONObject = JSONObject()
        .put("url", i.url).put("title", i.title).put("download", i.download).put("upload", i.upload)
        .put("total", i.total).put("expire", i.expire).put("updatedAt", i.updatedAt)
        .put("updateIntervalHours", i.updateIntervalHours).put("announce", i.announce).put("supportUrl", i.supportUrl)
        .put("webPage", i.webPage).put("routing", i.routing)

    fun fromJson(o: JSONObject) = SubInfo(
        url = o.getString("url"),
        title = o.optString("title").ifEmpty { null },
        download = o.optLong("download"),
        upload = o.optLong("upload"),
        total = o.optLong("total"),
        expire = o.optLong("expire"),
        updatedAt = o.optLong("updatedAt"),
        updateIntervalHours = if (o.has("updateIntervalHours") && !o.isNull("updateIntervalHours")) o.optInt("updateIntervalHours") else null,
        announce = o.optString("announce").takeIf { !o.isNull("announce") && it.isNotBlank() },
        supportUrl = o.optString("supportUrl").takeIf { !o.isNull("supportUrl") && it.isNotBlank() },
        webPage = o.optString("webPage").takeIf { !o.isNull("webPage") && it.isNotBlank() },
        routing = o.optString("routing").takeIf { !o.isNull("routing") && it.isNotBlank() }
    )
}
