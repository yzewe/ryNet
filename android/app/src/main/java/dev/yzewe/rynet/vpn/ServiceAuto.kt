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
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_AUTO_UNDO
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO_DEEP_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO_WHITELIST_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_PROFILE
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_SUB
import dev.yzewe.rynet.vpn.RynetService.Companion.PING_HOSTS_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.underlying
import dev.yzewe.rynet.vpn.RynetService.Live
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

internal fun RynetService.forWhitelists(s: Settings, sub: String, title: String?, p: Profile, learned: AutoSelect.Learned): Boolean {
    val yes = AutoSelect.sig(p) in learned.yes
    val mark = if (s.autoWlMarked) s.isAutoWl(sub, p.remarks) || Profile.parentOf(p.remarks)?.let { s.isAutoWl(sub, it) } == true else null
    return (mark ?: (yes || AutoSelect.namedForWhitelists(p.remarks, title))) || yes
}

internal fun RynetService.autoDelay(s: Settings, mode: NetMode?): Long? {
    val full = s.autoInterval.takeIf { it > 0 }?.let { it * 60_000L }
    return if (mode == NetMode.Whitelist && s.autoWhitelist == WhitelistMode.Auto) minOf(full ?: Long.MAX_VALUE, AUTO_WHITELIST_MS) else full
}

internal fun RynetService.autoPrepare(names: List<String>) {
    if (synchronized(lock) { tun != null || handle != 0L }) Native.addHosts(resolveHosts(names, budgetMs = PING_HOSTS_MS).toString())
}

internal fun RynetService.autoService(): VpnService? = if (synchronized(lock) { tun != null || handle != 0L }) this else null

internal fun RynetService.autoPick(requested: Selection, gen: Int): Selection {
    val s = Settings.load(this)
    val t0 = SystemClock.elapsedRealtime()
    autoWasOn = true
    val prev = AutoSelect.load(this)
    AutoSelect.publish(this, (prev ?: AutoState()).copy(running = true))
    val st = runCatching {
        AutoSelect.evaluate(this, s, AutoSelect.Run(underlying(), requested, quick = true, service = autoService(), prepare = ::autoPrepare, alive = { gen == generation }))
    }.onFailure { Diag.event(this, "auto", "state" to "error", "error" to (it.message ?: it.toString())) }.getOrNull()
    autoFullAt = SystemClock.elapsedRealtime()
    if (st == null) {
        AutoSelect.publish(this, (prev ?: AutoState()).copy(running = false))
        return requested
    }
    AutoSelect.publish(this, st.copy(next = System.currentTimeMillis() + (autoDelay(s, st.mode) ?: 0L).coerceAtMost(24 * 3_600_000L)))
    val to = st.chosen ?: return requested
    Diag.event(this, "auto", "state" to "picked", "mode" to st.mode.key, "to" to to.profile, "pool" to st.total, "ms" to SystemClock.elapsedRealtime() - t0)
    if (gen == generation && to != requested) {
        Subs.select(this, to)
        VpnState.publish(this, VpnState(Status.Connecting, to.profile))
    }
    return to
}

internal fun RynetService.autoCheck(why: AutoWhy, error: String? = null) {
    val l = live ?: return
    val gen = generation
    if (!autoRunning.compareAndSet(false, true)) {
        if (why != AutoWhy.Periodic) autoAgain = why
        return
    }
    Thread({
        try {
            autoRun(l, gen, why, error)
        } catch (e: Throwable) {
            Diag.event(this, "auto", "state" to "error", "error" to (e.message ?: e.toString()))
            main.post { if (gen == generation) schedule(Settings.load(this)) }
        } finally {
            AutoSelect.load(this)?.takeIf { it.running }?.let { AutoSelect.publish(this, it.copy(running = false)) }
            autoRunning.set(false)
            autoAgain?.let { again ->
                autoAgain = null
                main.post { autoCheck(again) }
            }
        }
    }, "rynet-auto").start()
}

internal fun RynetService.autoRun(l: Live, gen: Int, why: AutoWhy, error: String?) {
    val s = Settings.load(this)
    if (!s.autoSelect) return
    val failed = why == AutoWhy.Failing || why == AutoWhy.Dead
    val prev = AutoSelect.load(this) ?: AutoState()
    fun later() {
        main.post {
            if (gen != generation) return@post
            main.removeCallbacks(autoTick)
            autoDelay(Settings.load(this), AutoSelect.load(this)?.mode)?.let { main.postDelayed(autoTick, it) }
        }
    }
    if (!failed && why != AutoWhy.Asked && getSystemService(PowerManager::class.java).isDeviceIdleMode) return later()
    if (why == AutoWhy.Periodic && !getSystemService(PowerManager::class.java).isInteractive) {
        if (!autoDeferred) Diag.event(this, "auto", "state" to "deferred")
        autoDeferred = true
        AutoSelect.publish(this, prev.copy(next = System.currentTimeMillis() + (autoDelay(s, prev.mode) ?: 0L)))
        return later()
    }
    autoDeferred = false
    val net = underlying()
    if (why == AutoWhy.Periodic && prev.mode == NetMode.Whitelist) {
        val dueFull = s.autoInterval > 0 && SystemClock.elapsedRealtime() - autoFullAt >= s.autoInterval * 60_000L
        if (!dueFull && !AutoSelect.freeAgain(s, net)) {
            AutoSelect.publish(this, prev.copy(next = System.currentTimeMillis() + (autoDelay(s, prev.mode) ?: 0L)))
            return later()
        }
    }
    val current = Selection(l.sub, l.profile)
    AutoSelect.publish(this, prev.copy(running = true))
    val light = why == AutoWhy.Periodic && SystemClock.elapsedRealtime() - autoDeepAt < AUTO_DEEP_MS
    val st = AutoSelect.evaluate(this, s, AutoSelect.Run(net, current, failed = failed, light = light, service = autoService(), prepare = ::autoPrepare, alive = { gen == generation }))
    trimSoon()
    if (!light) autoDeepAt = SystemClock.elapsedRealtime()
    autoFullAt = SystemClock.elapsedRealtime()
    if (gen != generation) return
    AutoSelect.publish(this, st.copy(next = System.currentTimeMillis() + (autoDelay(s, st.mode) ?: 0L)))
    val to = st.chosen
    val held = !failed && why != AutoWhy.Asked && SystemClock.elapsedRealtime() < autoHoldUntil && st.mode == prev.mode
    Diag.event(this, "auto", "state" to "checked", "why" to why.name.lowercase(), "mode" to st.mode.key, "from" to l.profile, "to" to (to?.profile ?: ""), "held" to held, "light" to light)
    if (to == null || to == current || held) {
        if (to == null && failed) {
            main.post {
                if (gen != generation) return@post
                if (s.healthNotify && error != null) notifyDead(error)
                connect(current)
            }
            return
        }
        return later()
    }
    main.post {
        if (gen != generation) return@post
        Subs.select(this, to)
        if (s.autoNotify) notifyAuto(prev, st, current, to, failed)
        connect(to)
    }
}

internal fun RynetService.autoLook(startId: Int) {
    if (!autoRunning.compareAndSet(false, true)) return
    Thread({
        try {
            val s = Settings.load(this)
            val prev = AutoSelect.load(this) ?: AutoState()
            AutoSelect.publish(this, prev.copy(running = true))
            val st = runCatching { AutoSelect.evaluate(this, s, AutoSelect.Run(underlying(), Subs.selection(this), service = autoService(), prepare = ::autoPrepare)) }.getOrNull()
            AutoSelect.publish(this, st ?: prev.copy(running = false))
            trimSoon()
        } finally {
            autoRunning.set(false)
            val again = autoAgain.also { autoAgain = null }
            main.post {
                if (again != null) autoCheck(again)
                if (idle()) stopSelf(startId)
            }
        }
    }, "rynet-auto").start()
}

internal fun RynetService.notifyAuto(prev: AutoState, st: AutoState, from: Selection, to: Selection, failed: Boolean) {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(AUTO) == null) {
        nm.createNotificationChannel(NotificationChannel(AUTO, tr("Автовыбор", "Auto select"), NotificationManager.IMPORTANCE_LOW))
    }
    val c = st.candidates
    val toName = c.firstOrNull { it.selection == to }?.let { listOf(it.flag, it.name).filter(String::isNotBlank).joinToString(" ") } ?: to.profile
    val toMs = c.firstOrNull { it.selection == to }?.ms?.takeIf { it >= 0 }?.let { tr(" ($it мс)", " ($it ms)") }.orEmpty()
    val fromName = c.firstOrNull { it.selection == from }?.name ?: from.profile
    val fromMs = c.firstOrNull { it.selection == from }?.ms?.takeIf { it >= 0 }
    val (title, text) = when {
        st.mode == NetMode.Whitelist && prev.mode != NetMode.Whitelist ->
            tr("Похоже, включили белые списки", "Looks like whitelists are on") to
                tr("Зарубежные сайты не открываются напрямую. Переключился на «$toName»$toMs.", "Foreign sites don't open directly. Switched to \"$toName\"$toMs.")

        st.mode == NetMode.Free && prev.mode == NetMode.Whitelist ->
            tr("Обычный интернет вернулся", "Open internet is back") to
                tr("Зарубежные сайты снова открываются. Переключился на «$toName»$toMs.", "Foreign sites open again. Switched to \"$toName\"$toMs.")

        failed -> tr("«$fromName» не отвечает", "\"$fromName\" isn't responding") to tr("Переключился на «$toName»$toMs.", "Switched to \"$toName\"$toMs.")

        else -> tr("Нашёлся сервер быстрее", "Found a faster server") to
            (
                fromMs?.let { tr("«$toName»$toMs против $it мс у «$fromName». Переключился.", "\"$toName\"$toMs vs $it ms on \"$fromName\". Switched.") }
                    ?: tr("Переключился на «$toName»$toMs.", "Switched to \"$toName\"$toMs.")
                )
    }
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    val undo = PendingIntent.getService(
        this,
        5,
        Intent(this, RynetService::class.java).setAction(ACTION_AUTO_UNDO).putExtra(EXTRA_SUB, from.sub).putExtra(EXTRA_PROFILE, from.profile),
        flags
    )
    val why = PendingIntent.getActivity(this, 6, Intent(this, HomeActivity::class.java).putExtra(HomeActivity.EXTRA_AUTO_WHY, true).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), flags)
    val n = builder(AUTO)
        .setSmallIcon(notifIcon())
        .setContentTitle(title)
        .setContentText(text)
        .setStyle(Notification.BigTextStyle().bigText(text))
        .setContentIntent(why)
        .setAutoCancel(true)
        .apply {
            if (!failed) addAction(Notification.Action.Builder(null, tr("Вернуть", "Undo"), undo).build())
            addAction(Notification.Action.Builder(null, tr("Почему?", "Why?"), why).build())
        }
        .build()
    nm.notify(AUTO_ID, n)
}
