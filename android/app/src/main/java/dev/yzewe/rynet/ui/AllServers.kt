package dev.yzewe.rynet.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.ServerGrid
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.tr

class AllEntry(val sub: String, val p: Profile, val ping: Int?, val error: String?, val waiting: Boolean) {
    val key: String get() = sub + "\u001f" + p.remarks
    val selection: Selection get() = Selection(sub, p.remarks)
}

@Composable
fun AllServersContent(
    entries: List<AllEntry>,
    pinned: Set<String>,
    selected: Selection?,
    autoFeature: Boolean,
    autoActive: Boolean,
    autoPick: AllEntry?,
    sort: ServerSort,
    grid: ServerGrid,
    info: ServerInfo,
    query: String,
    onAuto: () -> Unit,
    columns: Int? = null,
    navInset: Boolean = true,
    onLongClick: (AllEntry) -> Unit,
    onSelect: (AllEntry) -> Unit
) {
    val c = MaterialTheme.colorScheme
    val q = query.trim()
    val shown = remember(entries, q, sort) {
        val words = queryWords(q)
        val found = if (words.isEmpty()) entries else entries.filter { it.p.matches(words) }
        when (sort) {
            ServerSort.Ping -> found.sortedBy { it.ping?.takeIf { v -> v >= 0 } ?: Int.MAX_VALUE }
            ServerSort.Name -> found.sortedBy { it.p.name.lowercase() }
            else -> found
        }
    }
    val top = if (q.isEmpty()) shown.filter { it.key in pinned } else emptyList()
    val rest = shown.filter { it !in top }
    val cols = columns ?: grid.columns
    val nav = if (navInset) navBottom() else 0.dp
    val placement = Springs.spatial<IntOffset>()
    val since = rememberEnterStart()

    @Composable
    fun header(text: String) = Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = c.primary,
        modifier = Modifier.padding(start = 4.dp, top = 8.dp, bottom = 2.dp)
    )
    LazyVerticalGrid(
        columns = GridCells.Fixed(cols),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = nav + 24.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxSize()
    ) {
        if (autoFeature && q.isEmpty()) {
            item(key = "auto", span = { GridItemSpan(maxLineSpan) }) {
                AutoTile(autoActive, autoPick?.takeIf { autoActive }, onAuto)
            }
        }
        if (top.isNotEmpty()) {
            item(key = "pin-h", span = { GridItemSpan(maxLineSpan) }) { header(tr("Закреплённые · ${top.size}", "Pinned · ${top.size}")) }
            itemsIndexed(top, key = { _, e -> "pin:" + e.key }) { j, e ->
                ServerTile(e.p, e.ping, e.error, e.selection == selected, cols == 1, info, Modifier.animateItem(placementSpec = placement).enter(j / cols, since), waiting = e.waiting, onLongClick = { onLongClick(e) }) { onSelect(e) }
            }
        }
        if (top.isNotEmpty() && rest.isNotEmpty() && q.isEmpty()) {
            item(key = "all-h", span = { GridItemSpan(maxLineSpan) }) { header(tr("Все серверы · ${rest.size}", "All servers · ${rest.size}")) }
        }
        val from = (top.size + cols - 1) / cols
        itemsIndexed(rest, key = { _, e -> e.key }) { j, e ->
            ServerTile(
                e.p, e.ping, e.error, e.selection == selected, cols == 1, info,
                Modifier.animateItem(placementSpec = placement).then(if (q.isEmpty()) Modifier.enter(from + j / cols, since) else Modifier),
                waiting = e.waiting, onLongClick = { onLongClick(e) }
            ) { onSelect(e) }
        }
        if (q.isNotEmpty() && shown.isEmpty()) {
            item(key = "none", span = { GridItemSpan(maxLineSpan) }) {
                Text(
                    tr("Ничего не найдено", "Nothing found"),
                    color = c.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(vertical = 32.dp)
                )
            }
        }
    }
}

@Composable
private fun AutoTile(active: Boolean, pick: AllEntry?, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val view = androidx.compose.ui.platform.LocalView.current
    val src = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val bg by androidx.compose.animation.animateColorAsState(if (active) c.secondaryContainer else c.surfaceContainerHigh, Springs.effects(), label = "autoTile")
    val fg = if (active) c.onSecondaryContainer else c.onSurface
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).pressSquish(src, 0.97f)
            .clip(MaterialTheme.shapes.largeIncreased).background(bg)
            .clickable(src, PressGlow) {
                view.haptic(Haptic.Tap)
                onClick()
            }
            .padding(start = 8.dp, end = 16.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically
    ) {
        androidx.compose.foundation.layout.Box(
            Modifier.size(width = 58.67.dp, height = 44.dp).clip(MaterialTheme.shapes.medium).background(if (active) c.primary else c.primaryContainer),
            contentAlignment = androidx.compose.ui.Alignment.Center
        ) { androidx.compose.material3.Icon(Glyph.Auto, null, tint = if (active) c.onPrimary else c.onPrimaryContainer, modifier = Modifier.size(24.dp)) }
        androidx.compose.foundation.layout.Spacer(Modifier.width(14.dp))
        androidx.compose.foundation.layout.Column(Modifier.weight(1f)) {
            Text(tr("Автовыбор", "Auto select"), style = MaterialTheme.typography.titleMediumEmphasized, color = fg)
            pick?.let { e ->
                Text(
                    listOfNotNull(e.p.flag.takeIf { it.isNotBlank() }, e.p.name).joinToString(" "),
                    style = MaterialTheme.typography.labelLarge,
                    color = if (active) fg.copy(alpha = 0.72f) else c.onSurfaceVariant,
                    maxLines = 1,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis
                )
            }
        }
        pick?.ping?.takeIf { it >= 0 }?.let { PingText(it, MaterialTheme.typography.labelLargeEmphasized, short = true) }
    }
}
