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

enum class Sheet { None, Entry, AutoWhy }

enum class Screen { Home, Servers, AddSub, Speed, Settings, SettingsPage, Rules, RulesText, Diag }

internal fun depth(s: Screen) = when (s) {
    Screen.Home -> 0
    Screen.Servers, Screen.AddSub, Screen.Speed, Screen.Settings -> 1
    Screen.SettingsPage, Screen.Diag -> 2
    Screen.Rules -> 3
    Screen.RulesText -> 4
}

class UiState(ctx: Context) {
    var vpn by mutableStateOf(VpnState.load(ctx))
    var subs by mutableStateOf(Subs.list(ctx))
    var selection by mutableStateOf(Subs.selection(ctx))
    var settings by mutableStateOf(Settings.load(ctx))
    var pings by mutableStateOf(Pings.load(ctx))
    var auto by mutableStateOf(AutoSelect.load(ctx))
    private val olderPings = HashMap<String, Pings?>()

    fun pingsFor(ctx: Context, sub: String?): Pings? = when {
        sub == null -> null
        pings.sub == sub -> pings
        olderPings.containsKey(sub) -> olderPings[sub]
        else -> Pings.archived(ctx, sub).also { olderPings[sub] = it }
    }

    fun updatePings(p: Pings) {
        val prev = pings
        if (prev.sub != null && prev.sub != p.sub && prev.results.isNotEmpty()) olderPings[prev.sub] = prev.copy(running = false, target = null, targets = null)
        p.sub?.let { olderPings.remove(it) }
        pings = p
    }
    var loading by mutableStateOf(false)
    var refreshing by mutableStateOf(emptySet<String>())
    var error by mutableStateOf<String?>(null)
    var hwidRetry by mutableStateOf<String?>(null)
    var incoming by mutableStateOf<String?>(null)
    var incomingSub by mutableStateOf<DeepLink.Target.Subscription?>(null)
    var tvOffer by mutableStateOf<TvLink.Offer?>(null)
    var sheet by mutableStateOf(Sheet.None)
    var entryShown by mutableStateOf<String?>(null)
    var pingAll by mutableStateOf<List<String>?>(null)
    var page by mutableStateOf(SettingsPage.Network)
    var screen by mutableStateOf(if (subs.isEmpty()) Screen.AddSub else Screen.Home)
    var speedTarget by mutableStateOf<Selection?>(null)
    var shown by mutableStateOf<String?>(null)
    var refreshAll by mutableStateOf(false)
    var speedNow by mutableStateOf(false)
    private val parsed = androidx.compose.runtime.mutableStateMapOf<String, List<Profile>>()
    private val parsing = HashSet<String>()
    private val parseGen = HashMap<String, Int>()
    private val app = ctx.applicationContext

    init {
        selection?.sub?.let { load(it) }
    }

    fun profiles(@Suppress("UNUSED_PARAMETER") ctx: Context, id: String?): List<Profile> {
        subs
        id ?: return emptyList()
        parsed[id]?.let { return it }
        load(id)
        return emptyList()
    }

    suspend fun profilesNow(id: String): List<Profile> = parsed[id]?.takeIf { id !in parsing } ?: withContext(Dispatchers.IO) { Subs.profiles(app, id) }.also { parsed[id] = it }

    private fun load(id: String, again: Boolean = false) {
        if (!parsing.add(id) && !again) return
        val gen = (parseGen[id] ?: 0) + 1
        parseGen[id] = gen
        parser.execute {
            val list = runCatching { Subs.profiles(app, id) }.getOrDefault(emptyList())
            mainHandler.post {
                if (parseGen[id] != gen) return@post
                parsing.remove(id)
                parsed[id] = list
            }
        }
    }

    fun reloadSubs(ctx: Context, changed: String? = null) {
        subs = Subs.list(ctx)
        selection = Subs.selection(ctx)
        val ids = subs.map { it.id }.toSet()
        parsed.keys.filter { it !in ids }.forEach { parsed.remove(it) }
        (if (changed != null) listOf(changed) else parsed.keys.toList()).filter { it in ids }.forEach { load(it, again = true) }
    }

    fun reload(ctx: Context) {
        vpn = VpnState.load(ctx)
        updatePings(Pings.load(ctx))
    }

    fun currentSub(): Sub? = subs.firstOrNull { it.id == selection?.sub } ?: subs.firstOrNull()

    fun select(ctx: Context, sel: Selection) {
        Subs.select(ctx, sel)
        selection = sel
    }
}

private val parser = java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "rynet-parse").apply { isDaemon = true } }

private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())
