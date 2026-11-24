@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.GeoLists
import dev.yzewe.rynet.data.ModeEntry
import dev.yzewe.rynet.data.ModeRules
import dev.yzewe.rynet.data.RuleAction
import dev.yzewe.rynet.data.SubscriptionRules
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private fun entries(n: Int) = plural(n, "запись", "записи", "записей", "entry", "entries")

@Composable
internal fun ModeSetRow(title: String, summary: String, index: Int, count: Int, active: Boolean, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val src = remember { MutableInteractionSource() }
    SettingsRow(
        shape = Segments.shape(index, count),
        onClick = onClick,
        leading = { RowIcon(Glyph.Grid, src) },
        interactionSource = src,
        title = title,
        summary = summary,
        modifier = Modifier.padding(bottom = if (index < count - 1) Segments.Gap else 0.dp),
        faded = !active
    ) { Icon(Glyph.Pin, tr("Закреплено", "Pinned"), tint = c.onSurfaceVariant) }
}

@Composable
internal fun SubscriptionSetRow(rules: SubscriptionView, active: Boolean, index: Int = 0, count: Int = 1, onClick: () -> Unit) = ModeSetRow(
    rules.name.ifBlank { tr("Правила подписки", "Subscription rules") },
    if (rules.entries.isEmpty()) tr("В подписке нет своих правил", "The subscription has no rules of its own") else tr("Режим Подписка · ", "Subscription mode · ") + entries(rules.entries.size),
    index,
    count,
    active,
    onClick
)

@Composable
internal fun SubscriptionRulesSheet(rules: SubscriptionRules, onDismiss: () -> Unit) {
    ModeRulesSheet(rules.name.ifBlank { tr("Правила подписки", "Subscription rules") }, ModeRules.subscription(rules), rules.rest, onDismiss)
}

@Composable
internal fun ModeRulesSheet(title: String, rules: List<ModeEntry>, rest: RuleAction?, onDismiss: () -> Unit, privateNets: List<String> = emptyList()) {
    val uri = androidx.compose.ui.platform.LocalUriHandler.current
    AppSheet(title, onDismiss) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                tr("Встроенный набор, изменить нельзя", "Built-in set, can't be changed"),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            listOf(RuleAction.Block, RuleAction.Proxy, RuleAction.Direct).forEach { action ->
                val own = rules.filter { it.action == action }
                if (own.isEmpty()) return@forEach
                Header(action.title)
                SECTIONS.forEach { (name, kinds) ->
                    val items = own.filter { it.kind in kinds }
                    if (items.isNotEmpty()) {
                        Text(
                            "${name()} · ${items.size}",
                            style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(start = 4.dp, top = 4.dp)
                        )
                        var expanded by remember(action, name) { mutableStateOf(false) }
                        val shown = if (expanded) items else items.take(30)
                        val more = !expanded && items.size > 30
                        val n = shown.size + if (more) 1 else 0
                        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Segments.Outer)), verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
                            shown.forEachIndexed { i, e ->
                                EntryRow(e, Segments.shape(i, n), privateNets.takeIf { e.kind == ModeEntry.Kind.GeoIp && e.value == "private" }) {
                                    runCatching { uri.openUri(geoLink(if (e.kind == ModeEntry.Kind.GeoIp) "geoip" else "geosite", e.value)) }
                                }
                            }
                            if (more) {
                                Box(
                                    Modifier.fillMaxWidth().clip(Segments.shape(n - 1, n)).background(MaterialTheme.colorScheme.surfaceContainerHigh)
                                        .clickable { expanded = true }.padding(horizontal = 16.dp, vertical = 14.dp)
                                ) {
                                    Text(tr("Показать все (${items.size})", "Show all (${items.size})"), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                                }
                            }
                        }
                    }
                }
            }
            if (rest != null) {
                Header(tr("Остальное", "Everything else"))
                ActionTag(rest)
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

private val SECTIONS: List<Pair<() -> String, Set<ModeEntry.Kind>>> = listOf(
    { tr("Сайты", "Sites") } to setOf(ModeEntry.Kind.Suffix, ModeEntry.Kind.Full, ModeEntry.Kind.Keyword, ModeEntry.Kind.Regex),
    { tr("Категории сайтов", "Site categories") } to setOf(ModeEntry.Kind.GeoSite),
    { tr("Сети", "Networks") } to setOf(ModeEntry.Kind.Cidr, ModeEntry.Kind.GeoIp),
    { tr("Приложения", "Apps") } to setOf(ModeEntry.Kind.App),
    { tr("Протоколы", "Protocols") } to setOf(ModeEntry.Kind.Protocol)
)

@Composable
private fun EntryRow(e: ModeEntry, shape: androidx.compose.ui.graphics.Shape, note: List<String>?, onOpen: () -> Unit) {
    val ctx = LocalContext.current
    val c = MaterialTheme.colorScheme
    val kind = if (e.kind == ModeEntry.Kind.GeoIp) "geoip" else "geosite"
    val count by produceState<Int?>(null, kind, e.value, e.geo) {
        if (!e.geo) return@produceState
        value = withContext(Dispatchers.IO) { GeoLists.file(ctx, kind, e.value)?.let { f -> runCatching { f.useLines { it.count { l -> l.isNotBlank() && !l.startsWith("#") } } }.getOrNull() } ?: -1 }
    }
    Row(
        Modifier.fillMaxWidth().clip(shape).background(c.surfaceContainerHigh)
            .then(if (e.geo) Modifier.clickable(onClick = onOpen) else Modifier)
            .heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Column(Modifier.weight(1f)) {
            Text(e.text, style = MaterialTheme.typography.bodyLarge, color = c.onSurface)
            note?.let { Text(it.joinToString(", "), style = MaterialTheme.typography.bodySmall, color = c.onSurfaceVariant) }
        }
        if (e.geo) {
            Text(
                when (count) {
                    null -> ""
                    -1 -> tr("не загружен", "not loaded")
                    else -> entries(count!!)
                },
                style = MaterialTheme.typography.labelLarge,
                color = c.onSurfaceVariant,
                modifier = Modifier.padding(start = 12.dp)
            )
        }
    }
}
