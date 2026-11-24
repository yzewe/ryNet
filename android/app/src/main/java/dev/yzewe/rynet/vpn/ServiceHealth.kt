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
import dev.yzewe.rynet.vpn.RynetService.AutoWhy
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_CONNECT
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_DISCONNECT
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO_SWITCH_CANDIDATES
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_PROFILE
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_REASON
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_SUB
import dev.yzewe.rynet.vpn.RynetService.Companion.FAILING_ALERT_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.FAILING_CHECK_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.NOTIFICATION_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.failoverRounds
import dev.yzewe.rynet.vpn.RynetService.Companion.underlying
import dev.yzewe.rynet.vpn.RynetService.Live
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

internal fun RynetService.checkHealth() {
    val l = live ?: return
    val gen = generation
    val hg = healthGen
    val settings = Settings.load(this)
    if (!settings.healthCheck) return
    if (getSystemService(android.os.PowerManager::class.java).isDeviceIdleMode) {
        main.post { if (gen == generation && hg == healthGen) main.postDelayed(health, settings.healthSec.coerceAtLeast(Settings.HEALTH_MIN_SEC) * 1000L) }
        return
    }
    val stats = (statsMap ?: CoreStats.open(this)?.also { statsMap = it })?.read()
    val mark = healthMark
    healthMark = stats
    CoreStats.healthSkip(mark, stats)?.let { skip ->
        Diag.event(this, "vpn", "state" to "check_skipped", "why" to skip.why, "tried" to skip.tried, "ok" to skip.ok)
        main.post { if (gen == generation && hg == healthGen) main.postDelayed(health, settings.healthSec.coerceAtLeast(Settings.HEALTH_MIN_SEC) * 1000L) }
        return
    }
    var why: String? = null
    for (attempt in 0 until 2) {
        if (attempt > 0) Thread.sleep(5000)
        if (gen != generation || handle == 0L) return
        val t0 = SystemClock.elapsedRealtime()
        val r = probe(l, settings)
        if (r is Number && r.toInt() >= 0) {
            lastPing = r.toInt().takeIf { it > 0 } ?: (SystemClock.elapsedRealtime() - t0).toInt()
            why = null
            break
        }
        why = (r as? String)?.takeIf { it.isNotBlank() } ?: tr("Таймаут: сервер не ответил вовремя", "Timeout: the server didn't respond in time")
    }
    trimSoon()
    if (gen != generation) return
    if (why == null) {
        main.post { if (gen == generation && hg == healthGen) main.postDelayed(health, settings.healthSec.coerceAtLeast(Settings.HEALTH_MIN_SEC) * 1000L) }
        return
    }
    Diag.event(this, "vpn", "state" to "dead", "error" to why)
    if (settings.autoSelect) {
        lastAutoSwitch = SystemClock.elapsedRealtime()
        autoCheck(AutoWhy.Dead, why)
        return
    }
    if (settings.healthIdle) {
        main.post {
            if (gen != generation || hg != healthGen) return@post
            notifyDead(why)
            main.postDelayed(health, settings.healthSec.coerceAtLeast(Settings.HEALTH_MIN_SEC) * 1000L)
        }
        return
    }
    if (settings.autoSwitch && switchAway(l, why, gen, verified = true)) return
    main.post {
        if (gen != generation) return@post
        if (settings.healthNotify) notifyDead(why)
        connect(Selection(l.sub, l.profile))
    }
}

internal fun RynetService.probe(l: Live, settings: Settings): Any? = runCatching {
    CaRoots.ensure()
    val tune = Tune.json(this, settings, chain = true, exit = Selection(l.sub, l.profile), servers = false).toString()
    val body = Subs.body(this, l.sub) ?: return@runCatching null
    JSONObject(Native.ping(this, body, JSONArray(listOf(l.profile)).toString(), PingMode.Proxy.code, 1, (settings.pingTimeout * 1000).coerceAtLeast(5000), null, settings.pingUrl, tune)).opt(l.profile)
}.getOrNull()

internal fun RynetService.answers(l: Live, settings: Settings): Boolean = (probe(l, settings) as? Number)?.toInt()?.let { it >= 0 } == true

internal fun RynetService.coreStats() = (statsMap ?: CoreStats.open(this)?.also { statsMap = it })?.read()

internal fun RynetService.showFailing(l: Live) {
    if (destroyed || live !== l) return
    val first = failingLive !== l
    failingLive = l
    failingMark = coreStats()
    failingProgress = 0
    if (first) {
        failingSince = SystemClock.elapsedRealtime()
        failingAlerted = false
    }
    main.removeCallbacks(failingCheck)
    main.postDelayed(failingCheck, FAILING_CHECK_MS)
    if (!first) return
    Diag.event(this, "vpn", "state" to "failing_shown", "profile" to l.profile)
    VpnState.publish(this, VpnState(Status.On, l.profile, l.since, failing = true))
    showOngoing()
}

internal fun RynetService.clearFailing() {
    val l = failingLive ?: return
    failingLive = null
    main.removeCallbacks(failingCheck)
    cancelFailingAlert()
    if (live !== l) return
    Diag.event(this, "vpn", "state" to "failing_cleared", "profile" to l.profile)
    VpnState.publish(this, VpnState(Status.On, l.profile, l.since))
    showOngoing()
}

internal fun RynetService.cancelFailingAlert() {
    if (!failingAlerted) return
    failingAlerted = false
    runCatching { getSystemService(NotificationManager::class.java).cancel(FAILING_ALERT_ID) }
}

internal fun RynetService.showOngoing() {
    runCatching {
        lastShown = null
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(connected = true))
    }
}

internal fun RynetService.failingNotice(b: Notification.Builder, connected: Boolean) {
    val l = failingLive?.takeIf { connected && it === live } ?: return
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    val again = Intent(this, RynetService::class.java).setAction(ACTION_CONNECT).putExtra(EXTRA_SUB, l.sub).putExtra(EXTRA_PROFILE, l.profile)
    val retry = if (Build.VERSION.SDK_INT >= 26) PendingIntent.getForegroundService(this, 7, again, flags) else PendingIntent.getService(this, 7, again, flags)
    val pick = PendingIntent.getActivity(
        this,
        8,
        Intent(this, HomeActivity::class.java).putExtra(HomeActivity.EXTRA_SERVERS, true).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        flags
    )
    val stop = PendingIntent.getService(
        this,
        1,
        Intent(this, RynetService::class.java).setAction(ACTION_DISCONNECT).putExtra(EXTRA_REASON, "notification"),
        PendingIntent.FLAG_IMMUTABLE
    )
    b.setContentTitle(tr("Сервер не отвечает", "The server does not respond"))
    if (!Settings.load(this).notifyDetails) b.setContentText(l.title) else b.setSubText(null)
    b.setActions(
        Notification.Action.Builder(null, tr("Повторить", "Retry"), retry).build(),
        Notification.Action.Builder(null, tr("Другой сервер", "Another server"), pick).build(),
        Notification.Action.Builder(null, getString(R.string.disconnect), stop).build()
    )
}

internal fun RynetService.switchAway(l: Live, why: String, gen: Int, verified: Boolean, retry: Int = 0): Boolean {
    lastAutoSwitch = SystemClock.elapsedRealtime()
    val cm = getSystemService(ConnectivityManager::class.java)
    val validated = underlying()?.let { cm.getNetworkCapabilities(it) }?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
    switchMissed = null
    if (!validated) {
        Diag.event(this, "vpn", "state" to "auto-switch", "result" to "no network")
        switchMissed = "no network"
        return false
    }
    val settings = Settings.load(this)
    val body = Subs.body(this, l.sub) ?: return false
    fun probe(names: List<String>, exit: Selection?): Map<String, Int> = runCatching {
        CaRoots.ensure()
        val tune = Tune.json(this, settings, chain = exit != null, exit = exit, servers = false).toString()
        val json = JSONObject(Native.ping(this, body, JSONArray(names).toString(), PingMode.Proxy.code, names.size.coerceIn(1, AUTO_SWITCH_CANDIDATES), (settings.pingTimeout * 1000).coerceAtLeast(5000), null, settings.pingUrl, tune))
        names.associateWith { (json.opt(it) as? Number)?.toInt() ?: -1 }
    }.getOrDefault(emptyMap())
    if (!verified && (probe(listOf(l.profile), Selection(l.sub, l.profile))[l.profile] ?: -1) >= 0) {
        Diag.event(this, "vpn", "state" to "auto-switch", "result" to "server answers")
        switchMissed = "server answers"
        return false
    }
    val pings = Pings.forSub(this, l.sub).results
    val netMode = AutoSelect.load(this)?.mode
    val title = Subs.list(this).firstOrNull { it.id == l.sub }?.info?.title
    val others = Subscription.parse(body).filter {
        it.supported && it.remarks != l.profile &&
            !AutoSelect.heldBack(settings, netMode ?: NetMode.Free, AutoSelect.reserve(settings, l.sub, it))
    }
    val lastResort = if (netMode == NetMode.Whitelist) {
        emptySet()
    } else {
        val learned = AutoSelect.learned(this)
        others.filter { forWhitelists(settings, l.sub, title, it, learned) }.map { it.remarks }.toSet()
    }
    var best: String? = null
    for (round in failoverRounds(others.map { it.remarks }, pings, lastResort, AUTO_SWITCH_CANDIDATES)) {
        if (gen != generation) break
        val res = probe(round, null)
        best = round.filter { (res[it] ?: -1) >= 0 }.minByOrNull { res[it] ?: Int.MAX_VALUE }
        if (best != null) break
    }
    if (best == null || gen != generation || (live != null && live !== l)) {
        switchMissed = if (best == null) "no candidates" else "superseded"
        Diag.event(this, "vpn", "state" to "auto-switch", "result" to switchMissed)
        return false
    }
    Diag.event(this, "vpn", "state" to "auto-switch", "from" to l.profile, "to" to best, "why" to why)
    main.post {
        if (gen != generation) return@post
        Subs.select(this, Selection(l.sub, best))
        if (settings.healthNotify) notifySwitched(l.profile, best, why)
        connect(Selection(l.sub, best), retry)
    }
    return true
}
