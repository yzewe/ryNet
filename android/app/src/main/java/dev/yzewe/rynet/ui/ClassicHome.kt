package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.ExitIp
import dev.yzewe.rynet.data.ExpiryAlert
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.SubInfo
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.tr
import kotlin.math.abs
import kotlinx.coroutines.launch

@Composable
fun ClassicHomeScreen(
    d: HomeData,
    act: HomeActions,
    clock: State<Long> = mutableStateOf(0L),
    pane: Boolean = false,
    modifier: Modifier = Modifier
) {
    var shown by remember { mutableStateOf(d.vpn.status) }
    var since by remember { mutableLongStateOf(d.vpn.since) }
    if (d.vpn.since > 0) since = d.vpn.since
    val menu = remember { SlideMenuState() }
    Box(if (pane) modifier.fillMaxSize() else modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        Column(
            (if (pane) Modifier.fillMaxSize() else Modifier.fillMaxSize().safeDrawingPadding().padding(horizontal = 16.dp))
        ) {
            Box(Modifier.enter(0)) {
                TopBar(act.onSettings, act.onSpeed.takeIf { d.profile?.supported == true })
            }
            var lastAlert by remember { mutableStateOf(d.alert) }
            if (d.alert != null) lastAlert = d.alert
            AnimatedVisibility(d.alert != null, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                lastAlert?.let { a -> AlertBar(a.text, onClose = { act.onDismissAlert(a.sub) }) }
            }
            AnimatedVisibility(d.geoWarn, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                AlertBar(
                    tr("Списки российских сайтов не загружены: российские сайты пока идут через сервер", "Russian site lists are not loaded: Russian sites go through the server for now"),
                    null,
                    if (d.geoBusy) tr("Загрузка…", "Downloading…") else tr("Скачать", "Download")
                ) { if (!d.geoBusy) act.onGeoDownload() }
            }
            BoxWithConstraints(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                val error = d.vpn.status == Status.Error
                val fit = if (pane) maxHeight - if (error) 330.dp else 100.dp else maxHeight
                val target = if (error) {
                    minOf(maxWidth * 0.4f, maxHeight * 0.3f, fit).coerceIn(if (pane) 80.dp else 140.dp, 220.dp)
                } else {
                    minOf(maxWidth * 0.5f, maxHeight * 0.42f, fit).coerceIn(if (pane) 80.dp else 188.dp, 280.dp)
                }
                val orb by animateDpAsState(target, MaterialTheme.motionScheme.defaultSpatialSpec(), label = "orbSize")
                Column(
                    if (pane) Modifier.verticalScroll(rememberScrollState()).heightIn(min = maxHeight) else Modifier,
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    ConnectOrb(d.vpn.status, act.onToggle, Modifier.enter(1), size = orb, onShown = { shown = it })
                    Spacer(Modifier.height(if (error) 20.dp else 12.dp))
                    StatusText(d.vpn.copy(status = shown, since = since), clock, act.onToggle, act.onServers, d.exit, act.onReconnect)
                }
            }
            Column(Modifier.enter(2)) {
                AnimatedVisibility(d.chain, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                    ViaChip(d.via, d.vpn.status == Status.On, menu, { listOf(chainItem(d)) }, {
                        act.onChainSwitch(it.checked == true)
                        if (it.checked == false) menu.close()
                    }, act.onChain)
                }
                ServerCard(
                    d.profile, d.ping, d.info, d.pingWaiting, menu,
                    { listOf(chainItem(d)) + d.quickServers() },
                    { if (it.id == CHAIN_ITEM) act.onChainSwitch(it.checked == true) else act.onPickServer(it.id) },
                    act.onServers, d.autoSub.takeIf { d.autoOn }
                )
            }
            if (!pane) Spacer(Modifier.height(16.dp))
        }
        SlideMenuHost(menu)
    }
}

private const val CHAIN_ITEM = "\u0000chain"

private fun chainItem(d: HomeData) = SlideItem(CHAIN_ITEM, tr("Цепочка", "Chain"), d.via?.name?.let { tr("через $it", "via $it") }, icon = Glyph.Link, checked = d.chainOn)

private val FLAG_W = 56.dp * 4 / 3
private val FLAG_MID = 8.dp + FLAG_W / 2
private val VIA_INSET = 10.dp

@Composable
private fun ViaChip(p: Profile?, running: Boolean, menu: SlideMenuState, quick: () -> List<SlideItem>, onPick: (SlideItem) -> Unit, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    Column {
        Surface(
            shape = MaterialTheme.shapes.extraLarge,
            color = c.surfaceContainerHigh,
            modifier = Modifier.padding(start = FLAG_MID - 14.dp - VIA_INSET)
        ) {
            Row(
                Modifier.holdToSlide(menu, items = quick, onPick = onPick, onTap = onClick, corner = 20.dp, ring = false).height(40.dp).padding(start = VIA_INSET, end = 16.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (p != null) {
                    FlagIcon(p.flag, Modifier.size(width = 28.dp, height = 21.dp), MaterialTheme.shapes.extraSmall)
                    Spacer(Modifier.width(10.dp))
                    Text(tr("через ", "via "), style = MaterialTheme.typography.labelLarge, color = c.onSurfaceVariant)
                    Text(p.name, style = MaterialTheme.typography.labelLarge, color = c.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                } else {
                    Icon(Glyph.Link, contentDescription = null, tint = c.primary, modifier = Modifier.size(width = 28.dp, height = 18.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(tr("Выберите вход цепочки", "Choose chain entry"), style = MaterialTheme.typography.labelLarge, color = c.primary)
                }
            }
        }
        ChainWave(running, c.primary, Modifier.padding(start = FLAG_MID - 12.dp).width(24.dp).height(20.dp))
    }
}

@Composable
private fun ChainWave(running: Boolean, color: Color, modifier: Modifier) {
    val moving = if (running) {
        val t = rememberInfiniteTransition(label = "chain")
        t.animateFloat(0f, 1f, infiniteRepeatable(tween(900, easing = LinearEasing)), label = "phase")
    } else {
        null
    }
    val alpha by animateFloatAsState(if (running) 1f else 0.55f, label = "alpha")
    Canvas(modifier) {
        val phase = moving?.value ?: 0f
        val amp = 3.dp.toPx()
        val len = 10.dp.toPx()
        val x0 = size.width / 2
        val path = Path()
        var y = 0f
        while (y <= size.height) {
            val x = x0 + amp * kotlin.math.sin(((y / len) - phase) * 2 * Math.PI).toFloat()
            if (y == 0f) path.moveTo(x, y) else path.lineTo(x, y)
            y += 1f
        }
        drawPath(path, color.copy(alpha = alpha), style = Stroke(width = 2.5.dp.toPx(), cap = StrokeCap.Round))
    }
}

@Composable
private fun AlertBar(text: String, onClose: (() -> Unit)?, action: String? = null, onAction: () -> Unit = {}) {
    val c = MaterialTheme.colorScheme
    Surface(shape = MaterialTheme.shapes.large, color = c.surfaceContainerHigh, modifier = Modifier.fillMaxWidth().padding(top = 8.dp)) {
        Row(Modifier.padding(start = 20.dp, end = 4.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Glyph.Timer, contentDescription = null, tint = c.onSurfaceVariant, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Text(text, style = MaterialTheme.typography.bodyMedium, color = c.onSurface, modifier = Modifier.weight(1f))
            if (action != null) androidx.compose.material3.TextButton(onClick = onAction) { Text(action) }
            if (onClose != null) IconButton(onClick = onClose) { Icon(Icons.Rounded.Close, contentDescription = tr("Скрыть", "Hide"), tint = c.onSurfaceVariant, modifier = Modifier.size(20.dp)) }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun TopBar(onSettings: () -> Unit, onSpeed: (() -> Unit)?) {
    val c = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        if (onSpeed != null) {
            FilledTonalIconButton(
                onClick = onSpeed,
                shape = CircleShape,
                colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = c.surfaceContainerHigh, contentColor = c.onSurface),
                modifier = Modifier.size(52.dp)
            ) {
                Icon(Glyph.Speed, contentDescription = tr("Скорость", "Speed"))
            }
        }
        Spacer(Modifier.weight(1f))
        FilledTonalIconButton(
            onClick = onSettings,
            shape = CircleShape,
            colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = c.surfaceContainerHigh, contentColor = c.onSurface),
            modifier = Modifier.size(52.dp)
        ) {
            Icon(Icons.Rounded.Settings, contentDescription = tr("Настройки", "Settings"))
        }
    }
}

@Composable
private fun StatusText(vpn: VpnState, clock: State<Long>, onRetry: () -> Unit, onServers: () -> Unit, exitIp: ExitIp? = null, onReconnect: () -> Unit = {}) {
    val enter = MaterialTheme.motionScheme.defaultEffectsSpec<Float>()
    val exit = MaterialTheme.motionScheme.fastEffectsSpec<Float>()
    AnimatedContent(targetState = vpn.status, transitionSpec = { (fadeIn(enter) + scaleIn(spring(0.5f, 500f), initialScale = 0.85f)) togetherWith fadeOut(exit) }, label = "status") { s ->
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                when (s) {
                    Status.On -> if (vpn.failing) tr("Сервер не отвечает", "The server does not respond") else tr("Защищено", "Protected")
                    Status.Connecting -> tr("Подключение…", "Connecting…")
                    Status.Error -> tr("Не подключилось", "Couldn't connect")
                    Status.Off -> tr("Не подключено", "Not connected")
                },
                style = MaterialTheme.typography.headlineMediumEmphasized,
                color = MaterialTheme.colorScheme.onSurface
            )
            val sub = MaterialTheme.typography.bodyLarge
            val muted = MaterialTheme.colorScheme.onSurfaceVariant
            if (s == Status.On) {
                Text(if (vpn.since > 0) uptime((clock.value - vpn.since).coerceAtLeast(0)) else " ", style = sub, color = muted)
                vpn.error?.let { e ->
                    Text(
                        e,
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.error,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(top = 6.dp).padding(horizontal = 24.dp)
                    )
                }
                if (vpn.failing) {
                    RetryActions(onReconnect, onServers, Modifier.padding(top = 16.dp))
                } else if (vpn.error == null) {
                    Box(Modifier.height(28.dp).padding(top = 6.dp), contentAlignment = Alignment.Center) {
                        androidx.compose.animation.AnimatedVisibility(exitIp != null, enter = fadeIn() + expandHorizontally(), exit = fadeOut()) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                exitIp?.flag?.takeIf { it.isNotEmpty() }?.let { f ->
                                    FlagIcon(f, Modifier.size(width = 24.dp, height = 18.dp), MaterialTheme.shapes.extraSmall)
                                    Spacer(Modifier.width(8.dp))
                                }
                                Text(exitIp?.ip.orEmpty(), style = MaterialTheme.typography.labelLarge, color = muted)
                            }
                        }
                    }
                }
            } else if (s == Status.Error) {
                ConnectError(vpn.error, onRetry, onServers, Modifier.padding(top = 20.dp))
                if (vpn.blocked) {
                    val ctx = androidx.compose.ui.platform.LocalContext.current
                    Spacer(Modifier.height(8.dp))
                    Text(tr("Интернет заблокирован", "Internet blocked"), style = sub, color = MaterialTheme.colorScheme.error)
                    androidx.compose.material3.TextButton(onClick = { dev.yzewe.rynet.vpn.RynetService.disconnect(ctx, "blocked banner") }) { Text(tr("Отключить", "Disconnect")) }
                }
            } else {
                Text(
                    vpn.error?.takeIf { s == Status.Connecting } ?: " ",
                    style = sub,
                    color = MaterialTheme.colorScheme.error,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 24.dp)
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ServerCard(
    profile: Profile?,
    ping: Int?,
    info: ServerInfo,
    waiting: Boolean,
    menu: SlideMenuState,
    quick: () -> List<SlideItem>,
    onPick: (SlideItem) -> Unit,
    onClick: () -> Unit,
    auto: String? = null,
    swiped: Boolean = false
) {
    val c = MaterialTheme.colorScheme
    Surface(shape = MaterialTheme.shapes.extraLarge, color = c.surfaceContainerHigh, modifier = Modifier.fillMaxWidth()) {
        Box(Modifier.holdToSlide(menu, enabled = profile != null, items = quick, onPick = onPick, onTap = onClick, ring = false)) {
            val spatial = MaterialTheme.motionScheme.defaultSpatialSpec<IntOffset>()
            val fx = MaterialTheme.motionScheme
            AnimatedContent(
                targetState = profile,
                contentKey = { it?.remarks },
                transitionSpec = {
                    if (swiped) {
                        EnterTransition.None togetherWith ExitTransition.None
                    } else {
                        (slideInVertically(spatial) { it / 2 } + fadeIn(fx.defaultEffectsSpec())) togetherWith (slideOutVertically(spatial) { -it / 2 } + fadeOut(fx.fastEffectsSpec()))
                    }
                },
                label = "server"
            ) { profile ->
                Row(Modifier.heightIn(min = 72.dp).padding(end = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    FlagIcon(profile?.flag.orEmpty(), Modifier.padding(8.dp).size(width = FLAG_W, height = 56.dp), MaterialTheme.shapes.large)
                    Spacer(Modifier.width(6.dp))
                    Column(Modifier.weight(1f)) {
                        Text(profile?.name ?: tr("Выберите сервер", "Choose a server"), style = MaterialTheme.typography.titleMediumEmphasized, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val second = if (auto != null && profile != null) {
                            listOf(tr("авто", "auto"), auto).filter { it.isNotBlank() }.joinToString(" · ")
                        } else {
                            when (info) {
                                ServerInfo.Description -> profile?.description ?: profile?.kind
                                ServerInfo.Protocol -> profile?.kind
                                ServerInfo.None -> null
                            }
                        }
                        if (second != null) {
                            Text(second, style = MaterialTheme.typography.labelLarge, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                    if (profile != null && waiting) {
                        Spacer(Modifier.width(8.dp))
                        LoadingIndicator(Modifier.size(32.dp), color = MaterialTheme.colorScheme.primary)
                    } else if (profile != null && ping != null) {
                        Spacer(Modifier.width(8.dp))
                        AnimatedContent(
                            targetState = ping,
                            transitionSpec = { (slideInVertically { it } + fadeIn()) togetherWith (slideOutVertically { -it } + fadeOut()) },
                            label = "ping"
                        ) { v -> PingText(v, MaterialTheme.typography.titleLargeEmphasized, short = true) }
                    }
                    Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null, tint = c.onSurfaceVariant)
                }
            }
        }
    }
}
