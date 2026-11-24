package dev.yzewe.rynet.vpn

import android.app.AlarmManager
import android.app.KeyguardManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.os.PowerManager
import android.os.SystemClock
import dev.yzewe.rynet.Lang
import dev.yzewe.rynet.R
import dev.yzewe.rynet.core.CaRoots
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.core.PingSink
import dev.yzewe.rynet.core.SpeedSink
import dev.yzewe.rynet.data.AppIcon
import dev.yzewe.rynet.data.AutoSelect
import dev.yzewe.rynet.data.AutoState
import dev.yzewe.rynet.data.CoreProc
import dev.yzewe.rynet.data.CoreStats
import dev.yzewe.rynet.data.Countries
import dev.yzewe.rynet.data.Diag
import dev.yzewe.rynet.data.Geo
import dev.yzewe.rynet.data.GeoDat
import dev.yzewe.rynet.data.Local
import dev.yzewe.rynet.data.NetMode
import dev.yzewe.rynet.data.PingMode
import dev.yzewe.rynet.data.Pings
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.Reports
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.SpeedResult
import dev.yzewe.rynet.data.SpeedRun
import dev.yzewe.rynet.data.SpeedTest
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.Store
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.Subscription
import dev.yzewe.rynet.data.TunRoutes
import dev.yzewe.rynet.data.Tune
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.data.WhitelistMode
import dev.yzewe.rynet.data.byRemarks
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.ui.HomeActivity
import dev.yzewe.rynet.ui.Seed
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

class RynetService : VpnService() {
    @Volatile internal var handle = 0L

    @Volatile internal var generation = 0

    @Volatile internal var destroyed = false

    @Volatile private var lastStartId = 0
    internal val lock = Any()
    private val connector = CONNECTOR
    private val connecting = java.util.concurrent.atomic.AtomicInteger()
    internal val geo = java.util.concurrent.atomic.AtomicInteger()

    @Volatile internal var pinging = false

    @Volatile internal var pingGen = 0

    @Volatile internal var pingSince = 0L

    @Volatile internal var pingingSub: String? = null
    internal val pingQueue = java.util.concurrent.ConcurrentLinkedQueue<String>()
    internal var netCallback: ConnectivityManager.NetworkCallback? = null
    internal var wifiWatch: ConnectivityManager.NetworkCallback? = null
    internal val wifiNets = HashSet<Network>()
    internal var onWifi: Boolean? = null

    @Volatile internal var wifiPaused: Selection? = null
    internal var lockWatch: BroadcastReceiver? = null
    internal val wifiCheck = Runnable { checkWifi() }

    internal class Live(val sub: String, val profile: String, val title: String, val since: Long = 0)

    @Volatile internal var live: Live? = null

    @Volatile internal var lastPing: Int? = null
    internal val main = Handler(Looper.getMainLooper())
    internal var prevStats: CoreStats.Snapshot? = null
    internal var statsMap: CoreStats? = null
    private var ticking = false
    internal var lastShown: String? = null
    internal var shownAs: Pair<Boolean, Boolean>? = null
    internal var promoted = false
    private val tick = object : Runnable {
        override fun run() {
            if (handle == 0L || live == null) return
            if (!getSystemService(android.os.PowerManager::class.java).isInteractive) {
                if (!screenRegistered) main.postDelayed(this, 30_000)
                return
            }
            val l = live ?: return
            val d = details(l)
            val shown = "$d|${failingLive === l}"
            if (shown != lastShown) {
                getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(connected = true, details = d))
                lastShown = shown
            }
            main.postDelayed(this, 2000)
        }
    }
    private val screen = object : android.content.BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            if (i.action != Intent.ACTION_USER_PRESENT) main.removeCallbacks(tick)
            if (ticking && i.action == Intent.ACTION_SCREEN_ON) main.post(tick)
            if (autoDeferred && i.action != Intent.ACTION_SCREEN_OFF) {
                main.removeCallbacks(autoScreen)
                main.postDelayed(autoScreen, if (i.action == Intent.ACTION_USER_PRESENT) AUTO_SCREEN_MS else AUTO_SCREEN_LIT_MS)
            }
        }
    }
    private var screenRegistered = false
    internal val health = Runnable {
        if (checking.compareAndSet(false, true)) {
            Thread({
                try {
                    checkHealth()
                } finally {
                    checking.set(false)
                }
            }, "rynet-health").start()
        }
    }
    private val checking = java.util.concurrent.atomic.AtomicBoolean(false)
    private val trim = Runnable { Thread({ Runtime.getRuntime().gc() }, "rynet-trim").start() }
    internal fun trimSoon() {
        main.removeCallbacks(trim)
        main.postDelayed(trim, TRIM_MS)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStartId = startId
        when (intent?.action) {
            ACTION_LOCK_OFF -> {
                val locked = getSystemService(KeyguardManager::class.java)?.isKeyguardLocked == true ||
                    getSystemService(PowerManager::class.java)?.isInteractive == false
                if (locked && Settings.load(this).lockOff && (live != null || wifiPaused != null)) {
                    disconnect("screen locked")
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_DISCONNECT -> {
                disconnect(intent.getStringExtra(EXTRA_REASON) ?: "unknown")
                return START_NOT_STICKY
            }

            ACTION_PAUSE -> {
                val l = live
                if (l == null) {
                    if (idle()) stopSelf(startId)
                    return START_NOT_STICKY
                }
                disconnect("pause")
                notifyPaused(Selection(l.sub, l.profile))
                return START_NOT_STICKY
            }

            ACTION_UNBLOCK -> {
                val blocked = synchronized(lock) { handle == 0L && tun != null } && connecting.get() == 0
                if (blocked && !Settings.load(this).killSwitch) {
                    disconnect("kill switch off")
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_SETTINGS -> {
                val s = Settings.load(this)
                when {
                    handle != 0L && live != null -> {
                        if (promoted && !s.requestsLiveUpdate(Build.VERSION.SDK_INT)) {
                            stopForeground(STOP_FOREGROUND_REMOVE)
                            startInForeground(connected = true)
                        } else {
                            showOngoing()
                        }
                        schedule(s)
                        if (s.wifiPause) watchWifi(s) else unwatchWifi()
                        if (s.lockOff) watchLock() else unwatchLock()
                    }

                    wifiPaused != null -> {
                        if (!s.wifiPause) {
                            disconnect("wifi pause off")
                        } else if (s.lockOff) {
                            watchLock()
                        } else {
                            unwatchLock()
                        }
                    }

                    idle() -> stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_PING_ALL -> {
                pingQueue.clear()
                intent.getStringArrayListExtra(EXTRA_SUBS)?.let { pingQueue.addAll(it) }
                if (!pinging) pingQueue.poll()?.let { ping(it, startId) } ?: run { if (idle()) stopSelf(startId) }
                return START_NOT_STICKY
            }

            ACTION_PING -> {
                pingQueue.clear()
                intent.getStringExtra(EXTRA_SUB)?.let { ping(it, startId, intent.getStringExtra(EXTRA_PROFILE), intent.getStringArrayListExtra(EXTRA_NAMES)) }
                return START_NOT_STICKY
            }

            ACTION_SPEEDTEST -> {
                val sub = intent.getStringExtra(EXTRA_SUB)
                val profile = intent.getStringExtra(EXTRA_PROFILE)
                if (sub != null && profile != null) {
                    speedtest(Selection(sub, profile), startId)
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_PING_CANCEL -> {
                pingQueue.clear()
                if (pinging) {
                    pingGen++
                    runCatching { Native.pingCancel() }
                    pinging = false
                    pingingSub?.let { sub -> Pings.publish(this, Pings.forSub(this, sub).copy(running = false, done = 0, total = 0, target = null, targets = null)) }
                }
                if (idle()) stopSelf(startId)
                return START_NOT_STICKY
            }

            ACTION_SPEEDTEST_CANCEL -> {
                speedCancel = true
                Native.speedtestCancel()
                if (!speeding && idle()) stopSelf(startId)
                return START_NOT_STICKY
            }

            ACTION_PROBE -> {
                val tid = synchronized(lock) { if (handle != 0L) Native.coreTid(handle) else -1 }
                if (tid >= 0) {
                    CoreProc.sample(this, tid)
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_HPROF -> {
                val path = intent?.getStringExtra(EXTRA_PATH)
                if (path != null) Thread { dev.yzewe.rynet.data.Profiling.dumpHeap(java.io.File(path)) }.start()
                if (idle()) stopSelf(startId)
                return START_NOT_STICKY
            }

            ACTION_AUTO_CHECK -> {
                if (live != null && handle != 0L) autoCheck(AutoWhy.Asked) else autoLook(startId)
                return START_NOT_STICKY
            }

            ACTION_AUTO_UNDO -> {
                val sub = intent.getStringExtra(EXTRA_SUB)
                val profile = intent.getStringExtra(EXTRA_PROFILE)
                getSystemService(NotificationManager::class.java).cancel(AUTO_ID)
                if (sub != null && profile != null && live != null) {
                    autoHoldUntil = SystemClock.elapsedRealtime() + AUTO_HOLD_MS
                    Diag.event(this, "auto", "state" to "undo", "to" to profile)
                    Subs.select(this, Selection(sub, profile))
                    connect(Selection(sub, profile))
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_ICON -> {
                iconRes = 0
                val shown = shownAs
                if (!idle() && shown != null) {
                    lastShown = null
                    runCatching { getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(shown.first, shown.second)) }
                } else if (idle()) {
                    stopSelf(startId)
                }
                return START_NOT_STICKY
            }

            ACTION_FLUSH_LOG -> {
                Native.flushLog()
                if (idle()) stopSelf(startId)
                return START_NOT_STICKY
            }

            else -> {
                runCatching { startInForeground(connected = false) }
                if (intent == null && consent(this) != null) {
                    VpnState.publish(this, VpnState(Status.Off))
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf(startId)
                    return START_NOT_STICKY
                }
                val sel = intent?.getStringExtra(EXTRA_SUB)?.let { sub ->
                    intent.getStringExtra(EXTRA_PROFILE)?.let { Selection(sub, it) }
                } ?: Subs.selection(this)
                autoHoldUntil = 0L
                autoTried.clear()
                wifiPaused = null
                connect(sel, auto = Settings.load(this).autoSelect)
            }
        }
        return START_STICKY
    }

    internal fun connect(sel: Selection?, retry: Int = 0, auto: Boolean = false) {
        if (destroyed) return
        Native.syncLanguage()
        if (retry == 0) getSystemService(NotificationManager::class.java).cancel(PAUSE_ID)
        if (sel == null && auto) {
            Subs.list(this).firstNotNullOfOrNull { sub -> Subs.profiles(this, sub.id).firstOrNull { it.supported }?.let { Selection(sub.id, it.remarks) } }?.let {
                return connect(it, retry, auto)
            }
        }
        if (sel == null) {
            fail(tr("Не выбран сервер", "No server selected"))
            return
        }
        val gen = ++generation
        failingLive = null
        main.removeCallbacks(failingCheck)
        cancelFailingAlert()
        synchronized(lock) { off = false }
        VpnState.publish(this, VpnState(Status.Connecting, sel.profile, error = if (retry > 0) VpnState.load(this).error else null))
        runCatching { startInForeground(connected = false) }
        if (Settings.load(this).debugLog) runCatching { Diag.start(this) }.onFailure { runCatching { Diag.stop() } }
        Diag.event(this, "vpn", "state" to "connecting")
        if (pinging) runCatching { Native.pingCancel() }
        connecting.incrementAndGet()
        connector.execute {
            try {
                if (gen == generation) {
                    if (runCatching { Settings.load(this).killSwitch }.getOrDefault(false)) runCatching { blockTraffic() }
                    runCatching { establish(if (auto) autoPick(sel, gen) else sel, gen, retry) }.onFailure { e -> main.post { if (gen == generation) runCatching { fail(e.message ?: e.toString()) } } }
                }
            } catch (_: Throwable) {
            } finally {
                connecting.decrementAndGet()
            }
        }
    }

    private fun establish(requested: Selection, gen: Int, retry: Int) {
        val t0 = SystemClock.elapsedRealtime()
        val config = Subs.body(this, requested.sub) ?: error(tr("Нет подписки", "No subscription"))
        val settings = Settings.load(this)
        val all = Subscription.parse(config)
        val sel = if (all.byRemarks(requested.profile) != null) {
            requested
        } else {
            replacement(requested, all)
                ?: error(tr("Профиль «${requested.profile}» не найден", "Profile \"${requested.profile}\" not found"))
        }
        val profile = sel.profile
        val p = all.byRemarks(profile)!!
        if (!p.supported) {
            val why = p.reason?.substringBefore('`')?.trim()?.trimEnd(':')?.takeIf { it.isNotEmpty() }
            Diag.event(this, "vpn", "state" to "unsupported", "kind" to p.kind, "reason" to (why ?: ""))
            error(tr("${p.kind.ifEmpty { "Этот профиль" }} не поддерживается", "${p.kind.ifEmpty { "This profile" }} is not supported") + (why?.let { ": $it" } ?: ""))
        }

        val entry = Tune.entry(this, settings, sel)
        if (settings.chainEntry() != null && settings.chainEntry() != sel && entry == null) error(tr("Цепочка: входной сервер не найден", "Chain: entry server not found"))
        if (entry != null && !entry.supported) error(tr("Цепочка: вход «${entry.name}» не поддерживается", "Chain: entry \"${entry.name}\" is not supported"))
        val names = (p.hosts + entry?.hosts.orEmpty()).distinct()
        var hosts = resolveHosts(names)
        if (handle != 0L && names.any { !hosts.has(it) }) {
            stopCore()
            hosts = resolveHosts(names)
        }
        val mtu = settings.tunMtu(autoMtu())

        val tHosts = SystemClock.elapsedRealtime()
        val proxy = settings.proxyMode
        val key = if (proxy) "proxy:${settings.proxyPort}" else tunKey(settings, mtu)
        val reuse = synchronized(lock) { handle != 0L && (tun != null) != proxy && tunKey == key }
        val tEstablish: Long
        var probeTook = 0L
        if (reuse) {
            if (gen != generation) return
            val prev = live
            if (prev != null && (prev.sub != sel.sub || prev.profile != profile)) {
                Native.addHosts(hosts.toString())
                val tProbe = SystemClock.elapsedRealtime()
                reachable(config, sel, gen)?.let { why ->
                    if (gen == generation) {
                        val (code, _) = Pings.classify(why)
                        val p0 = Pings.forSub(this, sel.sub)
                        Pings.publish(this, p0.copy(results = p0.results + (profile to code), errors = p0.errors + (profile to why)))
                        main.post { if (gen == generation) runCatching { stayOn(prev, profile, why) } }
                    }
                    return
                }
                probeTook = SystemClock.elapsedRealtime() - tProbe
                if (gen != generation) return
            }
            CaRoots.ensure()
            val options = coreOptions(settings, sel)
            tEstablish = SystemClock.elapsedRealtime()
            synchronized(lock) {
                if (gen != generation || handle == 0L) return
                Native.reconfigure(this, handle, config, profile, hosts.toString(), Geo.dir(this).path, options)
            }
        } else {
            val pfd = if (proxy) {
                null
            } else {
                val builder = tunBuilder(settings, mtu)
                if (gen != generation) return
                builder.establish() ?: error(tr("Нет разрешения на VPN", "No VPN permission"))
            }
            val prev = synchronized(lock) {
                if (off) {
                    runCatching { pfd?.close() }
                    return
                }
                tunKey = key
                tun.also { tun = pfd }
            }
            if (prev !== pfd) prev?.let { runCatching { it.close() } }
            if (gen != generation) return
            val fd = pfd?.dup()?.detachFd() ?: -1
            tEstablish = SystemClock.elapsedRealtime()
            val next = try {
                CaRoots.ensure()
                Native.start(this, fd, config, profile, hosts.toString(), mtu, Geo.dir(this).path, coreOptions(settings, sel))
            } catch (e: Throwable) {
                if (fd >= 0) ParcelFileDescriptor.adoptFd(fd).close()
                throw e
            }
            val old = synchronized(lock) {
                if (gen != generation) {
                    Native.stop(next)
                    return
                }
                handle.also { handle = next }
            }
            if (old != 0L) Native.stop(old)
            main.post { if (gen == generation) watchNetwork() }
        }
        val tStart = SystemClock.elapsedRealtime()
        val probed = reuse && live != null
        (if (probed) null else reachable(config, sel, gen))?.let { why ->
            if (gen == generation) {
                val (code, _) = Pings.classify(why)
                val p0 = Pings.forSub(this, sel.sub)
                Pings.publish(this, p0.copy(results = p0.results + (profile to code), errors = p0.errors + (profile to why)))
                if (!retryConnect(sel, config, why, gen, retry)) main.post { if (gen == generation) runCatching { fail(why) } }
            }
            return
        }
        if (gen != generation) return
        val probeMs = (SystemClock.elapsedRealtime() - tStart + probeTook).toInt()
        lastPing = probeMs
        if (entry != null) {
            val p0 = Pings.forSub(this, sel.sub)
            Pings.publish(this, p0.copy(results = p0.results + (profile to probeMs), errors = p0.errors - profile))
        }
        val title = { x: dev.yzewe.rynet.data.Profile -> listOf(x.flag, x.name).filter { it.isNotBlank() }.joinToString(" ") }
        val since = System.currentTimeMillis()
        val connected = Live(sel.sub, profile, listOfNotNull(entry?.let(title), title(p)).joinToString(" → "), since)
        val now = SystemClock.elapsedRealtime()
        main.post {
            if (gen != generation) return@post
            runCatching {
                live = connected
                autoTried.clear()
                trimSoon()
                if (settings.debugLog) Diag.start(this) else Diag.stop()
                Diag.event(
                    this, "vpn", "state" to "on", "kind" to p.kind, "routing" to settings.routing.key, "ms" to now - t0,
                    "hosts_ms" to tHosts - t0, "tun_ms" to tEstablish - tHosts - probeTook, "core_ms" to tStart - tEstablish, "probe_ms" to now - tStart + probeTook,
                    "fakedns" to settings.fakeDns, "mtu" to mtu, "same_tun" to reuse, "chain" to (entry != null), "fragment" to settings.fragment
                )
                VpnState.publish(this, VpnState(Status.On, profile, since))
                startInForeground(connected = true)
                schedule(settings)
                watchWifi(settings)
                if (settings.lockOff) watchLock()
            }.onFailure { e -> runCatching { fail(e.message ?: e.toString()) } }
        }
    }

    internal fun replacement(gone: Selection, all: List<Profile>): Selection? {
        val pings = Pings.forSub(this, gone.sub).results
        val ok = all.filter { it.supported }
        val p = ok.filter { (pings[it.remarks] ?: -1) >= 0 }.minByOrNull { pings[it.remarks]!! } ?: ok.firstOrNull() ?: return null
        val next = Selection(gone.sub, p.remarks)
        Diag.event(this, "vpn", "state" to "replaced", "from" to gone.profile, "to" to p.remarks)
        if (Subs.selection(this) == gone) Subs.select(this, next)
        VpnState.publish(this, VpnState(Status.Connecting, p.remarks))
        return next
    }

    private fun reachable(config: String, sel: Selection, gen: Int): String? {
        val settings = Settings.load(this)
        val tune = Tune.json(this, settings, chain = true, exit = sel, servers = false).toString()
        var why: String? = null
        for (timeout in listOf((settings.pingTimeout * 1000).coerceIn(3000, 6000), (settings.pingTimeout * 1000).coerceIn(6000, 10000))) {
            if (gen != generation) return null
            val r = runCatching {
                CaRoots.ensure()
                JSONObject(Native.ping(this, config, JSONArray(listOf(sel.profile)).toString(), PingMode.Proxy.code, 1, timeout, null, settings.pingUrl, tune)).opt(sel.profile)
            }.getOrElse { return it.message ?: it.toString() }
            if (r is Number && r.toInt() >= 0) return null
            why = (r as? String)?.takeIf { it.isNotBlank() } ?: tr("Таймаут: сервер не ответил вовремя", "Timeout: the server didn't respond in time")
            if (Pings.classify(why).first != Pings.TIMEOUT) break
        }
        return why
    }

    private fun coreOptions(settings: Settings, sel: Selection): String = settings.toJson()
        .also { o -> Tune.json(this, settings, chain = true, exit = sel).let { t -> t.keys().forEach { k -> o.put(k, t.get(k)) } } }
        .put("userRules", TunRoutes.guarded(settings.effectiveRules()))
        .also { o -> Subs.list(this).firstOrNull { it.id == sel.sub }?.info?.routing?.let { r -> runCatching { o.put("subRouting", JSONObject(GeoDat.localize(this, r))) } } }
        .put("logDir", Diag.dir(this).path)
        .apply { if (settings.fakeDns) put("fakeDnsPath", java.io.File(filesDir, "fakedns").path) }
        .put("statsPath", CoreStats.file(this).path)
        .put("systemDns", systemDns(null))
        .put("dns", org.json.JSONArray(settings.dnsFor(sel)))
        .toString()

    internal fun underlying(): Network? {
        val cm = getSystemService(ConnectivityManager::class.java)
        return runCatching {
            fun usable(n: Network) = cm.getNetworkCapabilities(n)?.let {
                it.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) && !it.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
            } == true
            @Suppress("DEPRECATION")
            cm.activeNetwork?.takeIf(::usable)
                ?: Companion.underlying?.takeIf(::usable)
                ?: cm.allNetworks.filter(::usable).maxByOrNull { n ->
                    val c = cm.getNetworkCapabilities(n)
                    (if (c?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true) 2 else 0) +
                        (if (c?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == true) 1 else 0)
                }
        }.getOrNull()
    }

    internal fun systemDns(network: Network?): JSONArray {
        val cm = getSystemService(ConnectivityManager::class.java)
        val net = network ?: underlying()
        val servers = net?.let { runCatching { cm.getLinkProperties(it)?.dnsServers }.getOrNull() }.orEmpty()
        return JSONArray(servers.mapNotNull { it.hostAddress?.takeUnless { a -> '%' in a } })
    }

    internal fun resolveHosts(names: List<String>, budgetMs: Long = 15_000): JSONObject {
        val cache = Store.read(this, HOSTS)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: JSONObject()
        val out = JSONObject()
        CaRoots.ensure()
        val settings = Settings.load(this)
        val net = underlying()
        val t0 = System.nanoTime()
        fun left(fromNs: Long, ms: Long) = (fromNs + ms * 1_000_000 - System.nanoTime()).coerceAtLeast(0)
        fun <T> java.util.concurrent.Future<T>.within(ns: Long): T? = runCatching { get(ns, java.util.concurrent.TimeUnit.NANOSECONDS) }.getOrNull()
        val r = synchronized(lock) { if (handle != 0L) Native.resolver(handle) else 0L }
        val threads = names.size.coerceIn(1, 8)
        val corePool = if (r != 0L) java.util.concurrent.Executors.newFixedThreadPool(threads) else null
        val directPool = java.util.concurrent.Executors.newFixedThreadPool(threads * 2 + 1)
        val found = try {
            val viaCore = corePool?.let { p -> names.map { h -> p.submit<String?> { runCatching { Native.resolve(r, h) }.getOrNull() } } }
            val mode = settings.serverDns.takeUnless { it == dev.yzewe.rynet.data.ServerDns.Custom && settings.serverDnsCustom.isBlank() } ?: dev.yzewe.rynet.data.ServerDns.Auto
            val direct by lazy {
                val from = System.nanoTime()
                val doh = if (mode != dev.yzewe.rynet.data.ServerDns.System) {
                    directPool.submit<JSONObject?> { runCatching { JSONObject(Native.bootstrapHosts(this, JSONArray(names).toString(), settings.serverDnsArg())) }.getOrNull() }
                } else {
                    null
                }
                val sys = if (mode == dev.yzewe.rynet.data.ServerDns.System || mode == dev.yzewe.rynet.data.ServerDns.Auto) {
                    names.map { h ->
                        directPool.submit<List<String>> {
                            runCatching { net?.getAllByName(h) ?: InetAddress.getAllByName(h) }.getOrElse { emptyArray() }
                                .sortedBy { if (it is Inet4Address) 0 else 1 }.mapNotNull { it.hostAddress }
                        }
                    }
                } else {
                    null
                }
                Triple(from, doh, sys)
            }
            names.mapIndexed { i, h ->
                val core = viaCore?.get(i)
                core?.within(left(t0, minOf(CORE_DNS_FIRST_MS, budgetMs)))?.let { return@mapIndexed listOf(it) }
                val (from, doh, sys) = direct
                fun dohIps(ns: Long) = doh?.within(ns)?.optJSONArray(h)?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()
                var ips = dohIps(if (sys != null) minOf(left(from, DOH_FIRST_MS), left(t0, budgetMs)) else left(t0, budgetMs))
                if (ips.isEmpty() && sys != null) ips = sys[i].within(left(t0, budgetMs)).orEmpty()
                if (ips.isEmpty() && sys != null) ips = dohIps(left(t0, budgetMs))
                if (ips.isEmpty()) core?.within(left(t0, budgetMs))?.let { ips = listOf(it) }
                ips
            }
        } finally {
            directPool.shutdownNow()
            if (corePool != null) {
                corePool.shutdownNow()
                Thread({
                    corePool.awaitTermination(15, java.util.concurrent.TimeUnit.SECONDS)
                    Native.freeResolver(r)
                }, "rynet-resolver-free").start()
            }
        }
        for ((h, ips) in names.zip(found)) {
            val got = ips
            val keep = got.filterNot { it.startsWith("198.18.") || it.startsWith("198.19.") }
            when {
                keep.isNotEmpty() -> {
                    out.put(h, JSONArray(keep))
                    cache.put(h, JSONArray(keep))
                }

                cache.has(h) -> out.put(h, cache.getJSONArray(h))
            }
        }
        Store.write(this, HOSTS, cache.toString())
        return out
    }

    @Volatile internal var speeding = false

    @Volatile internal var speedCancel = false

    private fun tunBuilder(settings: Settings, mtu: Int): Builder {
        val builder = Builder()
            .setSession(getString(R.string.app_name))
            .setMtu(mtu)
            .addAddress("172.19.0.1", 30)
            .addDnsServer("172.19.0.2")
            .setBlocking(false)
            .setConfigureIntent(
                PendingIntent.getActivity(this, 0, Intent(this, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
            )
        val only = settings.vpnOnlyApps()
        if (only.isNotEmpty()) {
            if (only.count { runCatching { builder.addAllowedApplication(it) }.isSuccess } > 0) {
                builder.addAllowedApplication(packageName)
            }
        } else {
            for (pkg in settings.directApps()) runCatching { builder.addDisallowedApplication(pkg) }
        }
        if (Build.VERSION.SDK_INT >= 29) {
            builder.setMetered(false)
        }
        val routes = TunRoutes.of(settings)
        if (Build.VERSION.SDK_INT >= 33) {
            builder.addRoute("0.0.0.0", 0)
            for ((a, len) in routes.excluded) builder.excludeRoute(IpPrefix(InetAddress.getByName(a), len))
        } else {
            for ((a, len) in routesExcept(routes.excluded)) builder.addRoute(a, len)
        }
        if (routes.ipv6.isNotEmpty()) {
            builder.addAddress("fdfe:dcba:9876::1", 126)
            builder.addRoute("::", 0)
        }

        return builder
    }

    @Volatile internal var tun: ParcelFileDescriptor? = null
    internal var off = false

    @Volatile private var tunKey: String? = null

    override fun protect(socket: Int): Boolean = if (tun == null && tunKey?.startsWith("proxy:") == true) true else super.protect(socket)

    private fun tunKey(settings: Settings, mtu: Int) = "$mtu|" + settings.directApps().sorted().joinToString(",") + "|" + settings.vpnOnlyApps().joinToString(",") + "|" + TunRoutes.of(settings)

    internal fun idle() = synchronized(lock) { handle == 0L && tun == null } && connecting.get() == 0 && wifiPaused == null && geo.get() == 0

    private fun blockTraffic(): Boolean {
        synchronized(lock) {
            if (tun != null) return true
            if (off) return false
        }
        val settings = Settings.load(this)
        if (settings.proxyMode) return false
        val pfd = runCatching { tunBuilder(settings, settings.tunMtu(autoMtu())).establish() }.getOrNull() ?: return false
        val kept = synchronized(lock) {
            if (tun == null && !off) {
                tun = pfd
                true
            } else {
                false
            }
        }
        if (!kept) runCatching { pfd.close() }
        Diag.event(this, "vpn", "state" to "blocked")
        return true
    }

    internal fun closeTun() {
        val t = synchronized(lock) {
            off = true
            tunKey = null
            tun.also { tun = null }
        }
        t?.let { runCatching { it.close() } }
    }

    private var wake: PowerManager.WakeLock? = null

    @Volatile private var awakeWanted = false

    @Volatile private var keepAliveLive = false

    private fun keepAliveNow(): Boolean = keepAliveLive || ((statsMap ?: CoreStats.open(this)?.also { statsMap = it })?.read()?.keepAlive ?: 0L) > 0

    @Synchronized
    private fun keepAwake(on: Boolean) {
        val w = wake
        if (on) {
            val lock = w ?: getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ryNet:background").apply {
                setReferenceCounted(false)
            }.also { wake = it }
            val was = lock.isHeld
            lock.acquire(AWAKE_MS)
            if (!was) {
                Diag.event(this, "awake", "on" to true)
                main.removeCallbacks(awakeRenew)
                main.postDelayed(awakeRenew, AWAKE_RENEW_MS)
            }
        } else if (w != null) {
            main.removeCallbacks(awakeRenew)
            main.removeCallbacks(awakeRelease)
            if (w.isHeld) {
                runCatching { w.release() }
                Diag.event(this, "awake", "on" to false)
            }
            wake = null
        }
    }

    private val awakeRelease = Runnable { if (!(awakeWanted && live != null && keepAliveNow())) keepAwake(false) }

    private val awakeRenew = object : Runnable {
        override fun run() {
            if (awakeWanted && live != null && keepAliveNow()) {
                keepAwake(true)
                main.removeCallbacks(this)
                main.postDelayed(this, AWAKE_RENEW_MS)
            } else {
                keepAwake(false)
            }
        }
    }

    private fun updateAwake() {
        if (awakeWanted && live != null && keepAliveNow()) {
            main.removeCallbacks(awakeRelease)
            keepAwake(true)
        } else if (wake != null) {
            main.removeCallbacks(awakeRelease)
            main.postDelayed(awakeRelease, AWAKE_LINGER_MS)
        }
    }

    @Suppress("unused")
    fun onKeepAlive(on: Boolean) {
        keepAliveLive = on
        if (on && awakeWanted && live != null) keepAwake(true)
        main.post { updateAwake() }
    }

    internal fun stopCore() {
        live = null
        keepAliveLive = false
        keepAwake(false)
        healthGen++
        prevStats = null
        ticking = false
        main.removeCallbacks(tick)
        main.removeCallbacks(health)
        main.removeCallbacks(refresh)
        main.removeCallbacks(report)
        reportWorked()
        reportMark = 0L
        netCallback?.let { cb -> runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(cb) } }
        netCallback = null
        val h = synchronized(lock) { handle.also { handle = 0 } }
        if (h != 0L) Native.stop(h)
    }

    internal fun disconnect(reason: String) {
        unwatchWifi()
        unwatchLock()
        generation++
        val was = VpnState.load(this).status
        stopCore()
        closeTun()
        Diag.event(this, "vpn", "state" to "off", "reason" to reason, "was" to was.name)
        Diag.stop()
        VpnState.publish(this, VpnState(Status.Off))
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf(lastStartId)
    }

    private fun retryConnect(sel: Selection, config: String, why: String, gen: Int, retry: Int): Boolean {
        val settings = Settings.load(this)
        if (settings.autoSelect && retry < CONNECT_RETRIES) {
            val next = AutoSelect.load(this)?.candidates?.filter { it.ok && it.pool && it.selection != sel && it.selection !in autoTried }?.minByOrNull { it.score }?.selection
            if (next != null) {
                autoTried += sel
                Diag.event(this, "auto", "state" to "next", "from" to sel.profile, "to" to next.profile, "error" to why)
                main.post {
                    if (gen != generation) return@post
                    Subs.select(this, next)
                    connect(next, retry + 1)
                }
                return true
            }
        }
        if (!settings.healthCheck || retry >= CONNECT_RETRIES) return false
        if (settings.autoSwitch && switchAway(Live(sel.sub, sel.profile, "", 0), why, gen, verified = true, retry = retry + 1)) return true
        if (RETRYABLE.none { why.startsWith(it) }) return false
        Diag.event(this, "vpn", "state" to "retry", "attempt" to retry + 1, "error" to why)
        main.post {
            if (gen != generation) return@post
            stopCore()
            VpnState.publish(this, VpnState(Status.Connecting, sel.profile, error = tr("$why · повтор ${retry + 1} из $CONNECT_RETRIES", "$why · retry ${retry + 1} of $CONNECT_RETRIES")))
            main.postDelayed({ if (gen == generation) connect(sel, retry + 1) }, CONNECT_RETRY_MS * (retry + 1))
        }
        return true
    }

    private fun stayOn(prev: Live, tried: String, why: String) {
        Diag.event(this, "vpn", "state" to "kept", "tried" to tried, "error" to why)
        schedule(Settings.load(this))
        Subs.select(this, Selection(prev.sub, prev.profile))
        VpnState.publish(this, VpnState(Status.On, prev.profile, prev.since, error = "«$tried»: $why"))
        runCatching { startInForeground(connected = true) }
        val gen = generation
        main.postDelayed({ if (gen == generation && live === prev) VpnState.publish(this, VpnState(Status.On, prev.profile, prev.since)) }, 8000)
    }

    private fun fail(msg: String) {
        generation++
        val blocked = Settings.load(this).killSwitch && blockTraffic()
        stopCore()
        Diag.event(this, "vpn", "state" to "error", "error" to msg)
        Diag.stop()
        if (blocked) {
            VpnState.publish(this, VpnState(Status.Error, Subs.selection(this)?.profile, error = msg, blocked = true))
            runCatching { startInForeground(connected = false, blocked = true) }
            return
        }
        closeTun()
        VpnState.publish(this, VpnState(Status.Error, Subs.selection(this)?.profile, error = msg))
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf(lastStartId)
    }

    override fun onRevoke() = disconnect("revoked")

    override fun onCreate() {
        super.onCreate()
        Diag.catchCrashes(this) { runCatching { Native.flushLog() } }
    }

    override fun onDestroy() {
        destroyed = true
        generation++
        main.removeCallbacksAndMessages(null)
        unwatchWifi()
        unwatchLock()
        stopCore()
        closeTun()
        runCatching { if (VpnState.load(this).status.let { it == Status.On || it == Status.Connecting }) VpnState.publish(this, VpnState(Status.Off)) }
        Diag.stop()
        if (screenRegistered) runCatching { unregisterReceiver(screen) }
        super.onDestroy()
    }

    private val owners = java.util.concurrent.ConcurrentHashMap<Int, String>()

    @Suppress("unused")
    fun onCoreExit(tid: Int, reason: String) {
        main.post {
            val ours = synchronized(lock) { handle.let { h -> h != 0L && Native.coreTid(h) == tid } }
            if (ours) {
                Diag.event(this, "vpn", "state" to "core-exit", "error" to reason)
                runCatching { fail(tr("Ядро остановилось: $reason", "Core stopped: $reason")) }
            }
        }
    }

    fun ownerPackage(tcp: Boolean, src: String, sport: Int, dst: String, dport: Int): String? {
        if (Build.VERSION.SDK_INT < 29) return null
        return runCatching {
            val uid = getSystemService(ConnectivityManager::class.java).getConnectionOwnerUid(
                if (tcp) android.system.OsConstants.IPPROTO_TCP else android.system.OsConstants.IPPROTO_UDP,
                java.net.InetSocketAddress(InetAddress.getByName(src), sport),
                java.net.InetSocketAddress(InetAddress.getByName(dst), dport)
            )
            if (uid < 0) return null
            owners.getOrPut(uid) { packageManager.getPackagesForUid(uid)?.firstOrNull() ?: return null }
        }.getOrNull()
    }

    internal fun schedule(settings: Settings) {
        awakeWanted = settings.background
        updateAwake()
        healthGen++
        main.removeCallbacks(tick)
        main.removeCallbacks(health)
        main.removeCallbacks(refresh)
        if (settings.autoUpdate) main.postDelayed(refresh, 60_000L)
        main.removeCallbacks(report)
        reportWorked()
        reportMark = if (settings.sendLogs && settings.debugLog) SystemClock.elapsedRealtime() else 0L
        if (reportMark != 0L) main.postDelayed(report, REPORT_TICK_MS)
        ticking = settings.notifyDetails
        if ((ticking || settings.autoSelect) && !screenRegistered) {
            val f = android.content.IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_ON)
                addAction(Intent.ACTION_SCREEN_OFF)
                addAction(Intent.ACTION_USER_PRESENT)
            }
            screenRegistered = runCatching {
                androidx.core.content.ContextCompat.registerReceiver(this, screen, f, androidx.core.content.ContextCompat.RECEIVER_NOT_EXPORTED)
            }.isSuccess
        }
        if (ticking) main.postDelayed(tick, 2000)
        if (settings.healthCheck) main.postDelayed(health, settings.healthSec.coerceAtLeast(Settings.HEALTH_MIN_SEC) * 1000L)
        main.removeCallbacks(autoTick)
        if (settings.autoSelect) {
            if (!autoWasOn) main.postDelayed(autoTick, 1500) else autoDelay(settings, AutoSelect.load(this)?.mode)?.let { main.postDelayed(autoTick, it) }
        }
        autoWasOn = settings.autoSelect
    }

    private val refreshing = java.util.concurrent.atomic.AtomicBoolean(false)

    @Volatile private var reportMark = 0L
    private var reportRetryAt = 0L
    private val reporting = java.util.concurrent.atomic.AtomicBoolean(false)

    private fun reportWorked(): Boolean {
        if (reportMark == 0L) return false
        val now = SystemClock.elapsedRealtime()
        val ms = now - reportMark
        reportMark = now
        return runCatching { Reports.worked(this, ms) }.getOrDefault(false)
    }

    private val report = object : Runnable {
        override fun run() {
            main.postDelayed(this, REPORT_TICK_MS)
            if (live == null || !reportWorked() || SystemClock.elapsedRealtime() < reportRetryAt || !reporting.compareAndSet(false, true)) return
            val version = runCatching {
                val pi = packageManager.getPackageInfo(packageName, 0)
                "android ${pi.versionName} (${androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(pi)})"
            }.getOrDefault("android")
            Thread({
                try {
                    runCatching { Native.flushLog() }
                    val ok = runCatching { Reports.send(this@RynetService, version) }.getOrDefault(false)
                    Diag.event(this@RynetService, "report", "sent" to ok)
                    if (!ok) reportRetryAt = SystemClock.elapsedRealtime() + REPORT_RETRY_MS
                } finally {
                    reporting.set(false)
                }
            }, "rynet-report").start()
        }
    }

    @Volatile internal var healthGen = 0

    @Volatile internal var healthMark: CoreStats.Snapshot? = null

    private val subRetry = java.util.concurrent.ConcurrentHashMap<String, Pair<Int, Long>>()

    private val refresh = object : Runnable {
        override fun run() {
            main.postDelayed(this, 30 * 60_000L)
            if (live == null || !refreshing.compareAndSet(false, true)) return
            Thread({
                try {
                    refreshSubs()
                } finally {
                    refreshing.set(false)
                }
            }, "rynet-subs").start()
        }
    }

    private fun refreshSubs() {
        if (!Settings.load(this).autoUpdate) return
        if (getSystemService(android.os.PowerManager::class.java).isDeviceIdleMode) return
        val now = System.currentTimeMillis()
        for (sub in Subs.list(this)) {
            if (Local.isLocal(sub)) continue
            val every = (sub.info.updateIntervalHours ?: 24).coerceAtLeast(1) * 3_600_000L
            if (now - sub.info.updatedAt <= every) continue
            subRetry[sub.id]?.let { (_, next) -> if (now < next) continue }
            val host = runCatching { java.net.URL(sub.info.url.trim()).host }.getOrNull().orEmpty()
            runCatching { Subs.fetch(this, sub.info.url, sub.id) }
                .onSuccess {
                    subRetry.remove(sub.id)
                    Diag.event(this, "sub", "state" to "refreshed", "sub" to sub.id, "host" to host)
                }
                .onFailure {
                    val n = (subRetry[sub.id]?.first ?: 0) + 1
                    subRetry[sub.id] = n to now + (30 * 60_000L shl (n - 1).coerceAtMost(4)).coerceAtMost(6 * 3_600_000L)
                    val why = generateSequence(it) { e -> e.cause }.take(3)
                        .joinToString(" ← ") { e -> "${e.javaClass.simpleName}: ${e.message.orEmpty()}" }
                        .replace(URL_IN_TEXT, "$1").take(300)
                    Diag.event(this, "sub", "state" to "refresh_failed", "sub" to sub.id, "host" to host, "error" to why)
                }
            trimSoon()
        }
    }

    @Suppress("unused")
    fun onServerFailing() {
        val l = live ?: return
        val gen = generation
        Diag.event(this, "vpn", "state" to "failing", "profile" to l.profile)
        val settings = Settings.load(this)
        val now = SystemClock.elapsedRealtime()
        val showOnly = { main.post { if (gen == generation) runCatching { showFailing(l) } } }
        if (settings.autoSelect) {
            if (now - lastAutoSwitch < AUTO_SWITCH_GAP_MS) {
                showOnly()
                return
            }
            lastAutoSwitch = now
            if (!switching.compareAndSet(false, true)) return
            Thread({
                try {
                    if (gen != generation) return@Thread
                    if (answers(l, settings)) {
                        Diag.event(this, "vpn", "state" to "auto-switch", "result" to "server answers")
                        return@Thread
                    }
                    showOnly()
                    autoCheck(AutoWhy.Failing)
                } finally {
                    switching.set(false)
                }
            }, "rynet-autocheck").start()
            return
        }
        if (!settings.healthCheck) {
            showOnly()
            return
        }
        if (settings.healthIdle || now - lastAutoSwitch < AUTO_SWITCH_GAP_MS) {
            showOnly()
            return
        }
        if (!settings.autoSwitch) {
            lastAutoSwitch = now
            main.post {
                if (gen == generation) {
                    main.removeCallbacks(health)
                    health.run()
                }
            }
            return
        }
        if (!switching.compareAndSet(false, true)) return
        Thread({
            try {
                if (!switchAway(l, tr("соединения не устанавливаются", "connections keep failing"), gen, verified = false) && switchMissed != "server answers") {
                    showOnly()
                }
            } finally {
                switching.set(false)
            }
        }, "rynet-autoswitch").start()
    }

    @Volatile internal var switchMissed: String? = null

    private val switching = java.util.concurrent.atomic.AtomicBoolean(false)

    @Volatile internal var lastAutoSwitch = 0L

    @Volatile internal var failingLive: Live? = null
    internal var failingMark: CoreStats.Snapshot? = null
    internal var failingSince = 0L
    internal var failingProgress = 0
    internal var failingAlerted = false

    internal val failingCheck = object : Runnable {
        override fun run() {
            val l = failingLive ?: return
            if (live !== l) {
                failingLive = null
                cancelFailingAlert()
                return
            }
            val now = coreStats()
            val mark = failingMark
            if (now != null && mark != null && now.generation != mark.generation) {
                clearFailing()
                return
            }
            failingProgress = if (now != null && (mark == null || now.proxyOk > mark.proxyOk)) failingProgress + 1 else 0
            if (now != null) failingMark = now
            if (failingProgress >= FAILING_CLEAR_CHECKS) {
                clearFailing()
                return
            }
            if (!failingAlerted && failingProgress == 0 && SystemClock.elapsedRealtime() - failingSince >= FAILING_ALERT_MS) {
                failingAlerted = true
                if (Settings.load(this@RynetService).healthNotify) runCatching { notifyStillFailing(l) }
            }
            main.postDelayed(this, FAILING_CHECK_MS)
        }
    }

    internal enum class AutoWhy { Periodic, Network, Failing, Dead, Asked }

    @Volatile internal var autoHoldUntil = 0L

    @Volatile internal var autoFullAt = 0L

    @Volatile internal var autoDeepAt = 0L
    internal var autoWasOn = false
    internal val autoRunning = java.util.concurrent.atomic.AtomicBoolean(false)

    @Volatile internal var autoAgain: AutoWhy? = null
    private val autoTried: MutableSet<Selection> = java.util.Collections.synchronizedSet(mutableSetOf())
    internal val autoTick = Runnable { autoCheck(AutoWhy.Periodic) }
    internal val autoNet = Runnable { autoCheck(AutoWhy.Network) }

    @Volatile internal var autoDeferred = false
    private val autoScreen = Runnable {
        if (autoDeferred && getSystemService(PowerManager::class.java).isInteractive) {
            autoDeferred = false
            autoCheck(AutoWhy.Periodic)
        }
    }

    internal data class Details(val title: String, val text: String, val big: String?)

    internal var iconRes = 0

    companion object {
        private val CONNECTOR = java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "rynet-connect") }
        private val URL_IN_TEXT = Regex("(?i)\\b[a-z][a-z0-9+.-]*://(?:[^\\s/?#@]*@)?([^\\s/?#:]+)\\S*")

        internal const val PING_STUCK_MS = 120_000L
        const val ACTION_CONNECT = "dev.yzewe.rynet.CONNECT"
        const val ACTION_DISCONNECT = "dev.yzewe.rynet.DISCONNECT"
        const val ACTION_PAUSE = "dev.yzewe.rynet.PAUSE"
        const val ACTION_LOCK_OFF = "dev.yzewe.rynet.LOCK_OFF"
        internal const val EXTRA_REASON = "reason"
        private const val CORE_DNS_FIRST_MS = 1500L
        private const val DOH_FIRST_MS = 1500L
        internal const val PING_HOSTS_MS = 3000L
        private const val AUTO_SWITCH_GAP_MS = 120_000L
        internal const val FAILING_CHECK_MS = 15_000L
        private const val FAILING_CLEAR_CHECKS = 2
        internal const val FAILING_ALERT_MS = 3 * 60_000L
        internal const val FAILING_ALERT_ID = 6
        private const val CONNECT_RETRIES = 3
        private const val CONNECT_RETRY_MS = 5_000L
        private const val REPORT_TICK_MS = 10 * 60_000L
        private const val TRIM_MS = 10_000L
        private const val AWAKE_MS = 10 * 60_000L
        private const val AWAKE_RENEW_MS = 4 * 60_000L
        private const val AWAKE_LINGER_MS = 5_000L
        private const val REPORT_RETRY_MS = 30 * 60_000L
        private val RETRYABLE = listOf(
            "Таймаут", "Timeout", "Соединение сброшено", "Соединение прервано", "Сервер недоступен из этой сети", "Нет сети",
            "Разрыв TLS", "Не удалось найти адрес сервера", "Сервер оборвал поток"
        )
        internal const val AUTO_SWITCH_CANDIDATES = 6
        const val ACTION_UNBLOCK = "dev.yzewe.rynet.UNBLOCK"

        @Volatile var underlying: Network? = null
        const val ACTION_SETTINGS = "dev.yzewe.rynet.SETTINGS"
        const val ACTION_PING = "dev.yzewe.rynet.PING"
        const val ACTION_PING_ALL = "dev.yzewe.rynet.PING_ALL"
        const val ACTION_PING_CANCEL = "dev.yzewe.rynet.PING_CANCEL"
        const val ACTION_SPEEDTEST = "dev.yzewe.rynet.SPEEDTEST"
        const val ACTION_SPEEDTEST_CANCEL = "dev.yzewe.rynet.SPEEDTEST_CANCEL"
        const val ACTION_FLUSH_LOG = "dev.yzewe.rynet.FLUSH_LOG"
        const val ACTION_ICON = "dev.yzewe.rynet.ICON"
        internal const val BRAND_ACCENT = 0xFF106B57.toInt()
        const val ACTION_PROBE = "dev.yzewe.rynet.PROBE"
        const val ACTION_HPROF = "dev.yzewe.rynet.HPROF"
        const val ACTION_AUTO_CHECK = "dev.yzewe.rynet.AUTO_CHECK"
        const val ACTION_AUTO_UNDO = "dev.yzewe.rynet.AUTO_UNDO"
        internal const val AUTO = "auto"
        internal const val AUTO_ID = 4
        private const val AUTO_HOLD_MS = 3_600_000L
        internal const val AUTO_WHITELIST_MS = 5 * 60_000L
        private const val AUTO_SCREEN_MS = 5_000L
        private const val AUTO_SCREEN_LIT_MS = 30_000L
        internal const val AUTO_DEEP_MS = 60 * 60_000L
        internal const val AUTO_NET_MS = 4_000L
        const val EXTRA_PATH = "path"
        const val EXTRA_PROFILE = "profile"
        private const val EXTRA_NAMES = "names"
        private const val EXTRA_SUBS = "subs"
        const val EXTRA_SUB = "sub"
        internal const val CHANNEL = "vpn"
        internal const val ALERTS = "alerts"
        internal const val NOTIFICATION_ID = 1
        internal const val ALERT_ID = 2
        internal const val PAUSE_ID = 3
        internal const val WIFI_SETTLE_MS = 5_000L
        internal const val LOCK_OFF_MS = 30 * 60_000L
        internal const val PAUSED = "paused"
        private const val HOSTS = "hosts.json"

        const val MTU = 65535

        const val HUAWEI_MTU = 1500

        fun autoMtu(manufacturer: String? = Build.MANUFACTURER, brand: String? = Build.BRAND): Int = if (listOf(manufacturer, brand).any { it.orEmpty().trim().lowercase() in setOf("huawei", "honor") }) HUAWEI_MTU else MTU

        internal fun failoverRounds(remarks: List<String>, pings: Map<String, Int>, lastResort: Set<String>, n: Int): List<List<String>> {
            val byPing = remarks.sortedBy { r -> pings[r]?.takeIf { it >= 0 } ?: Int.MAX_VALUE }
            val (spare, usual) = byPing.partition { it in lastResort }
            return listOf(usual.take(n), spare.take(n)).filter { it.isNotEmpty() }
        }

        internal fun routesExcept(skip: List<Pair<String, Int>>): List<Pair<String, Int>> {
            fun ip(a: String) = a.split('.').fold(0L) { n, b -> n shl 8 or b.toLong() }
            val ranges = skip.map { (a, len) -> ip(a) to (ip(a) + (1L shl (32 - len))) }
            val out = mutableListOf<Pair<String, Int>>()
            fun cover(base: Long, len: Int) {
                val end = base + (1L shl (32 - len))
                when {
                    ranges.any { (s, e) -> s <= base && end <= e } -> {}

                    ranges.none { (s, e) -> s < end && base < e } ->
                        out += (0..3).joinToString(".") { (base shr (24 - 8 * it) and 255).toString() } to len

                    else -> {
                        cover(base, len + 1)
                        cover(base + (1L shl (31 - len)), len + 1)
                    }
                }
            }
            cover(0, 0)
            return out
        }

        fun connect(ctx: Context, sel: Selection) {
            androidx.core.content.ContextCompat.startForegroundService(
                ctx,
                Intent(ctx, RynetService::class.java).setAction(ACTION_CONNECT)
                    .putExtra(EXTRA_SUB, sel.sub).putExtra(EXTRA_PROFILE, sel.profile)
            )
        }

        fun ping(ctx: Context, sub: String, profile: String? = null, names: List<String>? = null) {
            if (names != null && names.isEmpty()) return
            ctx.startService(
                Intent(ctx, RynetService::class.java).setAction(ACTION_PING).putExtra(EXTRA_SUB, sub).putExtra(EXTRA_PROFILE, profile)
                    .apply { names?.let { putStringArrayListExtra(EXTRA_NAMES, ArrayList(it)) } }
            )
        }

        fun consent(ctx: Context): Intent? = if (Settings.load(ctx).proxyMode) null else VpnService.prepare(ctx)

        fun pingAll(ctx: Context, subs: List<String>) {
            if (subs.isEmpty()) return
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_PING_ALL).putStringArrayListExtra(EXTRA_SUBS, ArrayList(subs)))
        }

        fun speedtest(ctx: Context, sel: Selection) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_SPEEDTEST).putExtra(EXTRA_SUB, sel.sub).putExtra(EXTRA_PROFILE, sel.profile))
        }

        fun cancelPing(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_PING_CANCEL))
        }

        fun cancelSpeedtest(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_SPEEDTEST_CANCEL))
        }

        fun iconChanged(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_ICON))
        }

        fun flushLog(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_FLUSH_LOG))
        }

        fun autoCheck(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_AUTO_CHECK))
        }

        fun probe(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_PROBE))
        }

        fun dumpHeap(ctx: Context, path: String) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_HPROF).putExtra(EXTRA_PATH, path))
        }

        fun applySettings(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_SETTINGS))
        }

        fun unblock(ctx: Context) {
            ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_UNBLOCK))
        }

        fun disconnect(ctx: Context, reason: String) {
            try {
                ctx.startService(Intent(ctx, RynetService::class.java).setAction(ACTION_DISCONNECT).putExtra(EXTRA_REASON, reason))
            } catch (_: IllegalStateException) {
                VpnState.publish(ctx, VpnState(Status.Off))
            }
        }
    }
}
