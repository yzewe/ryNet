@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import android.text.format.DateUtils
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import dev.yzewe.rynet.data.Local
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlinx.coroutines.launch

class SubActions(
    val onRefresh: (Sub) -> Unit,
    val onEdit: (Sub) -> Unit,
    val onCopyAll: (Sub) -> Unit,
    val onRemoveDead: (Sub) -> Unit,
    val onDelete: (Sub) -> Unit,
    val onAddServer: () -> Unit
)

@Composable
fun ServersScreen(
    subs: List<Sub>,
    shown: String?,
    servers: Map<String, Int>,
    refreshing: Set<String>,
    auto: Boolean,
    onShow: (String) -> Unit,
    onAuto: () -> Unit,
    onAdd: () -> Unit,
    onRefreshAll: () -> Unit,
    actions: SubActions,
    onBack: () -> Unit,
    ping: Float?,
    onPing: (sub: String, names: List<String>?) -> Unit,
    onStopPing: () -> Unit,
    pane: Boolean = false,
    all: Boolean = false,
    allPing: Float? = null,
    onPingAll: () -> Unit = {},
    allPage: @Composable (query: String) -> Unit = {},
    list: @Composable (sub: String, query: String, onFound: (List<String>?) -> Unit) -> Unit
) {
    val c = MaterialTheme.colorScheme
    var query by rememberSaveable { mutableStateOf("") }
    var searching by rememberSaveable { mutableStateOf(false) }
    var menu by remember { mutableStateOf<String?>(null) }
    val pick = remember { HoldPick() }
    BackHandler(searching) {
        searching = false
        query = ""
    }
    val remote = subs.count { !Local.isLocal(it) }
    val first = if (all) 1 else 0
    val start = if (all) 0 else subs.indexOfFirst { it.id == shown }.coerceAtLeast(0)
    val pager = androidx.compose.foundation.pager.rememberPagerState(start) { subs.size + first }
    val chips = androidx.compose.foundation.lazy.rememberLazyListState()
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val current = subs.getOrNull(pager.currentPage - first)
    val found = remember { androidx.compose.runtime.mutableStateMapOf<String, List<String>?>() }
    val lead = if (auto && !all) 1 else 0
    LaunchedEffect(pager.currentPage) {
        subs.getOrNull(pager.currentPage - first)?.let { onShow(it.id) }
        runCatching { chips.animateScrollToItem((pager.currentPage + lead - 1).coerceAtLeast(0)) }
    }
    val nav = if (pane) Modifier else Modifier.navigationBarsPadding()
    Box(Modifier.fillMaxSize().then(if (pane) Modifier.clip(MaterialTheme.shapes.extraLarge) else Modifier).background(c.surface)) {
        Column(Modifier.fillMaxSize()) {
            SearchBar(
                searching, query, refreshing.isNotEmpty(), remote > 0, pane, onBack, onRefreshAll,
                onQuery = { query = it },
                onOpen = { searching = true },
                onClose = {
                    searching = false
                    query = ""
                }
            )
            androidx.compose.animation.AnimatedVisibility(
                !searching && !pane,
                enter = androidx.compose.animation.expandVertically(MaterialTheme.motionScheme.defaultSpatialSpec()) + fadeIn(MaterialTheme.motionScheme.defaultEffectsSpec()),
                exit = androidx.compose.animation.shrinkVertically(MaterialTheme.motionScheme.defaultSpatialSpec()) + fadeOut(MaterialTheme.motionScheme.fastEffectsSpec())
            ) {
                Text(
                    tr("Серверы", "Servers"),
                    style = PageType.title,
                    color = c.onSurface,
                    modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 12.dp)
                )
            }
            LazyRow(
                state = chips,
                contentPadding = PaddingValues(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                modifier = Modifier.padding(top = 4.dp, bottom = 12.dp)
            ) {
                if (all) {
                    item(key = "all") { SubChip(tr("Все серверы", "All servers"), pager.currentPage == 0) { scope.launch { pager.animateScrollToPage(0) } } }
                } else if (auto) {
                    item(key = "auto") { SubChip(tr("Автовыбор", "Auto select"), true, icon = Glyph.Auto, onClick = onAuto) }
                }
                itemsIndexed(subs, key = { _, it -> it.id }) { j, x ->
                    val i = j + first
                    Box {
                        SubChip(
                            subTitle(x),
                            i == pager.currentPage,
                            pick = pick,
                            loading = x.id in refreshing,
                            onLongClick = { menu = x.id }
                        ) { if (i == pager.currentPage) menu = x.id else scope.launch { pager.animateScrollToPage(i) } }
                        SubMenu(pick, x, menu == x.id, servers[x.id], x.id in refreshing, actions) { menu = null }
                    }
                }
                item(key = "add") { SubChip(tr("Добавить", "Add"), false, icon = Icons.Rounded.Add, onClick = onAdd) }
            }
            androidx.compose.foundation.pager.HorizontalPager(
                pager,
                Modifier.weight(1f),
                key = { if (all && it == 0) "\u0000all" else subs.getOrNull(it - first)?.id ?: it },
                beyondViewportPageCount = 0
            ) { i ->
                if (all && i == 0) {
                    allPage(query)
                } else {
                    subs.getOrNull(i - first)?.let { x -> list(x.id, query) { found[x.id] = it } }
                }
            }
        }
        if (current != null) {
            PingFab(
                ping,
                { if (ping != null) onStopPing() else onPing(current.id, if (query.isBlank()) null else found[current.id].orEmpty()) },
                Modifier.align(Alignment.BottomEnd).then(nav).padding(end = 16.dp, bottom = 16.dp)
            )
        } else if (all && pager.currentPage == 0 && subs.isNotEmpty()) {
            PingFab(
                allPing,
                { if (allPing != null) onStopPing() else onPingAll() },
                Modifier.align(Alignment.BottomEnd).then(nav).padding(end = 16.dp, bottom = 16.dp)
            )
        }
    }
}

internal fun subTitle(x: Sub): String = if (Local.isLocal(x)) tr("Свои серверы", "My servers") else x.title

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SubChip(
    text: String,
    selected: Boolean,
    icon: ImageVector? = null,
    loading: Boolean = false,
    pick: HoldPick? = null,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit
) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    val bg by animateColorAsState(if (selected) c.primary else c.surfaceContainerHigh, Springs.effects(), label = "chipBg")
    val fg by animateColorAsState(if (selected) c.onPrimary else c.onSurfaceVariant, Springs.effects(), label = "chipFg")
    val corner by androidx.compose.animation.core.animateDpAsState(if (selected) 18.dp else 10.dp, MaterialTheme.motionScheme.fastSpatialSpec(), label = "chipCorner")
    Row(
        Modifier
            .pressSquish(src, 0.94f)
            .heightIn(min = 36.dp)
            .clip(androidx.compose.foundation.shape.RoundedCornerShape(corner))
            .background(bg)
            .then(
                if (pick != null && onLongClick != null) {
                    Modifier.indication(src, PressIndication).holdMenu(pick, onClick, src) { onLongClick() }
                } else {
                    Modifier.clickable(src, PressIndication) {
                        view.haptic(Haptic.Segment)
                        onClick()
                    }
                }
            )
            .padding(start = if (icon != null || loading) 12.dp else 16.dp, end = 16.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (loading) {
            LoadingIndicator(Modifier.size(20.dp), color = fg)
            Spacer(Modifier.width(6.dp))
        } else if (icon != null) {
            Icon(icon, null, tint = fg, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(text, color = fg, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun SubMenu(pick: HoldPick, sub: Sub, open: Boolean, servers: Int?, loading: Boolean, actions: SubActions, close: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val info = sub.info
    val local = Local.isLocal(sub)
    val now = System.currentTimeMillis()
    val used = info.download + info.upload
    val left = if (info.expire > 0) info.expire * 1000 - now else null
    var qr by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf(false) }
    if (qr) QrSheet(subTitle(sub), info.url) { qr = false }
    if (removing) {
        DeleteDialog(
            if (local) tr("Удалить свои серверы?", "Delete your servers?") else tr("Удалить подписку?", "Delete subscription?"),
            if (local) tr("Все добавленные вручную серверы будут удалены.", "All manually added servers will be deleted.") else tr("«${subTitle(sub)}» и её серверы будут удалены.", "\"${subTitle(sub)}\" and its servers will be deleted."),
            tr("Удалить", "Delete"),
            onConfirm = { actions.onDelete(sub) },
            onDismiss = { removing = false }
        )
    }
    data class Act(val icon: ImageVector, val text: String, val run: () -> Unit)
    val main = buildList {
        if (local) {
            add(
                Act(Icons.Rounded.Add, tr("Добавить сервер", "Add server")) {
                    close()
                    actions.onAddServer()
                }
            )
            add(
                Act(Glyph.Bolt, tr("Убрать нерабочие", "Remove dead ones")) {
                    close()
                    actions.onRemoveDead(sub)
                }
            )
        } else {
            add(Act(Icons.Rounded.Refresh, if (loading) tr("Обновляю…", "Updating…") else tr("Обновить", "Update")) { if (!loading) actions.onRefresh(sub) })
            add(
                Act(Icons.Rounded.Edit, tr("Изменить", "Edit")) {
                    close()
                    actions.onEdit(sub)
                }
            )
        }
        add(
            Act(Glyph.Copy, tr("Скопировать конфиги", "Copy configs")) {
                close()
                actions.onCopyAll(sub)
            }
        )
        if (!local) {
            add(
                Act(Glyph.Qr, tr("QR-код", "QR code")) {
                    close()
                    qr = true
                }
            )
        }
        info.supportUrl?.takeIf { !local }?.let { url ->
            add(
                Act(Glyph.Support, tr("Поддержка", "Support")) {
                    close()
                    runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, url.toUri())) }
                }
            )
        }
    }
    androidx.compose.material3.DropdownMenuPopup(expanded = open, onDismissRequest = close) {
        val groups = 3
        androidx.compose.material3.DropdownMenuGroup(shapes = androidx.compose.material3.MenuDefaults.groupShape(0, groups)) {
            Column(Modifier.widthIn(min = 240.dp, max = 280.dp).padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(subTitle(sub), style = MaterialTheme.typography.titleMedium, color = c.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(
                        servers?.let { plural(it, "сервер", "сервера", "серверов", "server") },
                        (tr("обновлена ", "updated ") + ago(info.updatedAt, now)).takeIf { !local }
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = c.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
                if (!local) {
                    val soon = left != null && left < 3 * DateUtils.DAY_IN_MILLIS
                    androidx.compose.foundation.layout.FlowRow(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Glyph.Data, null, tint = c.onSurfaceVariant, modifier = Modifier.size(16.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(
                                when {
                                    info.total > 0 -> tr("${bytes(used)} из ${bytes(info.total)}", "${bytes(used)} of ${bytes(info.total)}")
                                    used > 0 -> bytes(used)
                                    else -> tr("без лимита", "unlimited")
                                },
                                style = MaterialTheme.typography.labelLarge,
                                color = c.onSurface,
                                maxLines = 1
                            )
                        }
                        if (left != null) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(Glyph.Timer, null, tint = if (soon) c.error else c.onSurfaceVariant, modifier = Modifier.size(16.dp))
                                Spacer(Modifier.width(6.dp))
                                val days = left / DateUtils.DAY_IN_MILLIS
                                Text(
                                    when {
                                        left <= 0 -> tr("истекла", "expired")
                                        days == 0L -> tr("последний день", "last day")
                                        else -> plural(days.toInt(), "день", "дня", "дней", "day")
                                    },
                                    style = MaterialTheme.typography.labelLarge,
                                    color = if (soon) c.error else c.onSurface,
                                    maxLines = 1
                                )
                            }
                        }
                    }
                    if (info.total > 0) {
                        LinearWavyProgressIndicator(
                            progress = { (used.toFloat() / info.total).coerceIn(0f, 1f) },
                            modifier = Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 2.dp),
                            color = c.primary,
                            trackColor = c.surfaceContainerHighest,
                            waveSpeed = 0.dp
                        )
                    }
                }
                info.announce?.takeIf { it.isNotBlank() }?.let { text ->
                    Text(
                        text.trim(),
                        style = MaterialTheme.typography.bodySmall,
                        color = c.onSurfaceVariant,
                        maxLines = 8,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(top = 6.dp)
                    )
                }
            }
        }
        Spacer(Modifier.height(androidx.compose.material3.MenuDefaults.GroupSpacing))
        androidx.compose.material3.DropdownMenuGroup(shapes = androidx.compose.material3.MenuDefaults.groupShape(1, groups)) {
            main.forEachIndexed { i, a ->
                HoldItem(
                    pick,
                    i,
                    a.text,
                    {
                        if (a.icon == Icons.Rounded.Refresh && loading) {
                            LoadingIndicator(Modifier.size(24.dp), color = c.primary)
                        } else {
                            Icon(a.icon, null)
                        }
                    },
                    androidx.compose.material3.MenuDefaults.itemShape(i, main.size).shape,
                    onClick = a.run
                )
            }
        }
        Spacer(Modifier.height(androidx.compose.material3.MenuDefaults.GroupSpacing))
        androidx.compose.material3.DropdownMenuGroup(shapes = androidx.compose.material3.MenuDefaults.groupShape(2, groups)) {
            HoldItem(
                pick,
                99,
                tr("Удалить", "Delete"),
                { HoldIcon(Icons.Rounded.Delete, danger = true) },
                androidx.compose.material3.MenuDefaults.itemShape(0, 1).shape,
                danger = true
            ) {
                close()
                removing = true
            }
        }
    }
}

@Composable
private fun SearchBar(
    open: Boolean,
    query: String,
    busy: Boolean,
    canRefresh: Boolean,
    pane: Boolean,
    onBack: () -> Unit,
    onRefreshAll: () -> Unit,
    onQuery: (String) -> Unit,
    onOpen: () -> Unit,
    onClose: () -> Unit
) {
    val c = MaterialTheme.colorScheme
    val k by androidx.compose.animation.core.animateFloatAsState(if (open) 1f else 0f, MaterialTheme.motionScheme.defaultSpatialSpec(), label = "searchOpen")
    val fx by androidx.compose.animation.core.animateFloatAsState(if (open) 1f else 0f, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "searchFx")
    val focus = remember { FocusRequester() }
    LaunchedEffect(open) {
        if (!open) return@LaunchedEffect
        androidx.compose.runtime.withFrameNanos { }
        runCatching { focus.requestFocus() }
    }
    Row(
        Modifier.fillMaxWidth().then(if (pane) Modifier else Modifier.statusBarsPadding()).padding(horizontal = 16.dp, vertical = 8.dp).height(56.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (!pane) {
            CircleBack(onBack)
            Spacer(Modifier.width(8.dp))
        }
        androidx.compose.foundation.layout.BoxWithConstraints(Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
            val full = maxWidth
            val side = 48.dp
            val width = (48.dp + (full - 48.dp) * k).coerceIn(48.dp, full)
            val bg = androidx.compose.ui.graphics.lerp(c.surfaceContainerHigh, c.surfaceContainerHighest, fx)
            Row(
                Modifier
                    .width(width)
                    .height(side)
                    .clip(CircleShape)
                    .background(bg)
                    .then(if (!open) Modifier.clickable { onOpen() } else Modifier),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Box(Modifier.size(side), contentAlignment = Alignment.Center) {
                    Icon(Icons.Rounded.Search, tr("Поиск", "Search"), tint = c.onSurface, modifier = Modifier.size(24.dp))
                }
                if (open || k > 0.02f) {
                    Box(Modifier.weight(1f).graphicsLayer { alpha = fx }, contentAlignment = Alignment.CenterStart) {
                        if (query.isEmpty()) Text(tr("Страна, город или протокол", "Country, city or protocol"), style = MaterialTheme.typography.bodyLarge, color = c.onSurfaceVariant, maxLines = 1)
                        androidx.compose.foundation.text.BasicTextField(
                            query,
                            onQuery,
                            singleLine = true,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = c.onSurface),
                            cursorBrush = androidx.compose.ui.graphics.SolidColor(c.primary),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus).remoteField()
                        )
                    }
                }
                if (open || k > 0.02f) {
                    androidx.compose.material3.IconButton(onClick = onClose, modifier = Modifier.graphicsLayer { alpha = fx }) {
                        Icon(Icons.Rounded.Close, tr("Закрыть поиск", "Close search"), tint = c.onSurfaceVariant)
                    }
                }
            }
        }
        if (canRefresh) {
            Spacer(Modifier.width(8.dp))
            Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                if (busy) {
                    LoadingIndicator(Modifier.size(40.dp), color = c.primary)
                } else {
                    CircleIconButton(Icons.Rounded.Refresh, tr("Обновить все", "Update all"), onRefreshAll, size = 48.dp)
                }
            }
        }
    }
}
