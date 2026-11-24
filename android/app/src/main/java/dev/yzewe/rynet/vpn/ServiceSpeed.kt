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

internal fun RynetService.speedtest(sel: Selection, startId: Int) {
    if (speeding) return
    speeding = true
    speedCancel = false
    Native.speedtestReset()
    Thread({
        var run = SpeedRun(SpeedRun.Phase.Servers)
        SpeedTest.publish(this, run)
        val samples = ArrayList<Double>()
        var shownAt = 0L
        try {
            val config = Subs.body(this, sel.sub) ?: error(tr("Нет подписки", "No subscription"))
            val profile = Subscription.parse(config).byRemarks(sel.profile) ?: error(tr("Нет сервера «${sel.profile}»", "No server \"${sel.profile}\""))
            val settings = Settings.load(this)
            if (synchronized(lock) { tun != null || handle != 0L }) {
                val hosts = profile.hosts + (Tune.entry(this, settings, sel)?.hosts ?: emptyList())
                Native.addHosts(resolveHosts(hosts.distinct()).toString())
            }
            CaRoots.ensure()
            val sink = object : SpeedSink {
                override fun server(json: String) {
                    val s = runCatching { JSONObject(json).getJSONObject("server") }.getOrNull() ?: return
                    run = run.copy(server = listOf(s.optString("sponsor"), SpeedTest.place(s)).filter { it.isNotBlank() }.joinToString(" · "))
                    SpeedTest.publish(this@speedtest, run)
                }
                override fun progress(phase: Int, value: Double, fraction: Double) {
                    run = when (phase) {
                        0 -> run.copy(phase = SpeedRun.Phase.Servers, value = 0.0, fraction = 0.0)

                        1 -> {
                            if (value > 0) samples += value
                            run.copy(
                                phase = SpeedRun.Phase.Ping,
                                value = value,
                                fraction = fraction,
                                ping = samples.minOrNull(),
                                jitter = samples.zipWithNext { a, b -> kotlin.math.abs(b - a) }.takeIf { it.isNotEmpty() }?.average()
                            )
                        }

                        2 -> run.copy(phase = SpeedRun.Phase.Download, value = value, fraction = fraction, download = if (fraction >= 1.0) value else run.download)

                        else -> run.copy(phase = SpeedRun.Phase.Upload, value = value, fraction = fraction, upload = if (fraction >= 1.0) value else run.upload)
                    }
                    val now = SystemClock.elapsedRealtime()
                    if (now - shownAt >= 100 || fraction >= 1.0) {
                        shownAt = now
                        SpeedTest.publish(this@speedtest, run)
                    }
                }
            }
            val json = JSONObject(
                Native.speedtest(
                    if (handle != 0L) this else null,
                    config,
                    sel.profile,
                    Tune.json(this, settings, chain = true, exit = sel, servers = false).toString(),
                    sink
                )
            )
            val server = json.optJSONObject("server") ?: JSONObject()
            val result = SpeedResult(
                at = System.currentTimeMillis(),
                name = profile.name, flag = profile.flag,
                target = SpeedTest.place(server),
                sponsor = server.optString("sponsor"),
                ping = json.optDouble("ping"), jitter = json.optDouble("jitter"),
                download = json.optDouble("download"), upload = json.optDouble("upload")
            )
            SpeedTest.add(this, result)
            run = run.copy(
                phase = SpeedRun.Phase.Done,
                fraction = 1.0,
                result = result,
                ping = result.ping,
                jitter = result.jitter,
                download = result.download,
                upload = result.upload
            )
        } catch (e: Throwable) {
            run = if (speedCancel) SpeedRun() else run.copy(phase = SpeedRun.Phase.Error, error = e.message?.ifBlank { null } ?: tr("Не получилось", "Something went wrong"))
        }
        SpeedTest.publish(this, run)
        speeding = false
        if (idle()) stopSelf(startId)
    }, "rynet-speedtest").start()
}
