@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.AutoSelect
import dev.yzewe.rynet.data.AutoState
import dev.yzewe.rynet.data.Candidate
import dev.yzewe.rynet.data.NetMode
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.WhitelistMode
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private fun minutesLeft(next: Long, now: Long): String {
    val m = ((next - now + 59_999) / 60_000).coerceAtLeast(0)
    return if (m == 0L) tr("сейчас", "now") else tr("через $m мин", "in $m min")
}

fun autoStatus(state: AutoState?, now: Long, live: Boolean): String? {
    val st = state ?: return null
    return when {
        st.running -> tr("подбираю сервер", "picking a server")
        st.at == 0L -> null
        st.mode == NetMode.Whitelist -> tr("белые списки", "whitelists") + (if (live && st.next > 0) tr(" · проверка ", " · check ") + minutesLeft(st.next, now) else "")
        st.mode == NetMode.Offline -> tr("напрямую ничего не открывается", "nothing opens directly")
        else -> tr("обычный интернет", "open internet") + (if (st.total > 0) " · " + plural(st.total, "кандидат", "кандидата", "кандидатов", "candidate") else "")
    }
}

@Composable
fun AutoCard(on: Boolean, servers: Int, status: String?, onPick: () -> Unit, modifier: Modifier = Modifier) {
    val c = MaterialTheme.colorScheme
    val bg by animateColorAsState(if (on) c.secondaryContainer else c.surfaceContainerHigh, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "autoCard")
    val content = if (on) c.onSecondaryContainer else c.onSurface
    val muted = if (on) content.copy(alpha = 0.72f) else c.onSurfaceVariant
    val src = remember { MutableInteractionSource() }
    Row(
        modifier.fillMaxWidth().pressSquish(src, 0.98f).clip(MaterialTheme.shapes.extraLarge).background(bg)
            .clickable(src, PressIndication, onClick = hapticOnClick(onClick = onPick)).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        ShapeIcon(Glyph.Auto, MaterialShapes.Cookie9Sided, if (on) c.primary else c.primaryContainer, if (on) c.onPrimary else c.onPrimaryContainer)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(tr("Автовыбор", "Auto select"), color = content, style = MaterialTheme.typography.titleLargeEmphasized, maxLines = 1)
            Text(
                status?.takeIf { on } ?: (tr("все подписки", "all subscriptions") + if (servers > 0) " · " + plural(servers, "сервер", "сервера", "серверов", "server") else ""),
                color = muted,
                style = MaterialTheme.typography.labelLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
        androidx.compose.animation.AnimatedVisibility(
            on,
            enter = fadeIn(MaterialTheme.motionScheme.fastEffectsSpec()) + scaleIn(MaterialTheme.motionScheme.fastSpatialSpec(), 0.6f),
            exit = fadeOut(MaterialTheme.motionScheme.fastEffectsSpec()) + scaleOut(MaterialTheme.motionScheme.fastSpatialSpec(), 0.6f)
        ) {
            Box(Modifier.size(28.dp).clip(CircleShape).background(c.primary), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.Check, null, tint = c.onPrimary, modifier = Modifier.size(18.dp))
            }
        }
    }
}

@Composable
fun AutoWhySheet(state: AutoState?, current: dev.yzewe.rynet.data.Selection?, onPick: (dev.yzewe.rynet.data.Selection) -> Unit, onCheck: () -> Unit, onSettings: () -> Unit, onDismiss: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val st = state ?: AutoState()
    val list = rememberLazyListState()
    AppSheet(
        title = tr("Почему этот сервер", "Why this server"),
        onDismiss = onDismiss,
        topDivider = list.canScrollBackward,
        bottomDivider = list.canScrollForward,
        animateHeight = true,
        footer = {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CircleIconButton(Glyph.Auto, tr("Настройки автовыбора", "Auto select settings"), onSettings, size = 56.dp, container = c.surfaceContainerHighest)
                PillButton(
                    if (st.running) tr("Проверяю", "Checking") else tr("Проверить сейчас", "Check now"),
                    onCheck,
                    Modifier.weight(1f),
                    enabled = !st.running,
                    accent = false,
                    container = c.surfaceContainerHighest,
                    contentColor = c.onSurface,
                    leading = {
                        if (st.running) {
                            LoadingIndicator(Modifier.size(20.dp), color = c.primary)
                        } else {
                            Icon(Icons.Rounded.Refresh, null, modifier = Modifier.size(20.dp))
                        }
                    }
                )
            }
        }
    ) {
        LazyColumn(state = list, contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
            item(key = "net") { NetCard(st) }
            if (st.candidates.isNotEmpty()) {
                item(key = "title") {
                    Header(tr("Кандидаты", "Candidates") + if (st.total > 0) tr(" · ${st.candidates.count { it.pool }} из ${st.total}", " · ${st.candidates.count { it.pool }} of ${st.total}") else "")
                }
                itemsIndexed(st.candidates, key = { _, it -> it.sub + "\u001f" + it.profile }) { i, x ->
                    CandidateRow(x, best = x.selection == (st.chosen ?: current), shape = Segments.shape(i, st.candidates.size)) { onPick(x.selection) }
                }
            }
        }
    }
}

@Composable
private fun NetCard(st: AutoState) {
    val c = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().clip(MaterialTheme.shapes.extraLarge).background(c.surfaceContainer).padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                tr("Сеть", "Network") + st.network.takeIf { it.isNotBlank() }?.let { " · $it" }.orEmpty(),
                style = MaterialTheme.typography.titleSmall,
                color = c.onSurface,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            if (st.at > 0) {
                val (label, bg, fg) = when (st.mode) {
                    NetMode.Whitelist -> Triple(tr("белые списки", "whitelists"), c.tertiaryContainer, c.onTertiaryContainer)
                    NetMode.Offline -> Triple(tr("нет доступа", "no access"), c.errorContainer, c.onErrorContainer)
                    NetMode.Free -> Triple(tr("обычный интернет", "open internet"), c.secondaryContainer, c.onSecondaryContainer)
                }
                Chip(label, container = bg, content = fg)
            }
        }
        if (st.probes.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(tr("Напрямую", "Directly"), style = MaterialTheme.typography.labelMedium, color = c.onSurfaceVariant)
            Spacer(Modifier.height(4.dp))
            st.probes.forEach { p ->
                Row(Modifier.fillMaxWidth().height(36.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (p.ok) Icons.Rounded.Check else Icons.Rounded.Close, null, tint = if (p.ok) c.primary else c.error, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(p.host, style = MaterialTheme.typography.bodyLarge, color = c.onSurface, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        if (p.ok) tr("${p.ms} мс", "${p.ms} ms") else tr("не открывается", "doesn't open"),
                        style = MaterialTheme.typography.labelLarge,
                        color = if (p.ok) c.onSurfaceVariant else c.error
                    )
                }
            }
        } else if (st.at == 0L) {
            Spacer(Modifier.height(6.dp))
            Text(tr("Ещё не проверял", "Not checked yet"), style = MaterialTheme.typography.bodyMedium, color = c.onSurfaceVariant)
        }
    }
}

@Composable
private fun CandidateRow(x: Candidate, best: Boolean, shape: androidx.compose.ui.graphics.Shape, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val off = !x.ok || !x.pool
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).clip(shape).background(if (best) c.secondaryContainer else c.surfaceContainer).clickable(onClick = onClick)
            .alpha(if (off) 0.6f else 1f).padding(start = 12.dp, end = 16.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        FlagIcon(x.flag, Modifier.size(width = 36.dp, height = 27.dp), MaterialTheme.shapes.small)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(x.name.ifBlank { x.profile }, style = MaterialTheme.typography.bodyLarge, color = if (best) c.onSecondaryContainer else c.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOf(
                    x.subTitle,
                    if (x.learned) tr("проходит при БС", "passes whitelists") else ""
                ).filter { it.isNotBlank() }.joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = c.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
        val badge = when {
            !x.ok -> tr("не проходит", "fails") to true
            else -> null
        }
        badge?.let { (t, bad) ->
            Spacer(Modifier.width(8.dp))
            Chip(t, container = if (bad) c.errorContainer else c.surfaceContainerHighest, content = if (bad) c.onErrorContainer else c.onSurfaceVariant)
        }
        if (x.ok) {
            Spacer(Modifier.width(10.dp))
            PingText(x.ms, MaterialTheme.typography.titleMediumEmphasized, short = true)
        }
    }
}

private val AUTO_INTERVALS = listOf(5, 15, 30, 0)
private val AUTO_MARGINS = listOf(30, 50, 100)

@Composable
internal fun AutoGroup(settings: Settings, onChange: (Settings) -> Unit) {
    val ctx = LocalContext.current
    var skipSheet by remember { mutableStateOf(false) }
    var wlSheet by remember { mutableStateOf(false) }
    var hosts by remember { mutableStateOf<Boolean?>(null) }
    val subs by produceState(emptyList<Sub>()) { value = withContext(Dispatchers.IO) { Subs.list(ctx) } }
    val transports by produceState(emptyList<String>()) {
        value = withContext(Dispatchers.IO) {
            val have = AutoSelect.servers(ctx).flatMap { it.second }.map(AutoSelect::transport).toSet()
            AutoSelect.TRANSPORTS.filter { it in have }
        }
    }
    val existing by produceState<Map<String, Set<String>>?>(null) { value = withContext(Dispatchers.IO) { AutoSelect.servers(ctx).associate { (sub, list) -> sub.id to list.map { it.remarks }.toSet() } } }
    val skipped = settings.autoSkip.entries.sumOf { (sub, list) -> existing?.let { e -> list.count { it in e[sub].orEmpty() } } ?: list.size }
    val on = settings.autoFeature
    Group(
        listOf(
            Item(Glyph.Auto, MaterialShapes.Cookie9Sided, tr("Умный автовыбор", "Smart auto select"), tr("Сам выбирает рабочий быстрый сервер", "Picks a working, fast server for you"), on) {
                onChange(settings.copy(autoFeature = !on, autoSelect = !on))
            }
        )
    )
    Header(tr("Где искать", "Where to look"))
    if (subs.size > 1) {
        Pills {
            subs.forEach { sub ->
                val use = sub.id !in settings.autoSkipSubs
                ToggleChip(sub.title, use, enabled = on, small = true) {
                    onChange(settings.copy(autoSkipSubs = if (use) settings.autoSkipSubs + sub.id else settings.autoSkipSubs - sub.id))
                }
            }
        }
    }
    Group(
        listOf(
            Item(Icons.Rounded.Close, MaterialShapes.Pill, tr("Не брать", "Never use"), if (skipped == 0) tr("Серверы, которые автовыбор пропускает", "Servers auto select skips") else plural(skipped, "сервер", "сервера", "серверов", "server"), enabled = on) { skipSheet = true },
            Item(Glyph.Pin, MaterialShapes.Cookie7Sided, tr("Закреплённые первыми", "Pinned first"), tr("Закреплённые серверы выбираются чаще", "Pinned servers are picked more often"), settings.autoPins, enabled = on) {
                onChange(settings.copy(autoPins = !settings.autoPins))
            }
        )
    )
    ChoiceBlock(
        tr("Белые списки", "Whitelists"),
        WhitelistMode.entries,
        settings.autoWhitelist,
        { it.title },
        {
            when (it) {
                WhitelistMode.Auto -> tr("Определять по тому, какие сайты открываются\u00A0напрямую", "Detect by which sites open directly")
                WhitelistMode.Always -> tr("Считать, что белые списки включены всегда", "Always assume whitelists are on")
                WhitelistMode.Never -> tr("Считать интернет всегда обычным", "Always assume open internet")
            }
        },
        enabled = on
    ) { onChange(settings.copy(autoWhitelist = it)) }
    Group(
        listOf(
            Item(
                Glyph.Flag,
                MaterialShapes.Cookie7Sided,
                tr("Для белых списков", "For whitelists"),
                listOfNotNull(tr("Проверяются первыми", "Tried first"), wlSummary(settings)).joinToString("\n"),
                enabled = on
            ) { wlSheet = true }
        ) + if (settings.autoWhitelist == WhitelistMode.Auto) {
            listOf(
                Item(
                    Glyph.Home,
                    MaterialShapes.Clover4Leaf,
                    tr("Свои сайты", "Home sites"),
                    tr("Открыты и при белых списках", "Open even under whitelists") + "\n" + AutoSelect.hosts(settings.autoHome).joinToString(", "),
                    enabled = on
                ) { hosts = true },
                Item(
                    Glyph.Globe,
                    MaterialShapes.Cookie6Sided,
                    tr("Зарубежные сайты", "Foreign sites"),
                    tr("Закрыты при белых списках", "Blocked under whitelists") + "\n" + AutoSelect.hosts(settings.autoFree).joinToString(", "),
                    enabled = on
                ) { hosts = false }
            )
        } else {
            emptyList()
        }
    )
    ChoiceBlock(
        tr("Проверять", "Check"),
        AUTO_INTERVALS,
        settings.autoInterval.takeIf { it in AUTO_INTERVALS } ?: 15,
        { if (it == 0) tr("При сбое", "On failure") else tr("$it мин", "$it min") },
        enabled = on
    ) { onChange(settings.copy(autoInterval = it)) }
    ChoiceBlock(
        tr("Менять, если быстрее на", "Switch if faster by"),
        AUTO_MARGINS,
        settings.autoMargin.takeIf { it in AUTO_MARGINS } ?: 50,
        { tr("$it мс", "$it ms") },
        enabled = on
    ) { onChange(settings.copy(autoMargin = it)) }
    Group(
        listOf(
            Item(Glyph.Speed, MaterialShapes.Gem, tr("Учитывать скорость", "Count speed"), tr("При равном пинге берёт более быстрый", "With equal ping, takes the faster one"), settings.autoSpeed, enabled = on) { onChange(settings.copy(autoSpeed = !settings.autoSpeed)) },
            Item(Glyph.Data, MaterialShapes.Burst, tr("Беречь мобильный трафик", "Save mobile data"), tr("В мобильной сети проверок меньше", "Fewer checks on mobile data"), settings.autoSaveData, enabled = on) { onChange(settings.copy(autoSaveData = !settings.autoSaveData)) },
            Item(Icons.Rounded.Notifications, MaterialShapes.Cookie7Sided, tr("Сообщать о смене", "Notify on switch"), tr("Уведомление, когда автовыбор переключил сервер", "A notification when auto select switches the server"), settings.autoNotify, enabled = on) { onChange(settings.copy(autoNotify = !settings.autoNotify)) }
        )
    )
    val choice = transports + settings.autoTransports.filter { it !in transports }
    if (choice.size > 1 || settings.autoTransports.isNotEmpty()) {
        Header(tr("Предпочитать транспорт", "Prefer transport"))
        Pills {
            choice.forEach { t ->
                val picked = t in settings.autoTransports
                ToggleChip(t, picked, enabled = on, small = true) {
                    onChange(settings.copy(autoTransports = if (picked) settings.autoTransports - t else settings.autoTransports + t))
                }
            }
        }
    }
    if (skipSheet) AutoSkipSheet(settings, onChange) { skipSheet = false }
    if (wlSheet) AutoWlSheet(settings, onChange) { wlSheet = false }
    hosts?.let { home ->
        HostsDialog(
            if (home) tr("Свои сайты", "Home sites") else tr("Зарубежные сайты", "Foreign sites"),
            if (home) settings.autoHome else settings.autoFree,
            if (home) Settings.AUTO_HOME else Settings.AUTO_FREE,
            onDismiss = { hosts = null }
        ) { text ->
            onChange(if (home) settings.copy(autoHome = text) else settings.copy(autoFree = text))
            hosts = null
        }
    }
}

@Composable
private fun Pills(content: @Composable () -> Unit) {
    FlowRow(
        Modifier.fillMaxWidth().padding(bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp)
    ) { content() }
}

@Composable
internal fun ToggleChip(text: String, on: Boolean, enabled: Boolean = true, small: Boolean = false, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val bg by animateColorAsState(
        if (on) {
            c.secondaryContainer
        } else if (small) {
            c.surfaceContainerHigh
        } else {
            c.surfaceContainerHighest
        },
        MaterialTheme.motionScheme.fastEffectsSpec(),
        label = "chip"
    )
    val alpha by animateFloatAsState(if (enabled) 1f else 0.38f, Springs.effects(), label = "chipAlpha")
    val src = remember { MutableInteractionSource() }
    Row(
        Modifier.reflow().height(if (small) 32.dp else 40.dp).alpha(alpha).pressSquish(src, 0.95f).clip(if (small) RoundedCornerShape(12.dp) else MaterialTheme.shapes.medium).background(bg)
            .clickable(src, PressIndication, enabled = enabled, onClick = hapticOnClick(onClick = onClick)).padding(horizontal = if (small) 12.dp else 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        AnimatedVisibility(
            on,
            enter = expandHorizontally(Springs.spatial()) + scaleIn(Springs.bouncy(), 0.4f) + fadeIn(Springs.effects()),
            exit = shrinkHorizontally(Springs.spatial()) + scaleOut(targetScale = 0.4f) + fadeOut(Springs.effects())
        ) { Icon(Icons.Rounded.Check, null, tint = c.onSecondaryContainer, modifier = Modifier.padding(end = 6.dp).size(18.dp)) }
        Text(text, style = MaterialTheme.typography.labelLarge, color = if (on) c.onSecondaryContainer else c.onSurfaceVariant, maxLines = 1)
    }
}

@Composable
private fun AutoSkipSheet(settings: Settings, onChange: (Settings) -> Unit, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    val c = MaterialTheme.colorScheme
    val groups by produceState<List<Pair<Sub, List<Profile>>>?>(null) { value = withContext(Dispatchers.IO) { AutoSelect.servers(ctx) } }
    val passed by produceState(emptySet<String>()) { value = withContext(Dispatchers.IO) { AutoSelect.passed(ctx) } }
    var current by remember { mutableStateOf(settings) }
    LaunchedEffect(groups) {
        groups?.let { g ->
            val spread = AutoSelect.withMembers(current, g)
            val names = g.associate { (sub, list) -> sub.id to list.flatMap { p -> listOf(p.remarks) + p.members.map { it.remarks } }.toSet() }
            val pruned = spread.copy(autoSkip = spread.autoSkip.mapNotNull { (sub, list) -> names[sub]?.let { n -> list.filter { it in n }.takeIf { it.isNotEmpty() }?.let { sub to it } } }.toMap())
            if (pruned != current) {
                current = pruned
                onChange(pruned)
            }
        }
    }
    val list = rememberLazyListState()
    var close by remember { mutableLongStateOf(0L) }
    AppSheet(
        title = tr("Не брать", "Never use"),
        onDismiss = onDismiss,
        topDivider = list.canScrollBackward,
        dismissRequestId = close,
        fillMaxHeight = true
    ) {
        val g = groups
        if (g == null) {
            Box(Modifier.fillMaxWidth().height(160.dp), contentAlignment = Alignment.Center) { LoadingIndicator() }
            return@AppSheet
        }
        LazyColumn(state = list, contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
            g.forEach { (sub, servers) ->
                val all = servers.isNotEmpty() && servers.all { it.remarks in current.autoSkipped(sub.id) }
                item(key = "h" + sub.id) {
                    Row(
                        Modifier.fillMaxWidth().padding(top = 8.dp).clip(MaterialTheme.shapes.large)
                            .clickable {
                                current = current.autoSkipAll(sub.id, servers.map { it.remarks }, !all)
                                onChange(current)
                            }
                            .padding(end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            sub.title,
                            style = PageType.header,
                            color = c.primary,
                            modifier = Modifier.weight(1f),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Checkbox(checked = all, onCheckedChange = null, modifier = Modifier.padding(horizontal = 12.dp))
                    }
                }
                itemsIndexed(servers, key = { _, p -> sub.id + "\u001f" + p.remarks }) { i, p ->
                    val skip = p.remarks in current.autoSkipped(sub.id)
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(Segments.shape(i, servers.size)).background(c.surfaceContainerHigh)
                            .clickable {
                                current = current.autoSkip(sub.id, p.remarks, !skip)
                                onChange(current)
                            }
                            .padding(start = 12.dp, end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        FlagIcon(p.flag, Modifier.size(width = 36.dp, height = 27.dp), MaterialTheme.shapes.small)
                        Spacer(Modifier.width(12.dp))
                        Text(p.name, style = MaterialTheme.typography.bodyLarge, color = c.onSurface, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (AutoSelect.sig(p) in passed) Chip(tr("проходит при БС", "passes whitelists"), container = c.tertiaryContainer, content = c.onTertiaryContainer)
                        Checkbox(checked = skip, onCheckedChange = null, modifier = Modifier.padding(horizontal = 12.dp))
                    }
                }
            }
        }
    }
}

private fun wlSummary(s: Settings): String? {
    val subs = s.autoWlSubs.size
    val servers = s.autoWl.values.sumOf { it.size }
    return listOfNotNull(
        subs.takeIf { it > 0 }?.let { plural(it, "подписка", "подписки", "подписок", "subscription") },
        servers.takeIf { it > 0 }?.let { plural(it, "сервер", "сервера", "серверов", "server") }
    ).joinToString(", ").ifEmpty { null }
}

@Composable
private fun AutoWlSheet(settings: Settings, onChange: (Settings) -> Unit, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    val c = MaterialTheme.colorScheme
    val groups by produceState<List<Pair<Sub, List<Profile>>>?>(null) { value = withContext(Dispatchers.IO) { AutoSelect.servers(ctx) } }
    var current by remember { mutableStateOf(settings) }
    val list = rememberLazyListState()
    fun set(s: Settings) {
        current = s
        onChange(s)
    }
    LaunchedEffect(groups) { groups?.let { g -> AutoSelect.withMembers(current, g).takeIf { it != current }?.let(::set) } }
    AppSheet(
        title = tr("Для белых списков", "For whitelists"),
        onDismiss = onDismiss,
        topDivider = list.canScrollBackward,
        fillMaxHeight = true
    ) {
        val g = groups
        if (g == null) {
            Box(Modifier.fillMaxWidth().height(160.dp), contentAlignment = Alignment.Center) { LoadingIndicator() }
            return@AppSheet
        }
        LazyColumn(state = list, contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
            item(key = "about") {
                Text(
                    tr(
                        "Когда сеть пускает только по белым спискам, автовыбор сначала проверяет отмеченные серверы. Галочка у подписки отмечает её целиком.",
                        "When the network only lets whitelisted traffic through, auto select tries the marked servers first. The tick by a subscription marks all of it."
                    ),
                    style = PageType.rowSummary,
                    color = c.onSurfaceVariant,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 4.dp)
                )
            }
            g.forEach { (sub, servers) ->
                val whole = sub.id in current.autoWlSubs
                item(key = "h" + sub.id) {
                    Row(
                        Modifier.fillMaxWidth().padding(top = 8.dp).clip(MaterialTheme.shapes.large)
                            .clickable { set(current.autoWlSub(sub.id, !whole)) }
                            .padding(end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            sub.title,
                            style = PageType.header,
                            color = c.primary,
                            modifier = Modifier.weight(1f),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Checkbox(checked = whole, onCheckedChange = null, modifier = Modifier.padding(horizontal = 12.dp))
                    }
                }
                itemsIndexed(servers, key = { _, p -> sub.id + "\u001f" + p.remarks }) { i, p ->
                    val on = current.isAutoWl(sub.id, p.remarks)
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(Segments.shape(i, servers.size)).background(c.surfaceContainerHigh)
                            .clickable {
                                set(
                                    if (whole) {
                                        servers.filter { it.remarks != p.remarks }
                                            .fold(current.autoWlSub(sub.id, false)) { acc, o -> acc.autoWlServer(sub.id, o.remarks, true) }
                                    } else {
                                        current.autoWlServer(sub.id, p.remarks, !on)
                                    }
                                )
                            }
                            .padding(start = 12.dp, end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        FlagIcon(p.flag, Modifier.size(width = 36.dp, height = 27.dp), MaterialTheme.shapes.small)
                        Spacer(Modifier.width(12.dp))
                        Text(p.name, style = MaterialTheme.typography.bodyLarge, color = c.onSurface, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Checkbox(checked = on, onCheckedChange = null, modifier = Modifier.padding(horizontal = 12.dp))
                    }
                }
            }
        }
    }
}

@Composable
internal fun HostsDialog(title: String, current: String, default: String, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf(AutoSelect.hosts(current).joinToString("\n")) }
    val valid = AutoSelect.hosts(text).let { h -> h.isNotEmpty() && h.all { Regex("""^(https?://)?[\w.-]+\.[a-z]{2,}(/\S*)?$""", RegexOption.IGNORE_CASE).matches(it) } }
    AppSheet(
        title,
        onDismiss,
        imePadding = true,
        footer = {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton(tr("По умолчанию", "Default"), { text = AutoSelect.hosts(default).joinToString("\n") }, Modifier.weight(1f), accent = false)
                PillButton(tr("Сохранить", "Save"), { onSave(AutoSelect.hosts(text).joinToString(", ")) }, Modifier.weight(1f), enabled = valid)
            }
        }
    ) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 8.dp)) {
            TextField(
                value = text,
                onValueChange = { text = it },
                minLines = 3,
                isError = !valid,
                textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                shape = MaterialTheme.shapes.large,
                colors = TextFieldDefaults.colors(focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth().remoteField(MaterialTheme.shapes.large)
            )
        }
    }
}
