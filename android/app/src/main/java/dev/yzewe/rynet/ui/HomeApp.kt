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

@Composable
internal fun App(s: UiState, serversOnly: Boolean = false) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbars = LocalSnackbars.current

    var iconEpoch by remember { mutableIntStateOf(0) }
    var editing by remember { mutableStateOf<Sub?>(null) }

    fun fetch(url: String, id: String? = null, then: (Sub) -> Unit = {}) {
        if (id == null) s.loading = true else s.refreshing += id
        s.error = null
        s.hwidRetry = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { Subs.fetch(ctx, url, id) } }
            if (id == null) s.loading = false else s.refreshing -= id
            r.onSuccess { sub ->
                iconEpoch++
                s.reloadSubs(ctx, sub.id)
                val sel = s.selection
                val list = s.profilesNow(sub.id)
                if (sel == null || sel.sub == sub.id && list.byRemarks(sel.profile)?.supported != true) {
                    list.firstOrNull { it.supported }?.let { s.select(ctx, Selection(sub.id, it.remarks)) }
                }
                if (id != null && s.settings.refreshNotices) {
                    val n = list.count { it.supported }
                    val servers = plural(n, "сервер", "сервера", "серверов", "server")
                    val title = (s.subs.firstOrNull { it.id == sub.id } ?: sub).info.title
                    snackbars.notice(tr("${title ?: "Подписка"} обновлена · $servers", "${title ?: "Subscription"} updated · $servers"))
                }
                then(sub)
            }.onFailure {
                val msg = it.message ?: it.toString()
                s.error = msg
                if (it is Subscription.NeedHwid && id == null) s.hwidRetry = url
                if (id != null) snackbars.error(msg)
            }
        }
    }

    LaunchedEffect(Unit) {
        for (sub in s.subs.filterNot(Local::isLocal)) {
            val every = (sub.info.updateIntervalHours ?: 24) * 3_600_000L
            if (s.settings.autoUpdate && System.currentTimeMillis() - sub.info.updatedAt > every) {
                fetch(sub.info.url, sub.id)
            } else if (SubIcon.load(ctx, sub.id) == null) {
                launch(Dispatchers.IO) {
                    runCatching { SubIcon.fetch(ctx, sub.id, sub.info) }
                    iconEpoch++
                }
            }
        }
    }

    var askConsent: (Intent) -> Unit = {}

    fun connect() {
        val sel = s.selection ?: return
        RynetService.consent(ctx)?.let {
            askConsent(it)
            return
        }
        RynetService.connect(ctx, sel)
    }

    fun addServers(text: String, then: () -> Unit = {}) {
        s.loading = true
        s.error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { Local.add(ctx, text) } }
            s.loading = false
            r.onSuccess { (sub, touched) ->
                s.reloadSubs(ctx, sub.id)
                val list = s.profilesNow(sub.id)
                touched.firstNotNullOfOrNull { list.getOrNull(it)?.takeIf { p -> p.supported } }?.let { p ->
                    s.select(ctx, Selection(sub.id, p.remarks))
                    if (s.vpn.status != Status.Off) connect()
                }
                snackbars.notice(
                    if (touched.size == 1) {
                        tr("Добавлено в «${Local.TITLE}»", "Added to \"${Local.TITLE}\"")
                    } else {
                        plural(touched.size, "сервер", "сервера", "серверов", "server").let { tr("В «${Local.TITLE}» · $it", "Added to \"${Local.TITLE}\" · $it") }
                    }
                )
                then()
            }.onFailure {
                val msg = it.message ?: it.toString()
                s.error = msg
                if (s.screen != Screen.AddSub) snackbars.error(msg)
            }
        }
    }

    fun editLocal(p: Profile, rename: String?) {
        scope.launch {
            val i = s.profilesNow(Local.ID).indexOf(p)
            if (i < 0) return@launch
            val wasSelected = s.selection?.let { it.sub == Local.ID && it.profile == p.remarks } == true
            withContext(Dispatchers.IO) { if (rename != null) Local.rename(ctx, i, rename) else Local.remove(ctx, i) }
            s.reloadSubs(ctx, Local.ID)
            if (s.subs.isEmpty()) {
                s.sheet = Sheet.None
                s.screen = Screen.AddSub
                return@launch
            }
            if (!wasSelected) return@launch
            val next = if (rename != null) {
                s.profilesNow(Local.ID).getOrNull(i)?.let { Selection(Local.ID, it.remarks) }
            } else {
                (listOfNotNull(s.subs.firstOrNull { it.id == Local.ID }) + s.subs).firstNotNullOfOrNull { sub ->
                    s.profilesNow(sub.id).firstOrNull { it.supported }?.let { Selection(sub.id, it.remarks) }
                }
            }
            next?.let { s.select(ctx, it) }
            if (rename == null && next != null && s.vpn.status != Status.Off) connect()
        }
    }
    fun removeDead(x: Sub) {
        if (!Local.isLocal(x)) return
        scope.launch {
            val results = s.pingsFor(ctx, x.id)?.results.orEmpty()
            val list = s.profilesNow(Local.ID)
            val dead = list.indices.filter { (results[list[it].remarks] ?: 0) < 0 }.toSet()
            if (dead.isEmpty()) {
                snackbars.notice(tr("Нерабочих серверов нет", "No non-working servers"))
                return@launch
            }
            val selected = s.selection?.takeIf { it.sub == Local.ID }?.profile
            val wasSelected = dead.any { list[it].remarks == selected }
            withContext(Dispatchers.IO) { Local.removeAll(ctx, dead) }
            s.reloadSubs(ctx, Local.ID)
            snackbars.notice(tr("Удалено серверов: ${dead.size}", "Servers deleted: ${dead.size}"))
            if (s.subs.isEmpty()) {
                s.sheet = Sheet.None
                s.screen = Screen.AddSub
                return@launch
            }
            if (!wasSelected) return@launch
            s.subs.firstNotNullOfOrNull { sub -> s.profilesNow(sub.id).firstOrNull { it.supported }?.let { Selection(sub.id, it.remarks) } }?.let { s.select(ctx, it) }
        }
    }
    val localEdit = remember { ServerEdit(rename = { p, name -> editLocal(p, name) }, remove = { p -> editLocal(p, null) }) }

    val vpnConsent = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (it.resultCode == Activity.RESULT_OK) connect()
    }
    askConsent = { vpnConsent.launch(it) }
    val notifPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        val consent = RynetService.consent(ctx)
        if (consent != null) vpnConsent.launch(consent) else connect()
    }

    fun openServers(sub: String?) {
        s.sheet = Sheet.None
        if (serversOnly) {
            s.shown = sub ?: s.currentSub()?.id
            s.screen = Screen.Servers
            return
        }
        ctx.startActivity(ServersActivity.intent(ctx, sub ?: s.currentSub()?.id))
    }

    fun toggle() {
        when (s.vpn.status) {
            Status.On, Status.Connecting -> RynetService.disconnect(ctx, if (s.vpn.status == Status.On) "button" else "button while connecting")

            else -> {
                if (s.selection == null) {
                    openServers(null)
                    return
                }
                if (Build.VERSION.SDK_INT >= 33 &&
                    ctx.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                ) {
                    notifPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    return
                }
                val consent = RynetService.consent(ctx)
                if (consent != null) vpnConsent.launch(consent) else connect()
            }
        }
    }

    fun updateSettings(new: Settings) {
        Settings.change(ctx, s.settings, new)
        s.settings = new
    }

    suspend fun receiveTv(p: TvLink.Payload): String? {
        p.hwid?.let { updateSettings(s.settings.copy(hwid = it, sendHwid = true)) }
        var added: Sub? = null
        var failure: String? = null
        for (url in p.subs) {
            withContext(Dispatchers.IO) { runCatching { Subs.fetch(ctx, url, null) } }
                .onSuccess { added = added ?: it }
                .onFailure { failure = failure ?: (it.message ?: it.toString()) }
        }
        p.servers?.let { text ->
            withContext(Dispatchers.IO) { runCatching { Local.add(ctx, text).first } }
                .onSuccess { added = added ?: it }
                .onFailure { failure = failure ?: (it.message ?: it.toString()) }
        }
        val sub = added ?: return failure ?: tr("Нечего добавить", "Nothing to add")
        iconEpoch++
        s.reloadSubs(ctx, sub.id)
        if (s.selection == null) s.profilesNow(sub.id).firstOrNull { it.supported }?.let { s.select(ctx, Selection(sub.id, it.remarks)) }
        s.screen = Screen.Home
        snackbars.notice(tr("Подписки получены с телефона", "Subscriptions received from the phone"))
        failure?.let { snackbars.error(it) }
        return null
    }
    fun prefsFor(shown: String?): ServerPrefs? = shown?.let { id ->
        ServerPrefs(
            s.settings.pinned(id),
            s.settings.perServerDns[id].orEmpty(),
            onPin = { p, on -> updateSettings(s.settings.pin(id, p.remarks, on)) },
            onDns = { p, dns -> updateSettings(s.settings.serverDns(id, p.remarks, dns)) },
            body = { Subs.body(ctx, id) }
        )
    }

    fun manual() {
        if (s.settings.autoSelect) updateSettings(s.settings.copy(autoSelect = false))
    }

    fun choose(sel: Selection) {
        manual()
        s.select(ctx, sel)
        Usage.bump(ctx, sel)
    }

    fun autoOn() {
        if (s.settings.autoSelect) return
        updateSettings(s.settings.copy(autoSelect = true, autoFeature = true))
        if (s.vpn.status == Status.Off || s.vpn.status == Status.Error) RynetService.autoCheck(ctx)
    }

    fun pickSub(target: Sub) {
        if (s.selection?.sub == target.id) return manual()
        manual()
        scope.launch {
            val list = s.profilesNow(target.id)
            (Subs.last(ctx, target.id)?.let { list.byRemarks(it) }?.takeIf { it.supported } ?: list.firstOrNull { it.supported })?.let { p ->
                s.select(ctx, Selection(target.id, p.remarks))
                if (s.vpn.status != Status.Off) connect()
            }
        }
    }

    editing?.let { e ->
        val balancers by produceState(false, e.id) {
            value = withContext(Dispatchers.IO) { runCatching { Subscription.parse(Subs.body(ctx, e.id) ?: "[]").any { it.members.isNotEmpty() } }.getOrDefault(false) }
        }
        val wasSplit = remember(e.id) { Subs.split(ctx, e.id) }
        SubEditDialog(e, balancers, wasSplit, onDismiss = { editing = null }) { name, url, split, done ->
            scope.launch {
                val r = withContext(Dispatchers.IO) {
                    runCatching {
                        Subs.setSplit(ctx, e.id, split)
                        Subs.edit(ctx, e.id, name, url)
                    }
                }
                r.onSuccess { sub ->
                    val moved = !Subs.sameUrl(sub.info.url, e.info.url)
                    if (moved) iconEpoch++
                    s.reloadSubs(ctx, sub.id)
                    val sel = s.selection
                    if ((moved || split != wasSplit) && sel != null && sel.sub == sub.id) {
                        val list = s.profilesNow(sub.id)
                        if (list.byRemarks(sel.profile)?.supported != true) list.firstOrNull { it.supported }?.let { s.select(ctx, Selection(sub.id, it.remarks)) }
                    }
                    editing = null
                    done(null)
                }.onFailure { done(it.message ?: it.toString()) }
            }
        }
    }

    s.tvOffer?.let { offer -> TvSendSheet(offer, s.subs) { s.tvOffer = null } }

    s.incoming?.let { link ->
        AddServersDialog(link, switches = s.vpn.status != Status.Off, onDismiss = { s.incoming = null }) {
            s.incoming = null
            addServers(link) { s.screen = Screen.Home }
        }
    }

    s.incomingSub?.let { link ->
        val have = s.subs.firstOrNull { Subs.sameUrl(it.info.url, Subs.withScheme(link.url)) }
        AddLinkDialog(link, have?.info?.title, have != null, onDismiss = { s.incomingSub = null }) {
            s.incomingSub = null
            if (have != null) {
                fetch(have.info.url, have.id) { if (!s.settings.autoSelect) pickSub(it) }
                return@AddLinkDialog
            }
            fetch(link.url) { sub ->
                s.screen = Screen.Home
                if (sub.info.title.isNullOrBlank() && link.name != null) {
                    scope.launch {
                        withContext(Dispatchers.IO) { Subs.body(ctx, sub.id)?.let { Subs.import(ctx, sub.copy(info = sub.info.copy(title = link.name)), it) } }
                        s.reloadSubs(ctx, sub.id)
                    }
                }
                if (!s.settings.autoSelect) pickSub(sub)
            }
        }
    }

    val sel = s.selection
    val sub = s.currentSub()
    val expanded = LocalConfiguration.current.screenWidthDp.dp >= ExpandedWidth
    val chainEntry = s.settings.chainEntry()?.takeIf { it != sel }?.let { e -> s.profiles(ctx, e.sub).byRemarks(e.profile) }
    var alertEpoch by remember { mutableIntStateOf(0) }
    val exitIp by produceState<ExitIp?>(null, s.vpn.status == Status.On, s.vpn.since) {
        value = null
        if (s.vpn.status != Status.On) return@produceState
        repeat(3) { attempt ->
            val asked = s.selection
            val r = withContext(Dispatchers.IO) { ExitIp.lookup(s.settings.ipUrl) }
            if (r != null) {
                value = r
                val x = asked?.takeIf { it == s.selection && s.vpn.status == Status.On }
                if (x != null && s.settings.exitFlags && s.settings.chainEntry()?.takeIf { it != x } == null && r.flag.isNotEmpty()) {
                    withContext(Dispatchers.IO) {
                        val p = Subs.profiles(ctx, x.sub).byRemarks(x.profile)?.takeIf { it.members.isEmpty() }
                        if (p != null) Countries.put(ctx, x.sub, mapOf(x.profile to r.country), mapOf(x.profile to Countries.endpoint(p)))
                    }
                }
                return@produceState
            }
            kotlinx.coroutines.delay(1500L * (attempt + 1))
        }
    }
    var geoEpoch by remember { mutableIntStateOf(0) }
    var geoBusy by remember { mutableStateOf(false) }
    val geoWarn = remember(s.settings.routing, geoEpoch) { GeoWarn.needed(ctx, s.settings.routing) }
    val geoDownload = {
        if (!geoBusy) {
            geoBusy = true
            scope.launch {
                val ok = withContext(Dispatchers.IO) { GeoWarn.download(ctx) }
                geoBusy = false
                geoEpoch++
                if (ok && s.vpn.status == Status.On) sel?.let { RynetService.connect(ctx, it) }
            }
        }
    }
    LaunchedEffect(s.settings.routing) { if (GeoWarn.needed(ctx, s.settings.routing)) geoDownload() }
    val resumed = androidx.lifecycle.compose.LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val alert = remember(s.subs, alertEpoch, resumed.value) { ExpiryAlert.pick(ctx, s.subs) }
    val clock = rememberClock(s.vpn.status == Status.On)
    val icon by produceState<ImageBitmap?>(null, sub?.id, iconEpoch) {
        value = sub?.let { withContext(Dispatchers.IO) { SubIcon.load(ctx, it.id)?.asImageBitmap() } }
    }
    fun pingSub(id: String?, profile: String? = null, names: List<String>? = null) {
        if (id == null || names != null && names.isEmpty()) return
        s.updatePings((s.pingsFor(ctx, id) ?: Pings(id)).restart(profile, names))
        RynetService.ping(ctx, id, profile, names)
    }
    LaunchedEffect(Unit) {
        if (s.settings.pingOnStart && !s.pings.running) pingSub(sub?.id)
    }
    BackHandler(enabled = s.screen == Screen.AddSub && s.subs.isNotEmpty() || s.screen == Screen.Speed) { s.screen = Screen.Home }
    fun openSpeed(target: Selection?) {
        if (target == null) return
        s.sheet = Sheet.None
        s.speedTarget = target
        s.screen = Screen.Speed
    }
    LaunchedEffect(s.pings) {
        val list = s.pingAll ?: return@LaunchedEffect
        if (!s.pings.running && s.pings.sub == list.last()) s.pingAll = null
    }
    LaunchedEffect(s.refreshAll) {
        if (!s.refreshAll) return@LaunchedEffect
        s.refreshAll = false
        s.subs.filter { it.id !in s.refreshing && !Local.isLocal(it) }.forEach { x -> fetch(x.info.url, x.id) }
    }
    LaunchedEffect(s.speedNow) {
        if (!s.speedNow) return@LaunchedEffect
        s.speedNow = false
        openSpeed(s.selection)
    }
    fun closeServers() {
        if (serversOnly) (ctx as? Activity)?.finish() else s.screen = Screen.Home
    }

    @Composable
    fun ServersPage(pane: Boolean) {
        val afterPick = { if (!pane) closeServers() }
        val shown = s.shown?.takeIf { id -> s.subs.any { it.id == id } } ?: sel?.sub ?: sub?.id
        val pp = s.pingsFor(ctx, shown)
        ServersScreen(
            subs = s.subs,
            shown = shown,
            servers = s.subs.associate { x -> x.id to s.profiles(ctx, x.id).count { it.supported } },
            refreshing = s.refreshing,
            auto = s.settings.autoSelect,
            onShow = { id -> s.shown = id },
            onAuto = { s.sheet = Sheet.AutoWhy },
            onAdd = {
                s.error = null
                s.screen = Screen.AddSub
            },
            onRefreshAll = { s.subs.filter { it.id !in s.refreshing && !Local.isLocal(it) }.forEach { x -> fetch(x.info.url, x.id) } },
            actions = SubActions(
                onRefresh = { fetch(it.info.url, it.id) },
                onEdit = { editing = it },
                onCopyAll = { x ->
                    scope.launch {
                        val links = s.profilesNow(x.id).flatMap { listOf(it) + it.members }.mapNotNull { it.link }.distinct()
                        if (links.isEmpty()) {
                            snackbars.error(tr("Нет серверов для копирования", "No servers to copy"))
                        } else {
                            (ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? android.content.ClipboardManager)
                                ?.setPrimaryClip(android.content.ClipData.newPlainText("ryNet", links.joinToString("\n")))
                            snackbars.notice(tr("Скопировано серверов: ${links.size}", "Servers copied: ${links.size}"))
                        }
                    }
                },
                onRemoveDead = { x -> removeDead(x) },
                onDelete = {
                    Subs.remove(ctx, it.id)
                    Usage.forget(ctx, it.id)
                    s.settings = Settings.load(ctx)
                    s.reloadSubs(ctx)
                    if (s.shown == it.id) s.shown = null
                    if (s.subs.isEmpty()) s.screen = Screen.AddSub
                },
                onAddServer = {
                    s.error = null
                    s.screen = Screen.AddSub
                }
            ),
            onBack = { closeServers() },
            pane = pane,
            ping = if (pp?.running == true) pp.let { if (it.total > 0) it.done.toFloat() / it.total else 0f } else null,
            onPing = { id, names -> pingSub(id, names = names) },
            onStopPing = {
                s.pingAll = null
                RynetService.cancelPing(ctx)
            },
            all = true,
            allPing = s.pingAll?.let { list ->
                val i = list.indexOf(s.pings.sub).coerceAtLeast(0)
                (i + if (s.pings.running && s.pings.total > 0) s.pings.done.toFloat() / s.pings.total else 0f) / list.size
            },
            onPingAll = {
                val ids = s.subs.map { it.id }
                if (ids.isNotEmpty()) {
                    s.pingAll = ids
                    s.updatePings((s.pingsFor(ctx, ids[0]) ?: Pings(ids[0])).restart(null))
                    RynetService.pingAll(ctx, ids)
                }
            },
            allPage = { query ->
                var details by remember { mutableStateOf<AllEntry?>(null) }
                val queued = s.pingAll?.let { list -> list.drop(list.indexOf(s.pings.sub) + 1).toSet() }.orEmpty()
                val entries = s.subs.flatMap { x ->
                    val pp = s.pingsFor(ctx, x.id)
                    s.profiles(ctx, x.id).filter { it.supported && !it.placeholder }.map { p ->
                        AllEntry(x.id, p, pp?.results?.get(p.remarks), pp?.errors?.get(p.remarks), pp?.waiting(p.remarks) == true || x.id in queued)
                    }
                }
                val pinned = s.subs.flatMap { x -> s.settings.pinned(x.id).map { x.id + "\u001f" + it } }.toSet()
                val autoPick = if (s.settings.autoFeature) {
                    (s.auto?.chosen ?: sel)?.let { a ->
                        entries.firstOrNull { it.selection == a } ?: s.profiles(ctx, a.sub).byRemarks(a.profile)?.let { m ->
                            val pp = s.pingsFor(ctx, a.sub)
                            val parent = Profile.parentOf(a.profile)?.let { r -> entries.firstOrNull { it.sub == a.sub && it.p.remarks == r }?.p }
                            val p = if (parent == null) m else m.copy(name = "${parent.name} · ${m.name}", flag = m.flag.ifBlank { parent.flag })
                            AllEntry(a.sub, p, pp?.results?.get(m.remarks), pp?.errors?.get(m.remarks), false)
                        }
                    }
                } else {
                    null
                }
                details?.let { e ->
                    ServerDetails(
                        e.p, e.ping, e.error, s.pingsFor(ctx, e.sub)?.running == true, e.selection == sel,
                        onCheck = { pingSub(e.sub, e.p.remarks) }, onDismiss = { details = null }, waiting = e.waiting,
                        onSpeed = { openSpeed(e.selection) }, edit = localEdit.takeIf { e.sub == Local.ID }, prefs = prefsFor(e.sub)
                    ) {
                        choose(e.selection)
                        afterPick()
                        if (s.vpn.status != Status.Off) connect()
                    }
                }
                AllServersContent(
                    entries, pinned, sel?.takeIf { !s.settings.autoSelect }, s.settings.autoFeature, s.settings.autoSelect, autoPick,
                    s.settings.sort, s.settings.grid, s.settings.serverInfo, query,
                    columns = if (pane) PANE_COLUMNS else null, navInset = !pane,
                    onAuto = {
                        if (s.settings.autoSelect) {
                            s.sheet = Sheet.AutoWhy
                        } else {
                            autoOn()
                            if (s.vpn.status == Status.On || s.vpn.status == Status.Connecting) RynetService.autoCheck(ctx)
                            afterPick()
                        }
                    },
                    onLongClick = { details = it }
                ) { e ->
                    choose(e.selection)
                    afterPick()
                    if (s.vpn.status != Status.Off) connect()
                }
            }
        ) { page, query, onFound ->
            val shown = page
            val pp = s.pingsFor(ctx, shown)
            ServersContent(
                s.profiles(ctx, shown), sel?.profile?.takeIf { sel.sub == shown && !s.settings.autoSelect }, pp?.results ?: emptyMap(), pp?.running == true,
                s.settings.sort, s.settings.view, s.settings.grid,
                onPing = { names -> pingSub(shown, names = names) },
                onStopPing = { RynetService.cancelPing(ctx) },
                info = s.settings.serverInfo,
                errors = pp?.errors ?: emptyMap(),
                onCheck = { p -> pingSub(shown, p.remarks) },
                progress = (pp?.done ?: 0) to (pp?.total ?: 0),
                waiting = { name -> pp?.waiting(name) == true },
                onSpeed = { p -> openSpeed(shown?.let { Selection(it, p.remarks) }) },
                edit = localEdit.takeIf { shown == Local.ID },
                query = query,
                fill = query.isNotBlank(),
                columns = if (pane) PANE_COLUMNS else null,
                navInset = !pane,
                stretch = true,
                pingFab = false,
                pingWave = false,
                onFound = onFound,
                prefs = prefsFor(shown)
            ) { p ->
                choose(Selection(shown, p.remarks))
                afterPick()
                if (s.vpn.status != Status.Off) connect()
            }
        }
    }
    LaunchedEffect(s.screen) { if (serversOnly && s.screen == Screen.Home) (ctx as? Activity)?.finish() }
    AnimatedContent(
        targetState = s.screen,
        transitionSpec = {
            val slide = spring<IntOffset>(dampingRatio = 1f, stiffness = Spring.StiffnessMediumLow)
            val dir = if (depth(targetState) > depth(initialState)) 1 else -1
            (slideInHorizontally(slide) { dir * it / 4 } + fadeIn(tween(220, delayMillis = 60))) togetherWith
                (slideOutHorizontally(slide) { -dir * it / 8 } + fadeOut(tween(150)))
        },
        label = "screen"
    ) { screen ->
        when (screen) {
            Screen.AddSub -> {
                OnboardingScreen(
                    initialUrl = "",
                    loading = s.loading,
                    error = s.error,
                    onFixHwid = s.hwidRetry?.let { url ->
                        {
                            updateSettings(s.settings.copy(sendHwid = true))
                            fetch(url) { s.screen = Screen.Home }
                        }
                    },
                    onBack = if (s.subs.isNotEmpty()) ({ s.screen = Screen.Home }) else null,
                    onSettings = if (s.subs.isEmpty()) ({ ctx.startActivity(SettingsActivity.intent(ctx)) }) else null,
                    onTvOffer = { s.tvOffer = it },
                    tvReceive = if (isTv(ctx)) ::receiveTv else null,
                    onSubmit = { text -> if (Local.isServers(text)) addServers(text) { s.screen = Screen.Home } else fetch(text) { s.screen = Screen.Home } }
                )
            }

            Screen.Speed -> {
                val t = s.speedTarget
                SpeedScreen(
                    profile = t?.let { s.profiles(ctx, it.sub).byRemarks(it.profile) },
                    onStart = { t?.let { RynetService.speedtest(ctx, it) } },
                    onCancel = { RynetService.cancelSpeedtest(ctx) },
                    onBack = { s.screen = Screen.Home }
                )
            }

            Screen.Servers -> ServersPage(pane = false)

            Screen.Settings, Screen.SettingsPage, Screen.Rules, Screen.RulesText, Screen.Diag -> {}

            Screen.Home -> {
                val data = HomeData(
                    title = if (s.settings.autoSelect) tr("Автовыбор", "Auto select") else sub?.title ?: "ryNet",
                    sub = sub?.info,
                    icon = icon,
                    vpn = s.vpn,
                    profile = s.profiles(ctx, sel?.sub).byRemarks(sel?.profile),
                    ping = sel?.let { x ->
                        s.pingsFor(ctx, x.sub)?.results?.get(x.profile)
                            ?: s.auto?.candidates?.firstOrNull { it.sub == x.sub && it.profile == x.profile && it.ms >= 0 }?.ms
                    },
                    pingWaiting = sel?.let { s.pingsFor(ctx, it.sub)?.waiting(it.profile) } == true,
                    info = s.settings.serverInfo,
                    alert = alert,
                    geoWarn = geoWarn,
                    geoBusy = geoBusy,
                    exit = exitIp,
                    via = chainEntry,
                    chain = s.settings.chain && s.settings.chainEntry() != sel,
                    chainOn = s.settings.chain,
                    subIndex = s.subs.indexOfFirst { it.id == sub?.id }.coerceAtLeast(0),
                    subCount = s.subs.size,
                    quickServers = {
                        val id = sel?.sub ?: sub?.id
                        val list = s.profiles(ctx, id).filter { it.supported && !it.placeholder }
                        val pings = s.pingsFor(ctx, id)?.results.orEmpty()
                        val pinned = id?.let { s.settings.pinned(it) }.orEmpty().mapNotNull { list.byRemarks(it) }
                        val used = id?.let { Usage.top(ctx, it) }.orEmpty().mapNotNull { list.byRemarks(it) }
                        val best = list.filter { (pings[it.remarks] ?: -1) >= 0 }.sortedBy { pings[it.remarks] }
                        (listOfNotNull(list.byRemarks(sel?.profile)) + pinned + used + best + list).distinctBy { it.remarks }.take(6).map { p ->
                            SlideItem(p.remarks, p.name, p.kind, p.flag, pings[p.remarks]?.takeIf { it >= 0 }?.toString(), p.remarks == sel?.profile)
                        }
                    },
                    quickSubs = {
                        listOf(SlideItem(AUTO_ITEM, tr("Автовыбор", "Auto select"), autoStatus(s.auto, System.currentTimeMillis(), s.vpn.status == Status.On)?.takeIf { s.settings.autoSelect } ?: tr("все подписки", "all subscriptions"), selected = s.settings.autoSelect)) +
                            s.subs.map { x ->
                                val n = s.profiles(ctx, x.id).count { it.supported }
                                SlideItem(x.id, x.title, plural(n, "сервер", "сервера", "серверов", "server").takeIf { n > 0 }, selected = !s.settings.autoSelect && x.id == sub?.id)
                            }
                    },
                    autoOn = s.settings.autoSelect,
                    autoSub = sub?.info?.title
                )
                val actions = HomeActions(
                    onToggle = ::toggle,
                    onReconnect = { connect() },
                    onServers = { if (!expanded) openServers(sel?.sub) },
                    onSettings = { ctx.startActivity(SettingsActivity.intent(ctx)) },
                    onSpeed = { openSpeed(sel) },
                    onChain = {
                        s.entryShown = s.settings.chainEntry()?.sub?.takeIf { id -> s.subs.any { it.id == id } } ?: sel?.sub
                        s.sheet = Sheet.Entry
                    },
                    onChainSwitch = { on -> updateSettings(s.settings.copy(chain = on)) },
                    onGeoDownload = geoDownload,
                    onDismissAlert = { id ->
                        ExpiryAlert.dismiss(ctx, id)
                        alertEpoch++
                    },
                    onPickServer = { remarks ->
                        val id = sel?.sub ?: sub?.id
                        if (id != null && remarks != sel?.profile) {
                            choose(Selection(id, remarks))
                            if (s.vpn.status != Status.Off) connect()
                        }
                    },
                    onPickSub = { id -> if (id == AUTO_ITEM) autoOn() else s.subs.firstOrNull { it.id == id }?.let(::pickSub) },
                    onSubStep = { step ->
                        val i = s.subs.indexOfFirst { it.id == sub?.id }
                        s.subs.getOrNull(i + step)?.let(::pickSub)
                    },
                    onAutoSwitch = { on -> if (on) autoOn() else manual() },
                    onAutoSettings = { ctx.startActivity(SettingsActivity.intent(ctx, SettingsActivity.page(SettingsPage.Auto))) }
                )
                if (!expanded) {
                    ClassicHomeScreen(data, actions, clock)
                } else {
                    TwoPane(
                        home = { m -> ClassicHomeScreen(data, actions, clock, pane = true, modifier = m) },
                        servers = { m -> Box(m) { ServersPage(pane = true) } }
                    )
                }
            }
        }
    }

    when (s.sheet) {
        Sheet.Entry -> {
            val shown = s.entryShown?.takeIf { id -> s.subs.any { it.id == id } } ?: sub?.id
            val pings = s.pingsFor(ctx, shown)
            val entry = s.settings.chainEntry()
            ServersSheet(
                profiles = s.profiles(ctx, shown),
                selected = entry?.profile?.takeIf { entry.sub == shown },
                pings = pings?.results ?: emptyMap(),
                errors = pings?.errors ?: emptyMap(),
                waiting = { name -> pings?.waiting(name) == true },
                pinging = pings?.running == true,
                progress = (pings?.done ?: 0) to (pings?.total ?: 0),
                sort = s.settings.sort,
                view = s.settings.view,
                grid = s.settings.grid,
                info = s.settings.serverInfo,
                onPing = { names -> pingSub(shown, names = names) },
                onStopPing = { RynetService.cancelPing(ctx) },
                onCheck = { p -> pingSub(shown, p.remarks) },
                onSelect = { p: Profile ->
                    if (shown == null) return@ServersSheet
                    updateSettings(s.settings.copy(chainSub = shown, chainProfile = p.remarks))
                    s.sheet = Sheet.None
                    if (s.vpn.status != Status.Off) connect()
                },
                onDismiss = { s.sheet = Sheet.None },
                title = tr("Вход цепочки", "Chain entry"),
                header = { SubChips(s.subs, shown) { s.entryShown = it } },
                contentKey = shown
            )
        }

        Sheet.AutoWhy -> AutoWhySheet(
            s.auto,
            sel,
            onPick = { pick ->
                s.sheet = Sheet.None
                choose(pick)
                if (s.vpn.status != Status.Off) connect()
            },
            onCheck = { RynetService.autoCheck(ctx) },
            onSettings = {
                s.sheet = Sheet.None
                ctx.startActivity(SettingsActivity.intent(ctx, SettingsActivity.page(SettingsPage.Auto)))
            },
            onDismiss = { s.sheet = Sheet.None }
        )

        Sheet.None -> {}
    }
}

private const val PANE_COLUMNS = 3

internal const val AUTO_ITEM = "\u0000auto"
