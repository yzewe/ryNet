package dev.yzewe.rynet.data

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.SystemClock
import dev.yzewe.rynet.core.CaRoots
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.core.PingSink
import dev.yzewe.rynet.tr
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import org.json.JSONObject

enum class WhitelistMode(val key: String, private val ruTitle: String, private val enTitle: String) {
    Auto("auto", "Авто", "Auto"),
    Always("always", "Всегда", "Always"),
    Never("never", "Никогда", "Never")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class NetMode(val key: String) {
    Free("free"),
    Whitelist("whitelist"),
    Offline("offline");

    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

data class Probe(val host: String, val ms: Int, val home: Boolean) {
    val ok get() = ms >= 0
}

data class Candidate(
    val sub: String,
    val profile: String,
    val name: String,
    val flag: String,
    val subTitle: String,
    val ms: Int,
    val score: Int,
    val whitelist: Boolean,
    val learned: Boolean,
    val pool: Boolean
) {
    val ok get() = ms >= 0
    val selection get() = Selection(sub, profile)
}

data class AutoState(
    val at: Long = 0,
    val mode: NetMode = NetMode.Free,
    val network: String = "",
    val probes: List<Probe> = emptyList(),
    val candidates: List<Candidate> = emptyList(),
    val total: Int = 0,
    val chosen: Selection? = null,
    val next: Long = 0,
    val running: Boolean = false
) {
    fun json(): JSONObject = JSONObject()
        .put("at", at).put("mode", mode.key).put("network", network).put("total", total).put("next", next).put("running", running)
        .put("probes", JSONArray(probes.map { JSONObject().put("host", it.host).put("ms", it.ms).put("home", it.home) }))
        .put(
            "candidates",
            JSONArray(
                candidates.map {
                    JSONObject().put("sub", it.sub).put("profile", it.profile).put("name", it.name).put("flag", it.flag).put("subTitle", it.subTitle)
                        .put("ms", it.ms).put("score", it.score).put("whitelist", it.whitelist).put("learned", it.learned).put("pool", it.pool)
                }
            )
        )
        .apply { chosen?.let { put("chosen", JSONObject().put("sub", it.sub).put("profile", it.profile)) } }

    companion object {
        fun of(o: JSONObject): AutoState {
            fun <T> arr(k: String, f: (JSONObject) -> T): List<T> = o.optJSONArray(k)?.let { a -> (0 until a.length()).map { f(a.getJSONObject(it)) } }.orEmpty()
            return AutoState(
                at = o.optLong("at"),
                mode = NetMode.of(o.optString("mode")) ?: NetMode.Free,
                network = o.optString("network"),
                probes = arr("probes") { Probe(it.optString("host"), it.optInt("ms", -1), it.optBoolean("home")) },
                candidates = arr("candidates") {
                    Candidate(
                        it.optString("sub"), it.optString("profile"), it.optString("name"), it.optString("flag"), it.optString("subTitle"),
                        it.optInt("ms", -1), it.optInt("score"), it.optBoolean("whitelist"), it.optBoolean("learned"), it.optBoolean("pool", true)
                    )
                },
                total = o.optInt("total"),
                chosen = o.optJSONObject("chosen")?.let { Selection(it.optString("sub"), it.optString("profile")) },
                next = o.optLong("next"),
                running = o.optBoolean("running")
            )
        }
    }
}

object AutoSelect {
    const val ACTION = "dev.yzewe.rynet.AUTO"
    private const val STATE = "auto-state.json"
    private const val LEARNED = "auto-learned2.json"
    private const val WHITELIST_PENALTY = 100_000
    private const val PROBE_MS = 3500
    private const val RACE_MS = 700L
    private const val SWITCH_MIN_MS = 80
    private const val SWITCH_SHARE = 30
    private const val FREE_BYTES = 32 * 1024
    private const val FREE_MS = 1500L
    private const val CARRY_MS = 3000
    private const val CARRY_URL = "https://www.google.com/#body=$FREE_BYTES&within=$CARRY_MS"
    private const val CARRY_ALT = "${Endpoints.SPEED_DOWN}$FREE_BYTES#body=$FREE_BYTES&within=$CARRY_MS"
    private val SLOW = listOf("Слишком медленно", "Too slow")
    private const val LOSS_MS = 150
    private const val PIN_MS = 60
    private const val LEARNED_KEEP = 256

    fun load(ctx: Context): AutoState? = Store.read(ctx, STATE)?.let { runCatching { AutoState.of(JSONObject(it)) }.getOrNull() }

    fun publish(ctx: Context, s: AutoState) {
        Store.write(ctx, STATE, s.json().toString())
        ctx.sendBroadcast(android.content.Intent(ACTION).setPackage(ctx.packageName))
    }

    fun hosts(text: String): List<String> = text.split(Regex("[\\s,;]+")).map { it.trim() }.filter { it.isNotEmpty() }

    private fun url(host: String): String = when {
        "://" in host -> host
        host == "google.com" || host == "www.google.com" -> "https://www.google.com/"
        host == "cloudflare.com" || host == "www.cloudflare.com" -> "${Endpoints.SPEED_DOWN}$FREE_BYTES"
        else -> "https://$host/"
    }

    private fun probeOne(network: Network?, host: String, timeout: Int, home: Boolean): Int = runCatching {
        val t0 = SystemClock.elapsedRealtime()
        val u = URL(url(host))
        val c = (network?.openConnection(u) ?: u.openConnection()) as HttpURLConnection
        try {
            c.connectTimeout = timeout
            c.readTimeout = timeout
            c.instanceFollowRedirects = false
            c.useCaches = false
            c.setRequestProperty("User-Agent", "Mozilla/5.0")
            c.setRequestProperty("Accept-Encoding", "identity")
            val code = c.responseCode
            val ms = (SystemClock.elapsedRealtime() - t0).toInt().coerceAtLeast(1)
            if (!home) {
                val t1 = SystemClock.elapsedRealtime()
                c.readTimeout = FREE_MS.toInt()
                val body = if (code >= 400) c.errorStream else c.inputStream
                var got = 0
                body?.use { input ->
                    val buf = ByteArray(8192)
                    while (got < FREE_BYTES && SystemClock.elapsedRealtime() - t1 <= FREE_MS) {
                        val n = input.read(buf)
                        if (n < 0) break
                        got += n
                    }
                }
                if (got < FREE_BYTES && c.contentLengthLong.let { it < 0 || got < it }) return@runCatching -1
                if (SystemClock.elapsedRealtime() - t1 > FREE_MS) return@runCatching -1
            }
            ms
        } finally {
            c.disconnect()
        }
    }.getOrDefault(-1)

    fun probe(network: Network?, home: List<String>, free: List<String>, timeout: Int = PROBE_MS, early: Boolean = false): List<Probe> {
        val all = home.map { it to true } + free.map { it to false }
        if (all.isEmpty()) return emptyList()
        val pool = Executors.newFixedThreadPool(all.size.coerceAtMost(8))
        return try {
            val done = java.util.concurrent.ExecutorCompletionService<Probe>(pool)
            all.forEach { (h, isHome) -> done.submit { Probe(h, probeOne(network, h, timeout, isHome), isHome) } }
            val got = ArrayList<Probe>()
            val until = SystemClock.elapsedRealtime() + timeout * 2L + 1000
            while (got.size < all.size) {
                val p = runCatching { done.poll((until - SystemClock.elapsedRealtime()).coerceAtLeast(0), TimeUnit.MILLISECONDS)?.get() }.getOrNull() ?: break
                got += p
                if (early && !p.home && p.ok) return all.mapNotNull { (h, _) -> got.firstOrNull { it.host == h } }
            }
            all.map { (h, isHome) -> got.firstOrNull { it.host == h } ?: Probe(h, -1, isHome) }
        } finally {
            pool.shutdownNow()
        }
    }

    fun classify(probes: List<Probe>): NetMode {
        val home = probes.filter { it.home }
        val free = probes.filter { !it.home }
        return when {
            free.any { it.ok } -> NetMode.Free
            home.any { it.ok } -> NetMode.Whitelist
            free.isEmpty() && home.isEmpty() -> NetMode.Free
            else -> NetMode.Offline
        }
    }

    fun network(ctx: Context, net: Network?): String {
        val cm = ctx.getSystemService(ConnectivityManager::class.java)
        val caps = net?.let { runCatching { cm.getNetworkCapabilities(it) }.getOrNull() } ?: return ""
        return when {
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "Wi-Fi"

            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> {
                val op = runCatching { ctx.getSystemService(android.telephony.TelephonyManager::class.java).networkOperatorName }.getOrNull().orEmpty()
                listOf(tr("мобильная", "mobile"), op).filter { it.isNotBlank() }.joinToString(", ")
            }

            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "Ethernet"

            else -> ""
        }
    }

    private fun metered(ctx: Context, net: Network?): Boolean {
        val cm = ctx.getSystemService(ConnectivityManager::class.java)
        return net?.let { runCatching { cm.getNetworkCapabilities(it) }.getOrNull() }?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == false
    }

    fun sig(p: Profile): String = (p.server ?: p.remarks) + "|" + (p.sni ?: "")

    class Learned(val yes: Set<String>, val no: Set<String>)

    fun learned(ctx: Context): Learned {
        val o = Store.read(ctx, LEARNED)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: return Learned(emptySet(), emptySet())
        fun set(k: String) = o.optJSONArray(k)?.let { a -> (0 until a.length()).map(a::optString).toSet() }.orEmpty()
        return Learned(set("yes"), set("no"))
    }

    private fun learn(ctx: Context, passed: List<String>, failed: List<String>, squeezed: List<String>) {
        if (passed.isEmpty() && squeezed.isEmpty()) return
        val old = learned(ctx)
        val yes = (old.yes.filter { it !in passed && it !in squeezed } + passed).takeLast(LEARNED_KEEP)
        val no = (old.no.filter { it !in passed && it !in failed && it !in squeezed } + failed.filter { it !in old.yes } + squeezed).takeLast(LEARNED_KEEP)
        Store.write(ctx, LEARNED, JSONObject().put("yes", JSONArray(yes)).put("no", JSONArray(no)).toString())
    }

    private class Entry(
        val sub: Sub,
        val body: String,
        val p: Profile,
        val yes: Boolean,
        val no: Boolean,
        val pinned: Boolean,
        val prior: Int?,
        val mark: Boolean? = null,
        val named: Boolean = false
    ) {
        val key get() = key(sub.id, p.remarks)
        val sel get() = Selection(sub.id, p.remarks)
        val udp get() = isUdp(p)
        val whitelist get() = mark ?: (yes || named)
        val reserve get() = mark == true
        val bypass get() = whitelist || yes
    }

    private val WHITELIST_NAME =
        Regex("""(?iu)(?<![\p{L}\p{N}])(lte|бс|б/с|wl|whitelists?|бел\p{L}*\s+спис\p{L}*|мобильн\p{L}*)(?![\p{L}\p{N}])""")

    internal fun namedForWhitelists(remarks: String, subTitle: String? = null): Boolean = WHITELIST_NAME.containsMatchIn(remarks) || subTitle?.let(WHITELIST_NAME::containsMatchIn) == true

    internal fun reserve(s: Settings, sub: String, p: Profile): Boolean = s.autoWlMarked && (s.isAutoWl(sub, p.remarks) || Profile.parentOf(p.remarks)?.let { s.isAutoWl(sub, it) } == true)

    internal fun heldBack(s: Settings, mode: NetMode, reserve: Boolean) = reserve && mode != NetMode.Whitelist

    private fun key(sub: String, remarks: String) = sub + Profile.MEMBER_SEP + remarks

    private val UDP = Regex("""(?i)hysteria|tuic|wireguard|\bquic\b""")

    fun isUdp(p: Profile): Boolean = UDP.containsMatchIn(p.kind)

    val TRANSPORTS = listOf("TCP", "XHTTP", "gRPC", "WebSocket", "HTTPUpgrade", "Hysteria")

    fun transport(p: Profile): String = TRANSPORTS.drop(1).firstOrNull { p.kind.contains(it, ignoreCase = true) } ?: "TCP"

    private fun entries(ctx: Context, s: Settings): List<Entry> {
        val learned = learned(ctx)
        return Subs.list(ctx).filter { it.id !in s.autoSkipSubs }.flatMap { sub ->
            val body = Subs.body(ctx, sub.id) ?: return@flatMap emptyList()
            val skip = s.autoSkip[sub.id].orEmpty().toSet()
            val pinned = s.pinned(sub.id).toSet()
            val prior = Pings.forSub(ctx, sub.id).results
            fun marked(r: String) = s.isAutoWl(sub.id, r) || Profile.parentOf(r)?.let { s.isAutoWl(sub.id, it) } == true
            runCatching { Subscription.parse(body) }.getOrDefault(emptyList())
                .let(::pickable).filterNot { skip.covers(it.remarks) }
                .map {
                    Entry(
                        sub, body, it, sig(it) in learned.yes, sig(it) in learned.no, pinned.covers(it.remarks), prior[it.remarks]?.takeIf { v -> v >= 0 },
                        if (s.autoWlMarked) marked(it.remarks) else null, namedForWhitelists(it.remarks, sub.info.title)
                    )
                }
        }
    }

    private fun Set<String>.covers(remarks: String) = remarks in this || Profile.parentOf(remarks)?.let { it in this } == true

    fun passed(ctx: Context): Set<String> = learned(ctx).yes

    private fun pickable(list: List<Profile>) = list.flatMap { p -> p.members.ifEmpty { listOf(p) } }.filter { it.supported && !it.placeholder }

    fun servers(ctx: Context): List<Pair<Sub, List<Profile>>> = Subs.list(ctx).map { sub ->
        sub to pickable(Subs.profiles(ctx, sub.id))
    }.filter { it.second.isNotEmpty() }

    fun withMembers(s: Settings, groups: List<Pair<Sub, List<Profile>>>): Settings {
        fun spread(sub: String, list: List<String>): List<String> {
            val servers = groups.firstOrNull { it.first.id == sub }?.second.orEmpty()
            return list.flatMap { r -> servers.filter { Profile.parentOf(it.remarks) == r }.map { it.remarks }.ifEmpty { listOf(r) } }.distinct()
        }
        return s.copy(autoSkip = s.autoSkip.mapValues { (sub, list) -> spread(sub, list) }, autoWl = s.autoWl.mapValues { (sub, list) -> spread(sub, list) })
    }

    class Run(
        val network: Network?,
        val current: Selection?,
        val failed: Boolean = false,
        val quick: Boolean = false,
        val light: Boolean = false,
        val service: VpnService? = null,
        val prepare: (List<String>) -> Unit = {},
        val alive: () -> Boolean = { true }
    )

    private fun ping(ctx: Context, s: Settings, run: Run, list: List<Entry>, timeout: Int): Map<String, Int> = viaServers(ctx, s, run, list, timeout, s.pingUrl, lenient = false, race = run.quick) { v -> (v as? Number)?.toInt()?.let { if (it == 0) 1 else it } ?: -1 }

    private fun <T> viaServers(
        ctx: Context,
        s: Settings,
        run: Run,
        list: List<Entry>,
        timeout: Int,
        url: String,
        lenient: Boolean,
        race: Boolean = false,
        read: (Any?) -> T
    ): Map<String, T> {
        if (list.isEmpty() || !run.alive()) return emptyMap()
        runCatching { run.prepare(list.flatMap { it.p.hosts }.distinct()) }
        CaRoots.ensure()
        val tune = Tune.json(ctx, s, chain = false, exit = null, servers = false).toString()
        val bySub = list.groupBy { it.sub.id }
        val pool = Executors.newFixedThreadPool(bySub.size.coerceIn(1, 4))
        val live = java.util.concurrent.ConcurrentHashMap<String, Any>()
        val firstOk = java.util.concurrent.atomic.AtomicLong(0)
        return try {
            val futures = bySub.values.map { group ->
                pool.submit<Map<String, T>> {
                    val names = group.map { it.p.remarks }
                    val keys = group.associate { it.p.remarks to it.key }
                    val sink = if (!race) {
                        null
                    } else {
                        PingSink { name, ms, error ->
                            val k = keys[name] ?: return@PingSink
                            live[k] = if (ms >= 0) ms else error.orEmpty()
                            if (ms >= 0) firstOk.compareAndSet(0, SystemClock.elapsedRealtime())
                        }
                    }
                    val json = runCatching {
                        JSONObject(
                            Native.ping(
                                run.service, group.first().body, JSONArray(names).toString(), PingMode.Proxy.code,
                                names.size.coerceIn(1, s.pingParallel.coerceAtLeast(1)), timeout, sink, url, tune
                            )
                        )
                    }.getOrNull()
                    if (json == null && lenient) return@submit emptyMap()
                    group.associate { e -> e.key to read(json?.opt(e.p.remarks)) }
                }
            }
            if (race) {
                val until = SystemClock.elapsedRealtime() + timeout * 4L + 10_000
                while (!futures.all { it.isDone } && SystemClock.elapsedRealtime() < until && run.alive()) {
                    val ok = firstOk.get()
                    if (ok != 0L && SystemClock.elapsedRealtime() - ok >= RACE_MS) {
                        return list.mapNotNull { e -> live[e.key]?.let { e.key to read(it) } }.toMap()
                    }
                    Thread.sleep(25)
                }
            }
            futures.flatMap { f -> runCatching { f.get(timeout * 4L + 10_000, TimeUnit.MILLISECONDS) }.getOrNull()?.entries.orEmpty() }
                .associate { it.key to it.value }
        } finally {
            pool.shutdownNow()
        }
    }

    internal enum class Carry { Ok, Slow, Unknown }

    internal fun carryOf(v: Any?): Carry = when {
        v is Number && v.toInt() >= 0 -> Carry.Ok
        v is String && SLOW.any { v.startsWith(it) } -> Carry.Slow
        else -> Carry.Unknown
    }

    private fun carry(ctx: Context, s: Settings, run: Run, list: List<Entry>, timeout: Int): Map<String, Carry> {
        val t = timeout.coerceAtLeast(CARRY_MS + 2000)
        val first = viaServers(ctx, s, run, list, t, CARRY_URL, lenient = true, read = ::carryOf)
        val again = list.filter { first[it.key] == null || first[it.key] == Carry.Unknown }
        val second = viaServers(ctx, s, run, again, t, CARRY_ALT, lenient = true, read = ::carryOf)
        return (first + second).filterValues { it != Carry.Unknown }
    }

    private fun order(list: List<Entry>, current: Selection?, prefer: (Entry) -> Boolean): List<Entry> = list.sortedWith(
        compareBy<Entry> { if (it.sel == current) 0 else 1 }
            .thenBy { if (prefer(it)) 0 else 1 }
            .thenBy { if (it.pinned) 0 else 1 }
            .thenBy { it.prior ?: Int.MAX_VALUE }
    )

    private fun whitelistOrder(list: List<Entry>, current: Selection?, prefer: (Entry) -> Boolean): List<Entry> = list.sortedWith(
        compareBy<Entry> {
            if (it.whitelist) {
                0
            } else if (it.udp) {
                3
            } else if (!it.no) {
                1
            } else {
                2
            }
        }
            .thenBy { if (prefer(it)) 0 else 1 }
            .thenBy { if (it.sel == current) 0 else 1 }
            .thenBy { it.prior ?: Int.MAX_VALUE }
    )

    internal fun switches(best: Int, current: Int, margin: Int, bestSamples: Int, currentSamples: Int): Boolean {
        val need = maxOf(margin, SWITCH_MIN_MS, (current.toLong() * SWITCH_SHARE / 100).toInt())
        return bestSamples >= 2 && currentSamples >= 2 && best.toLong() + need < current
    }

    internal fun <T> preferring(list: List<T>, prefer: (T) -> Boolean): List<T> = list.filter(prefer).ifEmpty { list }

    fun evaluate(ctx: Context, s: Settings, run: Run): AutoState {
        val every = entries(ctx, s)
        val early = if (s.autoWhitelist != WhitelistMode.Always) every.filterNot { it.reserve } else every
        val wanted = s.autoTransports.toSet()
        val prefers = { e: Entry -> transport(e.p) in wanted }
        val netName = network(ctx, run.network)
        if (every.isEmpty()) return AutoState(System.currentTimeMillis(), NetMode.Free, netName)
        val timeout = (s.pingTimeout * 1000).coerceIn(3000, 8000)
        val small = s.autoSaveData && metered(ctx, run.network)
        val batch = if (small) {
            5
        } else if (run.quick) {
            16
        } else if (run.light) {
            8
        } else {
            32
        }

        val bg = Executors.newSingleThreadExecutor()
        val probesF = bg.submit<List<Probe>> {
            when (s.autoWhitelist) {
                WhitelistMode.Auto -> probe(run.network, hosts(s.autoHome), hosts(s.autoFree), early = run.quick)
                else -> emptyList()
            }
        }
        val first = (order(early.filter { !it.whitelist }, run.current, prefers).take(batch) + whitelistOrder(early, run.current, prefers).take(if (small || run.light) 3 else 8)).distinctBy { it.key }
        val res: HashMap<String, Int>
        var probes: List<Probe>
        try {
            res = HashMap(ping(ctx, s, run, first, timeout))
            probes = runCatching { probesF.get(PROBE_MS * 3L, TimeUnit.MILLISECONDS) }.getOrDefault(emptyList())
        } finally {
            bg.shutdownNow()
        }

        var mode = when (s.autoWhitelist) {
            WhitelistMode.Always -> NetMode.Whitelist
            WhitelistMode.Never -> NetMode.Free
            WhitelistMode.Auto -> classify(probes)
        }
        if (mode == NetMode.Whitelist && s.autoWhitelist == WhitelistMode.Auto && !run.quick) {
            val again = probe(run.network, emptyList(), hosts(s.autoFree))
            if (again.any { it.ok }) {
                probes = probes.filter { it.home } + again
                mode = NetMode.Free
            }
        }

        val all = every.filterNot { heldBack(s, mode, it.reserve) }
        if (all.isEmpty()) return AutoState(System.currentTimeMillis(), mode, netName, probes)

        val carried = HashSet<String>()
        val squeezed = HashSet<String>()
        fun passing(list: List<Entry>) = list.filter { (res[it.key] ?: -1) >= 0 }
        if (passing(all).isEmpty() && run.alive()) {
            val rest = all.filter { it.key !in res }
            res += ping(ctx, s, run, (if (mode == NetMode.Free) order(rest, run.current, prefers) else whitelistOrder(rest, run.current, prefers)).take(if (small) 5 else 24), timeout)
        }
        if (mode == NetMode.Whitelist && run.alive()) {
            val known = all.filter { it.whitelist && it.key !in res }.take(8)
            if (known.isNotEmpty()) res += ping(ctx, s, run, known, timeout)
        }

        val tries = HashMap<String, MutableList<Int>>()
        res.forEach { (k, v) -> tries[k] = mutableListOf(v) }
        val lead = passing(all).sortedWith(compareBy<Entry> { if (prefers(it)) 0 else 1 }.thenBy { res[it.key] }).take(3).toMutableList()
        all.firstOrNull { it.sel == run.current && it !in lead && (res[it.key] ?: -1) >= 0 }?.let { lead += it }
        if (!run.quick && lead.size > 1) {
            repeat(if (run.light) 1 else 2) {
                if (!run.alive()) return@repeat
                ping(ctx, s, run, lead, timeout).forEach { (k, v) -> tries.getOrPut(k) { mutableListOf() } += v }
            }
        }
        if (mode == NetMode.Whitelist && run.alive()) {
            fun best(e: Entry) = tries[e.key].orEmpty().filter { it >= 0 }.minOrNull() ?: Int.MAX_VALUE
            var left = passing(all).sortedWith(compareBy<Entry> { if (it.whitelist) 0 else 1 }.thenBy { if (it.sel == run.current) 0 else 1 }.thenBy(::best))
            repeat(3) {
                if (left.isEmpty() || carried.isNotEmpty() || !run.alive()) return@repeat
                val chunk = left.take(if (small) 3 else 6)
                left = left.drop(chunk.size)
                carry(ctx, s, run, chunk, timeout).forEach { (k, v) -> if (v == Carry.Ok) carried += k else squeezed += k }
            }
        }
        fun visible(e: Entry): List<Int> = if (e.key in squeezed) listOf(-1) else tries[e.key].orEmpty()
        val speeds = if (s.autoSpeed) SpeedTest.history(ctx).groupBy { it.name }.mapValues { (_, v) -> v.first().download } else emptyMap()

        fun score(e: Entry): Int {
            val t = tries[e.key].orEmpty()
            val ok = t.filter { it >= 0 }.sorted()
            if (ok.isEmpty()) return Int.MAX_VALUE
            val median = ok[ok.size / 2]
            val loss = (t.size - ok.size) * LOSS_MS
            val fast = speeds[e.p.name]?.let { (it / 5).toInt().coerceIn(0, 40) } ?: 0
            val pin = if (s.autoPins && e.pinned) PIN_MS else 0
            val bypass = if (mode == NetMode.Free && e.bypass) WHITELIST_PENALTY else 0
            return median + loss - fast - pin + bypass
        }

        val marks = s.autoWlMarked
        fun rank(e: Entry) = (
            if (e.key in carried) {
                0
            } else if (e.key in squeezed) {
                4
            } else {
                2
            }
            ) + when (mode) {
            NetMode.Whitelist -> if (!marks || e.whitelist) 0 else 1
            NetMode.Free -> if (e.bypass) 1 else 0
            NetMode.Offline -> 0
        }
        val answered = passing(all)
        val top = answered.minOfOrNull(::rank)
        val ranked = answered.filter { rank(it) == top }.map { it to score(it) }.sortedBy { it.second }
        val pool = preferring(ranked) { prefers(it.first) }
        val best = pool.firstOrNull()
        val cur = pool.firstOrNull { it.first.sel == run.current }
        fun samples(e: Entry) = tries[e.key].orEmpty().count { it >= 0 }
        val chosen = when {
            best == null -> null
            cur == null || run.failed -> best.first.sel
            run.quick -> if (best.second + s.autoMargin < cur.second) best.first.sel else cur.first.sel
            switches(best.second, cur.second, s.autoMargin, samples(best.first), samples(cur.first)) -> best.first.sel
            else -> cur.first.sel
        }
        if (mode == NetMode.Whitelist) {
            val tried = all.filter { it.key in res }
            learn(
                ctx,
                tried.filter { it.key in carried }.map { sig(it.p) }.distinct(),
                tried.filter { (res[it.key] ?: -1) < 0 }.map { sig(it.p) }.distinct(),
                tried.filter { it.key in squeezed }.map { sig(it.p) }.distinct()
            )
        }
        all.filter { it.key in tries }.groupBy { it.sub.id }.forEach { (sub, list) ->
            runCatching {
                Pings.merge(ctx, sub, list.associate { e -> e.p.remarks to visible(e).filter { it >= 0 }.sorted().let { if (it.isEmpty()) Pings.TIMEOUT else it[it.size / 2] } })
            }
        }

        val shown = res.keys.mapNotNull { k -> all.firstOrNull { it.key == k } }
            .map { e ->
                val ms = visible(e).filter { it >= 0 }.sorted().let { if (it.isEmpty()) -1 else it[it.size / 2] }
                Candidate(e.sub.id, e.p.remarks, e.p.name, e.p.flag, e.sub.info.title.orEmpty(), ms, score(e), e.whitelist, e.yes, pool = true)
            }
            .sortedWith(compareBy<Candidate> { if (it.ok) 0 else 1 }.thenBy { it.score })
            .take(12)
        return AutoState(System.currentTimeMillis(), mode, netName, probes, shown, all.size, chosen)
    }

    fun freeAgain(s: Settings, network: Network?): Boolean = s.autoWhitelist == WhitelistMode.Auto && probe(network, emptyList(), hosts(s.autoFree)).any { it.ok }
}
