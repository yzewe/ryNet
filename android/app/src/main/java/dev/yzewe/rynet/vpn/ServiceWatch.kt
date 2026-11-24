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
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_LOCK_OFF
import dev.yzewe.rynet.vpn.RynetService.Companion.AUTO_NET_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.LOCK_OFF_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.NOTIFICATION_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.WIFI_SETTLE_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.underlying
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

internal fun RynetService.watchNetwork() {
    if (netCallback != null) return
    val cm = getSystemService(ConnectivityManager::class.java)
    var current: Network? = null
    val onBest = { network: Network ->
        if (current != null && current != network) {
            val dns = systemDns(network).toString()
            synchronized(lock) { if (handle != 0L) Native.networkChanged(handle, dns) }
            Diag.event(this@watchNetwork, "net", "state" to "changed")
            main.removeCallbacks(autoNet)
            if (Settings.load(this@watchNetwork).autoSelect) main.postDelayed(autoNet, AUTO_NET_MS)
        }
        current = network
        underlying = network
    }
    val request = NetworkRequest.Builder()
        .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
        .build()
    val cb = if (Build.VERSION.SDK_INT >= 31) {
        object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = onBest(network)
        }
    } else {
        object : ConnectivityManager.NetworkCallback() {
            private val seen = HashMap<Network, NetworkCapabilities>()
            private fun pick() {
                seen.entries.maxByOrNull { (_, c) ->
                    (if (c.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) 2 else 0) +
                        (if (c.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)) 1 else 0)
                }?.let { onBest(it.key) }
            }
            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                seen[network] = caps
                pick()
            }
            override fun onLost(network: Network) {
                seen.remove(network)
                pick()
            }
        }
    }
    runCatching {
        if (Build.VERSION.SDK_INT >= 31) {
            cm.registerBestMatchingNetworkCallback(request, cb, Handler(Looper.getMainLooper()))
        } else if (Build.VERSION.SDK_INT >= 26) {
            cm.registerNetworkCallback(request, cb, Handler(Looper.getMainLooper()))
        } else {
            cm.registerNetworkCallback(request, cb)
        }
    }.onSuccess { netCallback = cb }
}

internal fun RynetService.lockAlarm(): PendingIntent = PendingIntent.getService(this, 7, Intent(this, RynetService::class.java).setAction(ACTION_LOCK_OFF), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)

internal fun RynetService.watchLock() {
    if (lockWatch != null) return
    val r = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            val am = getSystemService(AlarmManager::class.java) ?: return
            when (i.action) {
                Intent.ACTION_SCREEN_OFF -> am.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + LOCK_OFF_MS, lockAlarm())
                Intent.ACTION_USER_PRESENT -> am.cancel(lockAlarm())
                Intent.ACTION_SCREEN_ON -> if (getSystemService(KeyguardManager::class.java)?.isKeyguardLocked != true) am.cancel(lockAlarm())
            }
        }
    }
    val filter = IntentFilter().apply {
        addAction(Intent.ACTION_SCREEN_OFF)
        addAction(Intent.ACTION_SCREEN_ON)
        addAction(Intent.ACTION_USER_PRESENT)
    }
    runCatching { if (Build.VERSION.SDK_INT >= 33) registerReceiver(r, filter, Context.RECEIVER_NOT_EXPORTED) else registerReceiver(r, filter) }.onSuccess { lockWatch = r }
}

internal fun RynetService.unwatchLock() {
    lockWatch?.let { runCatching { unregisterReceiver(it) } }
    lockWatch = null
    runCatching { getSystemService(AlarmManager::class.java)?.cancel(lockAlarm()) }
}

internal fun RynetService.watchWifi(settings: Settings) {
    if (wifiWatch != null || !settings.wifiPause) return
    val cm = getSystemService(ConnectivityManager::class.java)
    val request = NetworkRequest.Builder()
        .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
        .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
        .build()
    val cb = object : ConnectivityManager.NetworkCallback() {
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            val ok = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            main.post { if (if (ok) wifiNets.add(network) else wifiNets.remove(network)) settleWifi() }
        }
        override fun onLost(network: Network) {
            main.post { if (wifiNets.remove(network)) settleWifi() }
        }
    }
    runCatching { cm.registerNetworkCallback(request, cb) }.onSuccess { wifiWatch = cb }
    main.postDelayed(wifiCheck, WIFI_SETTLE_MS)
}

internal fun RynetService.settleWifi() {
    main.removeCallbacks(wifiCheck)
    main.postDelayed(wifiCheck, if (onWifi == null) 0L else WIFI_SETTLE_MS)
}

internal fun RynetService.unwatchWifi() {
    main.removeCallbacks(wifiCheck)
    wifiWatch?.let { cb -> runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(cb) } }
    wifiWatch = null
    wifiNets.clear()
    onWifi = null
    wifiPaused = null
}

internal fun RynetService.checkWifi() {
    val now = wifiNets.isNotEmpty()
    val was = onWifi
    onWifi = now
    if (was == null || was == now || destroyed) return
    if (now) {
        val l = live ?: return
        if (Build.VERSION.SDK_INT >= 29 && isLockdownEnabled) return
        val sel = Selection(l.sub, l.profile)
        wifiPaused = sel
        generation++
        stopCore()
        closeTun()
        Diag.event(this, "vpn", "state" to "off", "reason" to "wifi")
        VpnState.publish(this, VpnState(Status.Off))
        val n = pausedNotification(sel, wifi = true)
        runCatching {
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(NOTIFICATION_ID, n, fgsType())
            } else {
                startForeground(NOTIFICATION_ID, n)
            }
        }
    } else {
        val sel = wifiPaused ?: return
        wifiPaused = null
        connect(sel)
    }
}
