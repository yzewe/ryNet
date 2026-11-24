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
fun SubsSheet(
    subs: List<Sub>,
    current: String?,
    refreshing: Set<String>,
    error: String?,
    onPick: (Sub) -> Unit,
    onRefresh: (Sub) -> Unit,
    onRefreshAll: () -> Unit,
    onDelete: (Sub) -> Unit,
    onAdd: () -> Unit,
    onDismiss: () -> Unit,
    servers: Map<String, Int> = emptyMap(),
    auto: Boolean = false,
    autoStatus: String? = null,
    onAuto: (() -> Unit)? = null
) {
    var close by remember { mutableLongStateOf(0L) }
    var then by remember { mutableStateOf<(() -> Unit)?>(null) }
    fun away(action: () -> Unit) {
        then = action
        close++
    }
    val list = rememberLazyListState()
    AppSheet(
        title = tr("Подписки", "Subscriptions"),
        onDismiss = {
            onDismiss()
            then?.invoke()
        },
        topDivider = list.canScrollBackward,
        bottomDivider = list.canScrollForward,
        dismissRequestId = close,
        footer = {
            Row(
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                if (subs.count { !Local.isLocal(it) } > 1) {
                    PillButton(
                        if (refreshing.isEmpty()) tr("Обновить все", "Update all") else tr("Обновляю…", "Updating…"),
                        onRefreshAll,
                        Modifier.weight(1f),
                        accent = false,
                        enabled = refreshing.size < subs.count { !Local.isLocal(it) },
                        padding = 16.dp,
                        container = MaterialTheme.colorScheme.surfaceContainerHighest,
                        contentColor = MaterialTheme.colorScheme.onSurface,
                        leading = {
                            if (refreshing.isNotEmpty()) {
                                LoadingIndicator(Modifier.size(20.dp), color = MaterialTheme.colorScheme.primary)
                            } else {
                                Icon(Icons.Rounded.Refresh, null, modifier = Modifier.size(20.dp))
                            }
                        }
                    )
                }
                PillButton(
                    if (subs.size > 1) tr("Добавить", "Add") else tr("Добавить подписку", "Add subscription"),
                    { away(onAdd) },
                    Modifier.weight(1f),
                    padding = 16.dp,
                    leading = { Icon(Icons.Rounded.Add, null, modifier = Modifier.size(20.dp)) }
                )
            }
        }
    ) {
        SubsContent(subs, current, refreshing, error, { away { onPick(it) } }, onRefresh, onRefreshAll, onDelete, onAdd = { away(onAdd) }, servers, list, auto, autoStatus, onAuto?.let { f -> { away(f) } })
    }
}

@Composable
fun SubsContent(
    subs: List<Sub>,
    current: String?,
    refreshing: Set<String>,
    error: String?,
    onPick: (Sub) -> Unit,
    onRefresh: (Sub) -> Unit,
    onRefreshAll: () -> Unit = {},
    onDelete: (Sub) -> Unit,
    onAdd: () -> Unit,
    servers: Map<String, Int> = emptyMap(),
    state: androidx.compose.foundation.lazy.LazyListState = rememberLazyListState(),
    auto: Boolean = false,
    autoStatus: String? = null,
    onAuto: (() -> Unit)? = null
) {
    val ctx = LocalContext.current
    val placement = Springs.spatial<IntOffset>()
    val ordered = remember(subs, current) { subs.sortedByDescending { it.id == current } }
    var confirm by remember { mutableStateOf<Sub?>(null) }
    confirm?.let { sub ->
        val local = Local.isLocal(sub)
        DeleteDialog(
            if (local) tr("Удалить свои серверы?", "Delete your servers?") else tr("Удалить подписку?", "Delete subscription?"),
            if (local) tr("Все добавленные вручную серверы будут удалены.", "All manually added servers will be deleted.") else tr("«${sub.info.title ?: sub.info.url}» и её серверы будут удалены.", "\"${sub.info.title ?: sub.info.url}\" and its servers will be deleted."),
            tr("Удалить", "Delete"),
            onConfirm = { onDelete(sub) },
            onDismiss = { confirm = null }
        )
    }
    LazyColumn(
        state = state,
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        if (onAuto != null) item(key = "auto") { AutoCard(auto, servers.values.sum(), autoStatus, onAuto, Modifier.enter(0).animateItem(placementSpec = placement)) }
        itemsIndexed(ordered, key = { _, it -> it.id }) { i, sub ->
            SubCard(
                sub = sub,
                servers = servers[sub.id],
                selected = sub.id == current,
                loading = sub.id in refreshing,
                onPick = { onPick(sub) },
                onRefresh = { onRefresh(sub) },
                onDelete = { confirm = sub },
                onAdd = onAdd,
                onSupport = { url -> runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, url.toUri())) } },
                modifier = Modifier.enter(i).animateItem(placementSpec = placement)
            )
        }
        if (error != null) {
            item(key = "error") { NoticeBlock(Icons.Rounded.Close, error, error = true) }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun SubCard(
    sub: Sub,
    servers: Int?,
    selected: Boolean,
    loading: Boolean,
    onPick: () -> Unit,
    onRefresh: () -> Unit,
    onDelete: () -> Unit,
    onAdd: () -> Unit,
    onSupport: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val info = sub.info
    val local = Local.isLocal(sub)
    val now = System.currentTimeMillis()
    val used = info.download + info.upload
    val left = if (info.expire > 0) info.expire * 1000 - now else null
    val bg by animateColorAsState(if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh, Springs.effects(), label = "sub")
    val content = if (selected) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface
    val muted = if (selected) content.copy(alpha = 0.72f) else MaterialTheme.colorScheme.onSurfaceVariant
    val chip = if (selected) MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.45f) else MaterialTheme.colorScheme.surfaceContainerHighest
    val title = sub.title
    val src = remember { MutableInteractionSource() }
    var qr by remember { mutableStateOf(false) }
    if (qr) QrSheet(title, info.url) { qr = false }
    Column(
        modifier
            .fillMaxWidth()
            .pressSquish(src, 0.98f)
            .clip(MaterialTheme.shapes.extraLarge)
            .background(bg)
            .combinedClickable(src, PressIndication, onClick = hapticOnClick(onClick = onPick))
            .padding(16.dp)
    ) {
        Row(Modifier.padding(start = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(title, color = content, style = MaterialTheme.typography.titleLargeEmphasized, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(servers?.let { plural(it, "сервер", "сервера", "серверов", "server") }, (tr("обновлена ", "updated ") + ago(info.updatedAt, now)).takeIf { !local }).joinToString(" · "),
                    color = muted,
                    style = MaterialTheme.typography.labelLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
            }
            if (!local) {
                LocalSubEdit.current?.let { edit ->
                    IconButton(onClick = hapticOnClick { edit(sub) }, modifier = Modifier.size(40.dp)) {
                        Icon(Icons.Rounded.Edit, tr("Изменить", "Edit"), tint = muted, modifier = Modifier.size(20.dp))
                    }
                }
            }
            AnimatedVisibility(selected, enter = fadeIn(), exit = fadeOut()) {
                Box(Modifier.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary), contentAlignment = Alignment.Center) {
                    Icon(Icons.Rounded.Check, tr("Текущая", "Current"), tint = MaterialTheme.colorScheme.onPrimary, modifier = Modifier.size(18.dp))
                }
            }
        }
        if (!local) Spacer(Modifier.height(12.dp))
        if (!local) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Chip(
                    when {
                        info.total > 0 -> tr("${bytes(used)} из ${bytes(info.total)}", "${bytes(used)} of ${bytes(info.total)}")
                        used > 0 -> bytes(used)
                        else -> tr("без лимита", "unlimited")
                    },
                    icon = Glyph.Data,
                    container = chip,
                    content = content
                )
                if (left != null) {
                    val days = left / DateUtils.DAY_IN_MILLIS
                    val soon = left < 3 * DateUtils.DAY_IN_MILLIS
                    Chip(
                        when {
                            left <= 0 -> tr("истекла", "expired")
                            days == 0L -> tr("последний день", "last day")
                            else -> plural(days.toInt(), "день", "дня", "дней", "day")
                        },
                        icon = Glyph.Timer,
                        container = if (soon) MaterialTheme.colorScheme.errorContainer else chip,
                        content = if (soon) MaterialTheme.colorScheme.onErrorContainer else content
                    )
                }
            }
        }
        if (info.total > 0) {
            Spacer(Modifier.height(12.dp))
            LinearWavyProgressIndicator(
                progress = { (used.toFloat() / info.total).coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp),
                color = MaterialTheme.colorScheme.primary,
                trackColor = chip,
                waveSpeed = 0.dp
            )
        }
        info.announce?.let {
            SwipeAway(sub.id, it) {
                Row(
                    Modifier.padding(top = 12.dp).fillMaxWidth().clip(MaterialTheme.shapes.medium).background(chip).padding(12.dp),
                    verticalAlignment = Alignment.Top
                ) {
                    Icon(Glyph.Megaphone, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(it, color = content, style = MaterialTheme.typography.bodyMedium, maxLines = 6, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        Spacer(Modifier.height(12.dp))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (local) {
                PillButton(
                    tr("Добавить сервер", "Add server"),
                    onAdd,
                    Modifier.weight(1f),
                    accent = false,
                    height = 44.dp,
                    container = chip,
                    contentColor = content,
                    leading = { Icon(Icons.Rounded.Add, null, modifier = Modifier.size(18.dp)) }
                )
            } else {
                PillButton(
                    if (loading) tr("Обновляю", "Updating") else tr("Обновить", "Update"), onRefresh, Modifier.weight(1f), enabled = !loading, accent = false, height = 44.dp,
                    container = chip, contentColor = content,
                    leading = {
                        if (loading) {
                            LoadingIndicator(Modifier.size(18.dp), color = MaterialTheme.colorScheme.primary)
                        } else {
                            Icon(Icons.Rounded.Refresh, null, modifier = Modifier.size(18.dp))
                        }
                    }
                )
            }
            info.supportUrl?.let { url ->
                CircleIconButton(Glyph.Support, tr("Поддержка", "Support"), { onSupport(url) }, size = 44.dp, container = chip, tint = content)
            }
            if (!local) CircleIconButton(Glyph.Qr, tr("QR-код", "QR code"), { qr = true }, size = 44.dp, container = chip, tint = content)
            CircleIconButton(
                Icons.Rounded.Delete,
                tr("Удалить", "Delete"),
                onDelete,
                size = 44.dp,
                container = chip,
                tint = if (selected) content else MaterialTheme.colorScheme.error
            )
        }
    }
}
