@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import android.text.format.DateUtils
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.exclude
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ripple
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import dev.yzewe.rynet.data.Endpoints
import dev.yzewe.rynet.data.Local
import dev.yzewe.rynet.data.Pings
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.Section
import dev.yzewe.rynet.data.ServerGrid
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.data.ServerView
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.data.Subscription
import dev.yzewe.rynet.data.byRemarks
import dev.yzewe.rynet.data.pinnedBy
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import java.text.Collator
import java.util.IdentityHashMap
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
internal fun ServerTile(
    p: Profile,
    ping: Int?,
    error: String?,
    selected: Boolean,
    wide: Boolean,
    info: ServerInfo,
    modifier: Modifier,
    waiting: Boolean = false,
    onLongClick: () -> Unit = {},
    onClick: () -> Unit
) {
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    val bg by animateColorAsState(if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh, Springs.effects(), label = "tile")
    val content = if (selected) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface
    val shape = if (wide) MaterialTheme.shapes.largeIncreased else MaterialTheme.shapes.large
    val height = if (wide) 60.dp else 52.dp
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = height)
            .alpha(if (p.supported) 1f else 0.45f)
            .pressSquish(src, 0.96f)
            .remoteFocus(src, shape)
            .clip(shape)
            .background(bg)
            .combinedClickable(
                interactionSource = src,
                indication = PressGlow,
                onClick = {
                    view.haptic(Haptic.Tap)
                    if (p.supported) onClick() else onLongClick()
                },
                onLongClick = {
                    view.haptic(Haptic.LongPress)
                    onLongClick()
                }
            ),
        verticalAlignment = Alignment.CenterVertically
    ) {
        FlagIcon(p.flag, Modifier.padding(8.dp).size(width = if (wide) 58.67.dp else 48.dp, height = if (wide) 44.dp else 36.dp), if (wide) MaterialTheme.shapes.medium else MaterialTheme.shapes.small)
        Spacer(Modifier.width(if (wide) 6.dp else 2.dp))
        Column(Modifier.weight(1f).padding(end = if (wide) 16.dp else 10.dp, top = 8.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(p.name, color = content, style = if (wide) MaterialTheme.typography.titleMediumEmphasized else MaterialTheme.typography.titleSmallEmphasized, maxLines = 1, overflow = TextOverflow.MiddleEllipsis)
            val dim = if (selected) content.copy(alpha = 0.72f) else MaterialTheme.colorScheme.onSurfaceVariant
            val small = if (wide) MaterialTheme.typography.labelLarge else MaterialTheme.typography.labelMedium
            val broken = ping == Pings.FAILED && error != null
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) {
                    when {
                        broken -> Sub(error.orEmpty().substringBefore(':').substringBefore(" — "), if (selected) content else MaterialTheme.colorScheme.error, small)
                        !p.supported -> Sub(listOf(shortKind(p.kind), tr("не поддерживается", "not supported")).filter { it.isNotEmpty() }.joinToString(" · "), dim, small)
                        info == ServerInfo.Description -> Sub(p.description ?: tileKind(p, wide), dim, small)
                        info == ServerInfo.Protocol -> Sub(tileKind(p, wide), dim, small)
                    }
                }
                if (p.supported && waiting) {
                    Spacer(Modifier.width(6.dp))
                    LoadingIndicator(Modifier.size(if (wide) 22.dp else 20.dp), color = if (selected) content else MaterialTheme.colorScheme.primary)
                } else if (p.supported && ping != null) {
                    Spacer(Modifier.width(6.dp))
                    PingText(ping, if (wide) MaterialTheme.typography.labelLargeEmphasized else MaterialTheme.typography.labelMediumEmphasized, short = true)
                }
            }
        }
    }
}

private fun tileKind(p: Profile, wide: Boolean): String = when {
    p.members.isNotEmpty() -> tr("Балансер · ${p.members.size}", "Balancer · ${p.members.size}")
    wide -> p.kind
    else -> shortKind(p.kind)
}

@Composable
internal fun Sub(text: String, color: Color, style: TextStyle) {
    Text(text, color = color, style = style, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

fun shortKind(kind: String): String {
    val parts = kind.split(" · ")
    return if (parts.first() in setOf("Hysteria 2", "Trojan", "Shadowsocks")) parts.first() else parts.last()
}

@Composable
fun PingText(ms: Int, style: TextStyle, on: Color? = null, short: Boolean = false) {
    val color = on ?: when {
        ms < 0 -> MaterialTheme.colorScheme.error
        ms < 300 -> MaterialTheme.colorScheme.primary
        ms < 800 -> MaterialTheme.colorScheme.warning
        else -> MaterialTheme.colorScheme.error
    }
    if (ms == Pings.FAILED) {
        val d = with(LocalDensity.current) { style.fontSize.toDp() }
        Icon(Icons.Rounded.Close, tr("Не работает", "Not working"), tint = color, modifier = Modifier.size(d + 4.dp))
        return
    }
    val text = when {
        short -> if (ms >= 0) "$ms" else "—"
        ms >= 0 -> tr("$ms мс", "$ms ms")
        else -> tr("нет ответа", "no response")
    }
    Text(text, style = style, color = color, maxLines = 1)
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PingFab(progress: Float?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = MaterialTheme.colorScheme
    val view = androidx.compose.ui.platform.LocalView.current
    val pinging = progress != null
    val round by animateFloatAsState(if (pinging) 1f else 0f, MaterialTheme.motionScheme.defaultSpatialSpec(), label = "fabRound")
    val ring by animateFloatAsState(if (pinging) 1f else 0f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "pingRing")
    val shown by animateFloatAsState(progress ?: 0f, MaterialTheme.motionScheme.slowEffectsSpec(), label = "pingProgress")
    val side = 56.dp
    val shape = androidx.compose.foundation.shape.RoundedCornerShape(16.dp + (side / 2 - 16.dp) * round)
    Surface(
        onClick = {
            view.haptic(if (pinging) Haptic.ToggleOff else Haptic.ToggleOn)
            onClick()
        },
        modifier = modifier.size(side),
        shape = shape,
        color = c.primaryContainer,
        contentColor = c.onPrimaryContainer
    ) {
        Box(contentAlignment = Alignment.Center) {
            if (ring > 0.01f) {
                androidx.compose.material3.CircularWavyProgressIndicator(
                    progress = { shown },
                    modifier = Modifier.size(46.dp).graphicsLayer {
                        scaleX = ring
                        scaleY = ring
                        alpha = ring
                    },
                    color = c.onPrimaryContainer,
                    trackColor = c.onPrimaryContainer.copy(alpha = 0.18f)
                )
            }
            val pop = MaterialTheme.motionScheme.fastSpatialSpec<Float>()
            AnimatedContent(
                pinging,
                transitionSpec = { (fadeIn(tween(160, 60)) + androidx.compose.animation.scaleIn(pop, 0.4f)) togetherWith (fadeOut(tween(90)) + androidx.compose.animation.scaleOut(tween(120), 0.4f)) },
                label = "fabIcon"
            ) { on ->
                Icon(
                    if (on) Glyph.Stop else Glyph.Bolt,
                    if (on) tr("Остановить проверку", "Stop checking") else tr("Проверить пинг", "Check ping"),
                    Modifier.size(if (on) 18.dp else 24.dp)
                )
            }
        }
    }
}
