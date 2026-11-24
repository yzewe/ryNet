package dev.yzewe.rynet.ui

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.currentStateAsState
import dev.yzewe.rynet.data.AutoSelect
import dev.yzewe.rynet.data.Countries
import dev.yzewe.rynet.data.DeepLink
import dev.yzewe.rynet.data.ExitIp
import dev.yzewe.rynet.data.ExpiryAlert
import dev.yzewe.rynet.data.Geo
import dev.yzewe.rynet.data.GeoWarn
import dev.yzewe.rynet.data.Hwid
import dev.yzewe.rynet.data.Local
import dev.yzewe.rynet.data.Pings
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.RuleSets
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.data.SubIcon
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.Subscription
import dev.yzewe.rynet.data.ThemeMode
import dev.yzewe.rynet.data.TvLink
import dev.yzewe.rynet.data.Usage
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.data.byRemarks
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.vpn.RynetService
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val pingIo = java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "rynet-pings").apply { isDaemon = true } }

open class HomeActivity : ComponentActivity() {
    private lateinit var state: UiState
    protected open val serversOnly: Boolean get() = false

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                VpnState.ACTION -> {
                    state.vpn = VpnState.load(context)
                    state.selection = Subs.selection(context)
                }

                Pings.ACTION -> if (pingsQueued.compareAndSet(false, true)) {
                    pingIo.execute {
                        pingsQueued.set(false)
                        val p = Pings.load(context)
                        runOnUiThread { state.updatePings(p) }
                    }
                }

                AutoSelect.ACTION -> state.auto = AutoSelect.load(context)

                Countries.ACTION -> state.reloadSubs(context)

                else -> state.reload(context)
            }
        }
    }
    private val pingsQueued = java.util.concurrent.atomic.AtomicBoolean(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        state = UiState(this)
        if (serversOnly) {
            state.screen = Screen.Servers
            state.shown = intent.getStringExtra(ServersActivity.EXTRA_SUB) ?: state.selection?.sub
        } else {
            state.incoming = serverLink(intent)
            state.incomingSub = subLink(intent)
            state.tvOffer = tvLink(intent)
            openSet(intent)
            openAuto(intent)
            openServerList(intent)
            openShortcut(intent)
            Shortcuts.publish(this)
        }
        setContent {
            AppFrame(state.settings) {
                val notices = remember(state.settings.dismissedNotices) {
                    Notices(state.settings.dismissedNotices) { key ->
                        val old = state.settings
                        val new = old.dismissNotice(key)
                        Settings.change(this, old, new)
                        state.settings = new
                    }
                }
                CompositionLocalProvider(LocalNotices provides notices) { App(state, serversOnly) }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        serverLink(intent)?.let { state.incoming = it }
        subLink(intent)?.let { state.incomingSub = it }
        tvLink(intent)?.let { state.tvOffer = it }
        openSet(intent)
        openAuto(intent)
        openServerList(intent)
        openShortcut(intent)
    }

    private fun openServerList(intent: Intent?) {
        if (intent?.getBooleanExtra(EXTRA_SERVERS, false) != true) return
        intent.removeExtra(EXTRA_SERVERS)
        if (state.subs.isEmpty()) return
        state.screen = Screen.Home
        state.sheet = Sheet.None
        startActivity(ServersActivity.intent(this, state.selection?.sub ?: state.subs.first().id))
    }

    private fun openShortcut(intent: Intent?) {
        when (intent?.getStringExtra(Shortcuts.EXTRA)) {
            Shortcuts.REFRESH -> state.refreshAll = true
            Shortcuts.SPEED -> state.speedNow = true
            else -> return
        }
        intent.removeExtra(Shortcuts.EXTRA)
    }

    private fun openAuto(intent: Intent?) {
        if (intent?.getBooleanExtra(EXTRA_AUTO_WHY, false) != true) return
        intent.removeExtra(EXTRA_AUTO_WHY)
        state.screen = Screen.Home
        state.sheet = Sheet.AutoWhy
    }

    private fun openSet(intent: Intent?) {
        val link = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString?.takeIf { RuleSets.isLink(it) } ?: return
        startActivity(SettingsActivity.importSet(this, link))
    }

    private fun serverLink(intent: Intent?): String? {
        val link = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString ?: return null
        return link.takeIf { Local.isServers(it) } ?: (DeepLink.parse(link) as? DeepLink.Target.Servers)?.text
    }

    private fun tvLink(intent: Intent?): TvLink.Offer? = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString?.let { TvLink.parse(it) }

    private fun subLink(intent: Intent?): DeepLink.Target.Subscription? = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString?.let { DeepLink.parse(it) as? DeepLink.Target.Subscription }

    override fun onStart() {
        super.onStart()
        fixStaleState()
        state.reload(this)
        state.selection = Subs.selection(this)
        if (Subs.list(this) != state.subs) state.reloadSubs(this)
        if (state.screen == Screen.AddSub && state.subs.isEmpty() && Subs.list(this).isNotEmpty()) {
            state.reloadSubs(this)
            state.screen = Screen.Home
        }
        val old = state.settings
        Settings.pruneSubs(this)
        val new = Settings.load(this)
        state.settings = new
        if (old.exitFlags != new.exitFlags) state.reloadSubs(this)
        if (old.tunnelDiffers(new) && (state.vpn.status == Status.On || state.vpn.status == Status.Connecting)) {
            state.selection?.let { RynetService.connect(this, it) }
        }
        state.auto = AutoSelect.load(this)
        val filter = IntentFilter(VpnState.ACTION).apply {
            addAction(Pings.ACTION)
            addAction(AutoSelect.ACTION)
            addAction(Countries.ACTION)
        }
        ContextCompat.registerReceiver(this, receiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
    }

    override fun onStop() {
        unregisterReceiver(receiver)
        super.onStop()
    }

    private fun fixStaleState() {
        val vpnProcess = getSystemService(ActivityManager::class.java).runningAppProcesses
            ?.any { it.processName == "$packageName:core" } == true
        if (!vpnProcess) Pings.load(this).takeIf { it.running }?.let { Pings.publish(this, it.copy(running = false)) }
        val s = VpnState.load(this)
        if (s.status != Status.On && s.status != Status.Connecting) return
        val cm = getSystemService(ConnectivityManager::class.java)

        @Suppress("DEPRECATION")
        val vpnUp = cm.allNetworks.any { cm.getNetworkCapabilities(it)?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true }
        if (!vpnProcess || !vpnUp && !Settings.load(this).proxyMode) VpnState.publish(this, VpnState(Status.Off))
    }

    companion object {
        const val EXTRA_AUTO_WHY = "auto_why"
        const val EXTRA_SERVERS = "servers"
    }
}

class ServersActivity : HomeActivity() {
    override val serversOnly: Boolean get() = true

    companion object {
        const val EXTRA_SUB = "sub"
        fun intent(ctx: Context, sub: String?): Intent = Intent(ctx, ServersActivity::class.java).putExtra(EXTRA_SUB, sub)
    }
}
