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
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_CONNECT
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_DISCONNECT
import dev.yzewe.rynet.vpn.RynetService.Companion.ACTION_PAUSE
import dev.yzewe.rynet.vpn.RynetService.Companion.ALERTS
import dev.yzewe.rynet.vpn.RynetService.Companion.ALERT_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.BRAND_ACCENT
import dev.yzewe.rynet.vpn.RynetService.Companion.CHANNEL
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_PROFILE
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_REASON
import dev.yzewe.rynet.vpn.RynetService.Companion.EXTRA_SUB
import dev.yzewe.rynet.vpn.RynetService.Companion.FAILING_ALERT_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.FAILING_ALERT_MS
import dev.yzewe.rynet.vpn.RynetService.Companion.NOTIFICATION_ID
import dev.yzewe.rynet.vpn.RynetService.Companion.PAUSED
import dev.yzewe.rynet.vpn.RynetService.Companion.PAUSE_ID
import dev.yzewe.rynet.vpn.RynetService.Details
import dev.yzewe.rynet.vpn.RynetService.Live
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

internal fun RynetService.builder(channel: String): Notification.Builder = (
    if (Build.VERSION.SDK_INT >= 26) {
        Notification.Builder(this, channel)
    } else {
        @Suppress("DEPRECATION")
        Notification.Builder(
            this
        )
    }
    )
    .apply { accent(Settings.load(this@builder))?.let(::setColor) }

internal fun RynetService.accent(s: Settings): Int? = when {
    s.wallpaperColors && Build.VERSION.SDK_INT >= 31 -> null
    s.seedColor != 0 -> Seed.accent(s.seedColor)
    else -> BRAND_ACCENT
}

internal fun RynetService.notifyPaused(sel: Selection) {
    getSystemService(NotificationManager::class.java).notify(PAUSE_ID, pausedNotification(sel, wifi = false))
}

internal fun RynetService.pausedNotification(sel: Selection, wifi: Boolean): Notification {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(PAUSED) == null) {
        nm.createNotificationChannel(NotificationChannel(PAUSED, tr("Пауза", "Pause"), NotificationManager.IMPORTANCE_LOW))
    }
    val resume = Intent(this, RynetService::class.java).setAction(ACTION_CONNECT)
        .putExtra(EXTRA_SUB, sel.sub).putExtra(EXTRA_PROFILE, sel.profile)
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    val go = if (Build.VERSION.SDK_INT >= 26) PendingIntent.getForegroundService(this, 3, resume, flags) else PendingIntent.getService(this, 3, resume, flags)
    val open = PendingIntent.getActivity(this, 0, Intent(this, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
    val b = builder(PAUSED)
        .setSmallIcon(notifIcon())
        .setContentTitle(if (wifi) tr("VPN на паузе · Wi-Fi", "VPN paused · Wi-Fi") else tr("VPN на паузе", "VPN paused"))
        .setContentIntent(open)
        .setWhen(System.currentTimeMillis())
        .setShowWhen(true)
        .setUsesChronometer(true)
        .addAction(Notification.Action.Builder(null, tr("Продолжить", "Resume"), go).build())
    if (wifi) {
        b.setContentText(tr("Включится снова, когда телефон уйдёт с Wi-Fi", "Comes back when the phone leaves Wi-Fi"))
        val off = PendingIntent.getService(
            this,
            4,
            Intent(this, RynetService::class.java).setAction(ACTION_DISCONNECT).putExtra(EXTRA_REASON, "notification"),
            PendingIntent.FLAG_IMMUTABLE
        )
        b.addAction(Notification.Action.Builder(null, tr("Выключить", "Turn off"), off).build())
    }
    return b.build()
}

internal fun RynetService.notifySwitched(from: String, to: String, why: String) {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(ALERTS) == null) {
        nm.createNotificationChannel(NotificationChannel(ALERTS, tr("Сервер недоступен", "Server unavailable"), NotificationManager.IMPORTANCE_DEFAULT))
    }
    val open = PendingIntent.getActivity(this, 0, Intent(this, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
    val n = builder(ALERTS)
        .setSmallIcon(notifIcon())
        .setContentTitle(tr("«$from» не отвечает — переключились на «$to»", "\"$from\" isn't responding — switched to \"$to\""))
        .setContentText(why)
        .setContentIntent(open)
        .setAutoCancel(true)
        .build()
    nm.notify(ALERT_ID, n)
}

internal fun RynetService.notifyStillFailing(l: Live) {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(ALERTS) == null) {
        nm.createNotificationChannel(NotificationChannel(ALERTS, tr("Сервер недоступен", "Server unavailable"), NotificationManager.IMPORTANCE_DEFAULT))
    }
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    val pick = PendingIntent.getActivity(
        this,
        9,
        Intent(this, HomeActivity::class.java).putExtra(HomeActivity.EXTRA_SERVERS, true).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        flags
    )
    val again = Intent(this, RynetService::class.java).setAction(ACTION_CONNECT).putExtra(EXTRA_SUB, l.sub).putExtra(EXTRA_PROFILE, l.profile)
    val retry = if (Build.VERSION.SDK_INT >= 26) PendingIntent.getForegroundService(this, 10, again, flags) else PendingIntent.getService(this, 10, again, flags)
    val minutes = (FAILING_ALERT_MS / 60_000L).toInt()
    val text = if (Settings.load(this).autoSelect) {
        tr(
            "Соединения через него не проходят уже $minutes мин, а автовыбор не нашёл замену. Выберите другой сервер.",
            "Nothing has gone through it for $minutes min, and auto select found no replacement. Pick another server."
        )
    } else {
        tr(
            "Соединения через него не проходят уже $minutes мин. Выберите другой сервер или включите автовыбор.",
            "Nothing has gone through it for $minutes min. Pick another server or turn on auto select."
        )
    }
    val n = builder(ALERTS)
        .setSmallIcon(notifIcon())
        .setContentTitle(tr("«${l.title}» не отвечает", "\"${l.title}\" isn't responding"))
        .setContentText(text)
        .setStyle(Notification.BigTextStyle().bigText(text))
        .setContentIntent(pick)
        .setAutoCancel(true)
        .addAction(Notification.Action.Builder(null, tr("Повторить", "Retry"), retry).build())
        .addAction(Notification.Action.Builder(null, tr("Другой сервер", "Another server"), pick).build())
        .build()
    Diag.event(this, "vpn", "state" to "failing_alert", "profile" to l.profile)
    nm.notify(FAILING_ALERT_ID, n)
}

internal fun RynetService.notifyDead(why: String) {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(ALERTS) == null) {
        nm.createNotificationChannel(NotificationChannel(ALERTS, tr("Сервер недоступен", "Server unavailable"), NotificationManager.IMPORTANCE_DEFAULT))
    }
    val open = PendingIntent.getActivity(this, 0, Intent(this, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
    val n = builder(ALERTS)
        .setSmallIcon(notifIcon())
        .setContentTitle(tr("Сервер не отвечает — переподключаюсь", "Server isn't responding — reconnecting"))
        .setContentText(why)
        .setStyle(Notification.BigTextStyle().bigText(why))
        .setContentIntent(open)
        .setAutoCancel(true)
        .build()
    nm.notify(ALERT_ID, n)
}

internal fun RynetService.details(l: Live): Details {
    val stats = (statsMap ?: CoreStats.open(this)?.also { statsMap = it })?.read()
    val prev = prevStats
    prevStats = stats
    val (down, up) = if (stats != null && prev != null && stats.generation == prev.generation && stats.at > prev.at) {
        val sec = (stats.at - prev.at) / 1000.0
        ((stats.down - prev.down) / sec).toLong().coerceAtLeast(0) to ((stats.up - prev.up) / sec).toLong().coerceAtLeast(0)
    } else {
        0L to 0L
    }
    val ping = lastPing?.let { tr(" · $it мс", " · $it ms") }.orEmpty()
    val text = "↓ ${speed(down)} · ↑ ${speed(up)}$ping"
    val big = stats?.let { text + "\n" + tr("Принято ${size(it.down)} · отдано ${size(it.up)}", "Received ${size(it.down)} · sent ${size(it.up)}") }
    return Details(l.title, text, big)
}

internal fun RynetService.notifIcon(): Int = iconRes.takeIf { it != 0 } ?: AppIcon.notif(this).also { iconRes = it }

internal fun RynetService.notification(connected: Boolean, blocked: Boolean = false, details: Details? = null): Notification {
    val open = PendingIntent.getActivity(this, 0, Intent(this, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
    val stop = PendingIntent.getService(
        this,
        1,
        Intent(this, RynetService::class.java).setAction(ACTION_DISCONNECT).putExtra(EXTRA_REASON, "notification"),
        PendingIntent.FLAG_IMMUTABLE
    )
    val b = builder(CHANNEL)
        .setSmallIcon(notifIcon())
        .setContentTitle(if (blocked) tr("Интернет заблокирован: VPN не подключён", "Internet blocked: VPN not connected") else getString(if (connected) R.string.status_on else R.string.status_connecting))
        .setContentIntent(open)
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setShowWhen(connected)
    if (connected && !blocked) {
        val pause = PendingIntent.getService(this, 2, Intent(this, RynetService::class.java).setAction(ACTION_PAUSE), PendingIntent.FLAG_IMMUTABLE)
        b.addAction(Notification.Action.Builder(null, tr("Пауза", "Pause"), pause).build())
    }
    b.addAction(Notification.Action.Builder(null, getString(R.string.disconnect), stop).build())
    val l = live
    if (connected && !blocked && l != null && l.since > 0) b.setWhen(l.since).setUsesChronometer(true)
    val settings = Settings.load(this)
    promoted = connected && !blocked && settings.requestsLiveUpdate(Build.VERSION.SDK_INT)
    if (promoted) {
        try {
            b.setRequestPromotedOngoing(true)
        } catch (_: NoSuchMethodError) {
            b.addExtras(android.os.Bundle().apply { putBoolean(Notification.EXTRA_REQUEST_PROMOTED_ONGOING, true) })
        }
    }
    if (connected && !blocked && l != null && !settings.notifyDetails && settings.requestsLiveUpdate(Build.VERSION.SDK_INT) && l.title.isNotBlank()) b.setContentText(l.title)
    if (connected && l != null && settings.notifyDetails) {
        val d = details ?: details(l)
        b.setContentTitle(d.title).setContentText(d.text).setSubText(getString(R.string.status_on))
        d.big?.let { b.setStyle(Notification.BigTextStyle().bigText(it)) }
    }
    failingNotice(b, connected && !blocked)
    return b.build()
}

internal fun RynetService.fgsType(): Int = if (VpnService.prepare(this) == null) ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED else ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE

internal fun RynetService.startInForeground(connected: Boolean, blocked: Boolean = false) {
    val nm = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_vpn), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            }
        )
    }
    val n = notification(connected, blocked)
    lastShown = null
    shownAs = connected to blocked
    if (Build.VERSION.SDK_INT >= 34) {
        startForeground(NOTIFICATION_ID, n, fgsType())
    } else {
        startForeground(NOTIFICATION_ID, n)
    }
}

internal fun RynetService.size(b: Long): String = when {
    b >= 1L shl 30 -> tr("%.1f ГБ", "%.1f GB").format(Lang.locale, b / (1L shl 30).toDouble())
    b >= 1L shl 20 -> tr("%.1f МБ", "%.1f MB").format(Lang.locale, b / (1L shl 20).toDouble())
    b >= 1L shl 10 -> tr("%d КБ", "%d KB").format(Lang.locale, b shr 10)
    else -> tr("$b Б", "$b B")
}

internal fun RynetService.speed(bps: Long): String = size(bps) + tr("/с", "/s")
