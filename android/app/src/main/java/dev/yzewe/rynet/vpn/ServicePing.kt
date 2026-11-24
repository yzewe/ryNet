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
import dev.yzewe.rynet.vpn.RynetService.Companion.PING_HOSTS_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.PING_STUCK_MS
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

internal fun RynetService.ping(sub: String, startId: Int, only: String? = null, names: List<String>? = null) {
    if (names != null && names.isEmpty()) return
    val now = SystemClock.elapsedRealtime()
    if (pinging && now - pingSince < PING_STUCK_MS) return
    pinging = true
    pingSince = now
    pingingSub = sub
    val gen = ++pingGen
    val prev = Pings.forSub(this, sub).restart(only, names)
    Pings.publish(this, prev)
    Thread({
        val parsed = runCatching {
            val config = Subs.body(this, sub) ?: error(tr("Нет подписки", "No subscription"))
            val all = Subscription.parse(config).let { if (Subs.split(this, sub)) Subscription.split(it) else it }
            val targets = when {
                names != null -> all.filter { it.remarks in names }
                only == null -> all
                only.endsWith(Profile.MEMBER_SEP) -> all.byRemarks(only.dropLast(1))?.members.orEmpty()
                else -> listOfNotNull(all.byRemarks(only))
            }.filter { it.supported }
            val names = targets.map { it.remarks }
            if (synchronized(lock) { tun != null || handle != 0L }) {
                Native.addHosts(resolveHosts(targets.flatMap { it.hosts }.distinct(), budgetMs = PING_HOSTS_MS).toString())
            }
            val settings = Settings.load(this)
            val live = java.util.concurrent.ConcurrentHashMap<String, Pair<Int, String?>>()
            var shownAt = 0L
            val sink = PingSink { name, ms, error ->
                live[name] = if (ms >= 0) ms to null else Pings.classify(error ?: "")
                val now = android.os.SystemClock.elapsedRealtime()
                if (gen == pingGen && (now - shownAt >= 150 || live.size == names.size)) {
                    shownAt = now
                    Pings.publish(
                        this,
                        Pings(
                            sub,
                            prev.results + live.mapValues { it.value.first },
                            running = true,
                            errors = prev.errors.filterKeys { !live.containsKey(it) } +
                                live.mapNotNull { (k, v) -> v.second?.let { k to it } }.toMap(),
                            done = live.size,
                            total = names.size,
                            target = only,
                            targets = names
                        )
                    )
                }
            }
            CaRoots.ensure()
            val json = JSONObject(
                Native.ping(
                    if (handle != 0L) this else null, config, JSONArray(names).toString(), settings.pingMode.code,
                    settings.pingParallel, settings.pingTimeout * 1000, sink, settings.pingUrl,
                    Tune.json(this, settings, chain = false, exit = null, servers = false).toString()
                )
            )
            json.keys().asSequence().associateWith { Pings.classify(json.opt(it)) }
        }.getOrElse { emptyMap() }
        if (gen != pingGen) return@Thread
        val endpoints = Countries.endpoints(Subs.profiles(this, sub))
        val ask = Settings.load(this).takeIf { it.exitFlags }?.let { s ->
            val known = Countries.load(this, sub, endpoints)
            parsed.filter { (k, v) -> v.first >= 0 && k !in known }.keys.toList().takeIf { it.isNotEmpty() }?.let { s to it }
        }
        Pings.publish(
            this,
            Pings(
                sub,
                prev.results + parsed.mapValues { it.value.first },
                running = false,
                errors = prev.errors.filterKeys { it !in parsed } +
                    parsed.mapNotNull { (k, v) -> v.second?.let { k to it } }.toMap()
            )
        )
        pinging = false
        val next = pingQueue.poll()
        if (next != null) main.post { ping(next, startId) }
        if (ask != null) {
            geo.incrementAndGet()
            runCatching {
                val (s, names) = ask
                val config = Subs.body(this, sub) ?: return@runCatching
                val json = JSONObject(
                    Native.countries(
                        if (handle != 0L) this else null,
                        config,
                        JSONArray(names).toString(),
                        s.pingParallel,
                        s.pingTimeout * 1000,
                        Tune.json(this, s, chain = false, exit = null, servers = false).toString()
                    )
                )
                Countries.put(this, sub, json.keys().asSequence().associateWith { json.getString(it) }, endpoints)
            }
            geo.decrementAndGet()
        }
        if (next == null && idle()) stopSelf(startId)
    }, "rynet-ping").start()
}
