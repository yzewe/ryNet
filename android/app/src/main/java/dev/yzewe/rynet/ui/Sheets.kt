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
fun ServersSheet(
    profiles: List<Profile>,
    selected: String?,
    pings: Map<String, Int>,
    pinging: Boolean,
    progress: Pair<Int, Int> = 0 to 0,
    sort: ServerSort,
    view: ServerView,
    errors: Map<String, String> = emptyMap(),
    waiting: (String) -> Boolean = { false },
    grid: ServerGrid,
    info: ServerInfo,
    onPing: (List<String>?) -> Unit,
    onSelect: (Profile) -> Unit,
    onDismiss: () -> Unit,
    onStopPing: () -> Unit = {},
    onCheck: (Profile) -> Unit = {},
    onSpeed: ((Profile) -> Unit)? = null,
    title: String = tr("Серверы", "Servers"),
    header: @Composable () -> Unit = {},
    contentKey: Any? = null,
    edit: ServerEdit? = null,
    prefs: ServerPrefs? = null
) {
    var close by remember { mutableLongStateOf(0L) }
    var chosen by remember { mutableStateOf<Profile?>(null) }
    var query by rememberSaveable { mutableStateOf("") }
    var focused by remember { mutableStateOf(false) }
    val searching = focused || query.isNotBlank()
    AppSheet(
        title = title,
        onDismiss = {
            chosen?.let(onSelect)
            onDismiss()
        },
        dismissRequestId = close,
        edgeToEdgeBottom = true,
        fillMaxHeight = searching,
        animateHeight = true
    ) {
        Column(
            Modifier
                .then(if (searching) Modifier.fillMaxHeight() else Modifier)
                .windowInsetsPadding(WindowInsets.ime.exclude(WindowInsets.navigationBars))
                .hideKeyboardOnScroll(focused)
        ) {
            header()
            SearchField(
                query,
                { query = it },
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 8.dp).onFocusChanged { focused = it.hasFocus },
                hint = tr("Поиск серверов", "Search servers")
            )
            AnimatedContent(
                targetState = contentKey to profiles,
                modifier = if (searching) Modifier.weight(1f) else Modifier,
                contentKey = { it.first },
                transitionSpec = { fadeIn(tween(220, delayMillis = 60)) togetherWith fadeOut(tween(90)) },
                label = "servers"
            ) { (_, list) ->
                ServersContent(
                    list, selected, pings, pinging, sort, view, grid, onPing,
                    info = info, errors = errors, onCheck = onCheck, progress = progress, waiting = waiting, onSpeed = onSpeed, edit = edit,
                    query = query, fill = searching, prefs = prefs, onStopPing = onStopPing
                ) { p ->
                    chosen = p
                    close++
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun ServersContent(
    profiles: List<Profile>,
    selected: String?,
    pings: Map<String, Int>,
    pinging: Boolean,
    sort: ServerSort,
    view: ServerView = ServerView.Groups,
    grid: ServerGrid = ServerGrid.Two,
    onPing: (List<String>?) -> Unit,
    info: ServerInfo = ServerInfo.Description,
    errors: Map<String, String> = emptyMap(),
    onCheck: (Profile) -> Unit = {},
    state: androidx.compose.foundation.lazy.grid.LazyGridState = rememberLazyGridState(),
    progress: Pair<Int, Int> = 0 to 0,
    waiting: (String) -> Boolean = { false },
    onSpeed: ((Profile) -> Unit)? = null,
    columns: Int? = null,
    navInset: Boolean = true,
    edit: ServerEdit? = null,
    query: String = "",
    fill: Boolean = false,
    prefs: ServerPrefs? = null,
    onStopPing: () -> Unit = {},
    pingFab: Boolean = true,
    pingWave: Boolean = true,
    header: (@Composable () -> Unit)? = null,
    bottom: Dp? = null,
    placementAnimation: Boolean = true,
    onFound: ((List<String>?) -> Unit)? = null,
    stretch: Boolean = false,
    onSelect: (Profile) -> Unit
) {
    var details by remember { mutableStateOf<Profile?>(null) }
    var balancer by remember { mutableStateOf<Profile?>(null) }
    balancer?.let { b ->
        BalancerSheet(b, selected, pings, errors, pinging, progress, waiting, grid, info, onCheck, onSpeed, prefs, onDismiss = { balancer = null }, onSelect = onSelect)
    }
    val longPress = { p: Profile -> if (p.members.isNotEmpty()) balancer = p else details = p }
    details?.let { p ->
        ServerDetails(p, pings[p.remarks], errors[p.remarks], pinging, p.remarks == selected, waiting = waiting(p.remarks), onCheck = { onCheck(p) }, onSpeed = onSpeed?.let { f -> { f(p) } }, onDismiss = { details = null }, edit = edit, prefs = prefs) {
            onSelect(p)
        }
    }
    val grouped = remember(profiles, view) {
        val all = Subscription.sections(profiles)
        if (view == ServerView.Single) all.filter { it.items.isEmpty() } + Section("", all.flatMap { it.items }) else all
    }
    val notices = LocalNotices.current
    val q = query.trim()
    val found = remember(grouped, q) { search(grouped, q) }
    if (onFound != null) {
        LaunchedEffect(found, q) {
            onFound(if (q.isEmpty()) null else found.flatMap { it.items }.filter { it.supported && !it.placeholder }.map { it.remarks })
        }
    }
    var picked by remember { mutableStateOf<String?>(null) }
    val byName = remember(found, sort) { if (sort == ServerSort.Ping) found else sorted(found, sort, emptyMap()) }
    val sections = if (sort == ServerSort.Ping) remember(byName, pings) { sorted(byName, sort, pings) } else byName
    val keys = remember(profiles) {
        val seen = HashMap<String, Int>()
        IdentityHashMap<Profile, String>().apply {
            for (p in profiles) {
                val n = seen.merge(p.remarks, 1, Int::plus)!!
                put(p, if (n == 1) "p:${p.remarks}" else "p:${p.remarks}#$n")
            }
        }
    }
    val placement = if (placementAnimation) Springs.spatial<IntOffset>() else null
    val cols = columns ?: grid.columns
    val since = rememberEnterStart()
    val nav = if (navInset) navBottom() else 0.dp
    val current = if (q.isEmpty() && !fill) profiles.byRemarks(selected) else null
    val pinned = remember(profiles, prefs?.pinned, q, current, fill) {
        if (q.isNotEmpty() || fill || prefs == null) {
            emptyList()
        } else {
            profiles.pinnedBy(prefs.pinned).filter { it.remarks != current?.remarks }
        }
    }
    val lastQuery = remember { arrayOf(q) }
    LaunchedEffect(q) {
        if (q != lastQuery[0]) {
            lastQuery[0] = q
            state.scrollToItem(0)
        }
    }
    Column(if (fill || stretch) Modifier.fillMaxHeight() else Modifier) {
        if (pingWave) PingWave(pinging, progress)
        Box(Modifier.weight(1f, fill = fill || stretch)) {
            LazyVerticalGrid(
                state = state,
                columns = GridCells.Fixed(columns ?: grid.columns),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = bottom ?: (nav + 104.dp)),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                header?.let { h -> item(key = "header", span = { GridItemSpan(maxLineSpan) }, contentType = "header") { h() } }
                current?.let { p ->
                    item(key = "current:" + (keys[p] ?: p.remarks), span = { GridItemSpan(maxLineSpan) }, contentType = "current") {
                        val land = remember { androidx.compose.animation.core.Animatable(if (picked == p.remarks) 0.86f else 1f) }
                        LaunchedEffect(Unit) { if (land.value < 1f) land.animateTo(1f, androidx.compose.animation.core.spring(0.45f, 420f)) }
                        ServerTile(
                            p, pings[p.remarks], errors[p.remarks], true, true, info,
                            Modifier.animateItem(fadeInSpec = Springs.effects(), placementSpec = placement, fadeOutSpec = Springs.effects())
                                .graphicsLayer {
                                    scaleX = land.value
                                    scaleY = land.value
                                }
                                .padding(bottom = 4.dp),
                            waiting = waiting(p.remarks), onLongClick = { longPress(p) }
                        ) {
                            picked = p.remarks
                            onSelect(p)
                        }
                    }
                }
                if (q.isNotEmpty() && sections.isEmpty()) {
                    item(key = "none", span = { GridItemSpan(maxLineSpan) }, contentType = "none") {
                        Text(
                            tr("Ничего не найдено", "Nothing found"),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyLarge,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 32.dp)
                        )
                    }
                }
                if (pinned.isNotEmpty()) {
                    item(key = "pinned", span = { GridItemSpan(maxLineSpan) }, contentType = "section") {
                        Box { SectionHeader(Section(tr("Закреплённые", "Pinned"), pinned), pinned.size) }
                    }
                    itemsIndexed(pinned, key = { _, p -> "pin:" + (keys[p] ?: p.remarks) }, contentType = { _, _ -> "server" }) { j, p ->
                        ServerTile(
                            p, pings[p.remarks], errors[p.remarks], p.remarks == selected, (columns ?: grid.columns) == 1, info,
                            Modifier.animateItem(placementSpec = placement).enter(j / cols, since),
                            waiting = waiting(p.remarks), onLongClick = { longPress(p) }
                        ) {
                            picked = p.remarks
                            onSelect(p)
                        }
                    }
                }
                var base = (pinned.size + cols - 1) / cols
                sections.forEachIndexed { i, full ->
                    val section = full.copy(items = full.items.filter { it !== current && it !in pinned })
                    if (full.items.isNotEmpty() && section.items.isEmpty()) return@forEachIndexed
                    val gone = section.items.isEmpty() && notices?.hidden("", noticeText(section)) == true
                    if (!gone && (section.title.isNotEmpty() || section.note != null)) {
                        item(key = "s$i", span = { GridItemSpan(maxLineSpan) }, contentType = "section") { Box { SectionHeader(section, full.items.size) } }
                    }
                    val from = base
                    base += (section.items.size + cols - 1) / cols
                    itemsIndexed(section.items, key = { _, p -> keys[p] ?: p.remarks }, contentType = { _, _ -> "server" }) { j, p ->
                        ServerTile(
                            p, pings[p.remarks], errors[p.remarks], p.remarks == selected, (columns ?: grid.columns) == 1, info,
                            Modifier.animateItem(placementSpec = placement).then(if (q.isEmpty()) Modifier.enter(from + j / cols, since) else Modifier),
                            waiting = waiting(p.remarks), onLongClick = { longPress(p) }
                        ) {
                            picked = p.remarks
                            onSelect(p)
                        }
                    }
                }
            }
            if (pingFab) {
                PingFab(
                    if (pinging) progress.let { (d, t) -> if (t > 0) d.toFloat() / t else 0f } else null,
                    {
                        if (pinging) onStopPing() else onPing(if (q.isEmpty()) null else found.flatMap { it.items }.filter { it.supported && !it.placeholder }.map { it.remarks })
                    },
                    Modifier.align(Alignment.BottomEnd).padding(end = 16.dp, bottom = nav + 16.dp)
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun PingWave(pinging: Boolean, progress: Pair<Int, Int>) {
    val (done, total) = progress
    val target = if (total > 0) done.toFloat() / total else 0f
    val p by animateFloatAsState(target, Springs.spatial(), label = "ping")
    AnimatedVisibility(
        pinging,
        enter = fadeIn() + expandVertically(expandFrom = Alignment.Top),
        exit = fadeOut() + shrinkVertically(shrinkTowards = Alignment.Top)
    ) {
        Box(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 2.dp, bottom = 8.dp)) {
            LinearWavyProgressIndicator(progress = { p }, Modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.primary, trackColor = MaterialTheme.colorScheme.surfaceContainerHighest)
        }
    }
}

private fun norm(s: String) = s.lowercase().replace('ё', 'е')

internal fun queryWords(query: String): List<String> = norm(query).split(' ').filter { it.isNotEmpty() }

internal fun Profile.matches(words: List<String>, extra: String = ""): Boolean {
    val text = norm(listOfNotNull(name, kind, description).joinToString(" ")) + " " + norm(extra)
    return words.all { it in text }
}

internal fun search(sections: List<Section>, query: String): List<Section> {
    val words = queryWords(query)
    if (words.isEmpty()) return sections
    return sections.mapNotNull { s ->
        val items = s.items.filter { it.matches(words, s.title) }
        if (items.isEmpty()) null else s.copy(items = items)
    }
}

internal fun sorted(sections: List<Section>, sort: ServerSort, pings: Map<String, Int>): List<Section> = when (sort) {
    ServerSort.Default -> sections

    ServerSort.Ping -> sections.map { s ->
        s.copy(
            items = s.items.sortedWith(
                compareBy<Profile> { it.members.isEmpty() }.thenBy { p ->
                    when (val v = pings[p.remarks]) {
                        null -> Int.MAX_VALUE - 2
                        Pings.FAILED -> Int.MAX_VALUE
                        in Int.MIN_VALUE..-1 -> Int.MAX_VALUE - 1
                        else -> v
                    }
                }
            )
        )
    }

    ServerSort.Name -> {
        val c = Collator.getInstance()
        sections.map { s -> s.copy(items = s.items.sortedWith(compareBy<Profile> { it.members.isEmpty() }.thenBy(c) { it.name })) }
    }
}

private fun noticeText(s: Section) = listOf(s.title, s.note.orEmpty()).filter { it.isNotBlank() }.joinToString("\n")

@Composable
private fun SectionHeader(s: Section, count: Int) {
    if (s.items.isEmpty()) {
        val text = noticeText(s)
        SwipeAway("", text) { NoticeBlock(Glyph.Megaphone, text) }
    } else {
        Column(Modifier.padding(start = 4.dp, top = 12.dp, bottom = 4.dp)) {
            if (s.title.isNotEmpty()) {
                Text(
                    buildAnnotatedString {
                        append(s.title)
                        withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurfaceVariant)) { append(" · $count") }
                    },
                    color = MaterialTheme.colorScheme.primary,
                    style = MaterialTheme.typography.titleSmallEmphasized
                )
            }
            s.note?.let { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall) }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun BalancerSheet(
    b: Profile,
    selected: String?,
    pings: Map<String, Int>,
    errors: Map<String, String>,
    pinging: Boolean,
    progress: Pair<Int, Int>,
    waiting: (String) -> Boolean,
    grid: ServerGrid,
    info: ServerInfo,
    onCheck: (Profile) -> Unit,
    onSpeed: ((Profile) -> Unit)?,
    prefs: ServerPrefs?,
    onDismiss: () -> Unit,
    onSelect: (Profile) -> Unit
) {
    var close by remember { mutableLongStateOf(0L) }
    var chosen by remember { mutableStateOf<Profile?>(null) }
    var details by remember { mutableStateOf<Profile?>(null) }
    val checkAll = { onCheck(b.copy(remarks = b.membersKey)) }
    LaunchedEffect(b.remarks) {
        if (!pinging && b.members.none { pings[it.remarks] != null }) checkAll()
    }
    details?.let { p ->
        ServerDetails(p, pings[p.remarks], errors[p.remarks], pinging, p.remarks == selected, waiting = waiting(p.remarks), onCheck = { onCheck(p) }, onSpeed = onSpeed?.let { f -> { f(p) } }, onDismiss = { details = null }, prefs = prefs) {
            chosen = p
            close++
        }
    }
    val state = rememberLazyGridState()
    val since = rememberEnterStart()
    AppSheet(
        title = b.name,
        onDismiss = {
            chosen?.let(onSelect)
            onDismiss()
        },
        dismissRequestId = close,
        topDivider = state.canScrollBackward,
        bottomDivider = state.canScrollForward,
        footer = {
            Column {
                if (prefs != null) Box(Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp)) { PrefActions(b, prefs) }
                Row(
                    Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = if (prefs != null) 8.dp else 12.dp, bottom = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    PillButton(
                        tr("Проверить", "Check"),
                        checkAll,
                        Modifier.weight(1f),
                        accent = false,
                        enabled = !pinging,
                        container = MaterialTheme.colorScheme.surfaceContainerHighest,
                        leading = {
                            if (pinging) {
                                LoadingIndicator(Modifier.size(22.dp), color = MaterialTheme.colorScheme.primary)
                            } else {
                                Icon(Glyph.Bolt, null, modifier = Modifier.size(20.dp))
                            }
                        }
                    )
                    PillButton(
                        if (b.remarks == selected) tr("Выбран", "Selected") else tr("Подключиться", "Connect"),
                        {
                            chosen = b
                            close++
                        },
                        Modifier.weight(1f),
                        enabled = b.supported
                    )
                }
            }
        }
    ) {
        Column {
            PingWave(pinging && b.members.any { waiting(it.remarks) }, progress)
            LazyVerticalGrid(
                state = state,
                columns = GridCells.Fixed(grid.columns),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                b.description?.let { d ->
                    item(key = "note", span = { GridItemSpan(maxLineSpan) }) { NoticeBlock(Glyph.Megaphone, d) }
                }
                val members = if (prefs == null) b.members else b.members.sortedBy { if (it.remarks in prefs.pinned) 0 else 1 }
                itemsIndexed(members, key = { _, p -> p.remarks }) { j, p ->
                    ServerTile(
                        p, pings[p.remarks], errors[p.remarks], p.remarks == selected, grid == ServerGrid.One, info,
                        Modifier.enter(j / grid.columns, since), waiting = waiting(p.remarks), onLongClick = { details = p }
                    ) {
                        chosen = p
                        close++
                    }
                }
            }
        }
    }
}
