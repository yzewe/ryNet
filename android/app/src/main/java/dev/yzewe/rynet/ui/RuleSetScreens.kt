@file:OptIn(ExperimentalMaterial3ExpressiveApi::class, ExperimentalLayoutApi::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import androidx.graphics.shapes.RoundedPolygon
import androidx.lifecycle.repeatOnLifecycle
import dev.yzewe.rynet.data.Endpoints
import dev.yzewe.rynet.data.GeoLists
import dev.yzewe.rynet.data.ModeRules
import dev.yzewe.rynet.data.RuleAction
import dev.yzewe.rynet.data.RuleCover
import dev.yzewe.rynet.data.RuleKind
import dev.yzewe.rynet.data.RuleSet
import dev.yzewe.rynet.data.RuleSets
import dev.yzewe.rynet.data.Rules
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.SubscriptionRules
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

internal val SET_SHAPES: List<RoundedPolygon>
    get() = listOf(MaterialShapes.Cookie9Sided, MaterialShapes.Sunny, MaterialShapes.Clover4Leaf, MaterialShapes.Pill, MaterialShapes.Gem, MaterialShapes.Cookie4Sided)

internal val SET_ICONS: List<ImageVector>
    get() = listOf(
        Glyph.Grid, Glyph.Globe, Glyph.Lock, Glyph.Bolt, Glyph.Smartphone, Glyph.Megaphone,
        Glyph.Home, Glyph.Work, Glyph.School, Glyph.Bank, Glyph.Cart, Glyph.Mail,
        Glyph.Chat, Glyph.Movie, Glyph.Music, Glyph.Tv, Glyph.Game, Glyph.Code,
        Glyph.Cloud, Glyph.Shield, Glyph.Block, Glyph.Flight, Glyph.Star
    )

internal fun setShape(s: RuleSet) = SET_SHAPES.let { it[s.icon.mod(it.size)] }

internal fun setIcon(i: Int) = SET_ICONS.let { it[i.mod(it.size)] }

internal fun setName(s: RuleSet) = s.name.ifBlank { tr("Набор", "Set") }

private fun entries(n: Int) = plural(n, "запись", "записи", "записей", "entry", "entries")

@Composable
internal fun SetRow(set: RuleSet, index: Int, count: Int, modifier: Modifier, onClick: () -> Unit) {
    val src = remember { MutableInteractionSource() }
    SettingsRow(
        shape = Segments.animated(index, count),
        onClick = onClick,
        leading = { RowIcon(setIcon(set.icon), src, set.enabled) },
        interactionSource = src,
        title = setName(set),
        summary = entries(set.size),
        modifier = modifier.padding(bottom = Segments.Gap).semantics { role = Role.Switch },
        below = { ActionTag(set.action, set.server?.profile) }
    ) {
        Switch(checked = set.enabled, onCheckedChange = null)
    }
}

@Composable
internal fun TailscaleSetSheet(onDismiss: () -> Unit, onSelect: (Selection) -> Unit) {
    AppSheet("Tailscale", onDismiss) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 20.dp)) {
            ServerPicker(null, onSelect)
        }
    }
}

@Composable
fun RuleSetScreen(id: String?, settings: Settings, onChange: (Settings) -> Unit, onBack: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val setId = rememberSaveable { id ?: RuleSets.newId() }
    var draft by remember { mutableStateOf(RuleSet(setId)) }
    val saved = settings.sets.firstOrNull { it.id == setId }
    val set = saved ?: draft
    val apps = rememberApps()
    val labels = remember(apps) { apps.associate { it.pkg to it.label } }
    var adding by remember { mutableStateOf<String?>(null) }
    var sharing by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    fun update(s: RuleSet) {
        val at = settings.sets.indexOfFirst { it.id == s.id }
        when {
            at >= 0 -> onChange(settings.copy(sets = settings.sets.toMutableList().also { it[at] = s }))
            s.size > 0 || s.name.isNotBlank() -> onChange(settings.copy(sets = settings.sets + s))
            else -> draft = s
        }
    }
    PageScaffold(
        set.name.ifBlank { tr("Новый набор", "New set") },
        onBack = onBack,
        actions = {
            if (set.size > 0) CircleIconButton(Glyph.Upload, tr("Поделиться", "Share"), { sharing = true })
            if (saved != null) CircleIconButton(Icons.Rounded.Delete, tr("Удалить", "Delete"), { deleting = true })
        }
    ) {
        item(key = "look") { LookCard(set) { update(it) } }
        item(key = "where") {
            Column(Modifier.padding(top = Segments.Gap)) {
                Group(
                    top = {
                        GroupCard {
                            Column(Modifier.padding(16.dp)) {
                                Text(tr("Куда", "Where"), style = MaterialTheme.typography.titleMedium)
                                Spacer(Modifier.height(12.dp))
                                Choice(RuleAction.entries, set.action, { it.title }, { a -> update(set.copy(action = a, server = set.server.takeIf { a == RuleAction.Server })) })
                                AnimatedVisibility(set.action == RuleAction.Server, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                                    ServerPicker(set.server) { update(set.copy(server = it)) }
                                }
                                AnimatedVisibility(set.apps.isNotEmpty() && set.action != RuleAction.Direct && Build.VERSION.SDK_INT < 29) {
                                    Text(
                                        tr("На этой версии Android для приложений работает только «Напрямую»", "On this Android version, apps can only go \"Direct\""),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = c.error,
                                        modifier = Modifier.padding(top = 8.dp, start = 4.dp)
                                    )
                                }
                            }
                        }
                    },
                    items = emptyList()
                )
            }
        }
        item(key = "domains") {
            Entries(tr("Сайты", "Sites"), set.domains, Glyph.Globe, tr("Сайт", "Site"), { adding = "domains" }) { v -> update(set.copy(domains = set.domains - v)) }
        }
        item(key = "apps") {
            Header(listOf(tr("Приложения", "Apps"), set.apps.size.takeIf { it > 0 }?.toString()).filterNotNull().joinToString(" · "))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                set.apps.forEach { pkg -> AppEntry(pkg, labels[pkg], installed = pkg in labels || apps.isEmpty()) { update(set.copy(apps = set.apps - pkg)) } }
                AddEntry(tr("Приложение", "App")) { adding = "apps" }
            }
        }
        item(key = "geosite") {
            val uri = androidx.compose.ui.platform.LocalUriHandler.current
            Entries(tr("Категории", "Categories"), set.geosite, Glyph.List, tr("Категория", "Category"), { adding = "geosite" }, onOpen = { v -> runCatching { uri.openUri(geoLink("geosite", v)) } }) { v -> update(set.copy(geosite = set.geosite - v)) }
        }
        item(key = "ips") {
            Entries(tr("Сети", "Networks"), set.ips, Glyph.Route, tr("Сеть", "Network"), { adding = "ips" }) { v -> update(set.copy(ips = set.ips - v)) }
        }
    }
    when (adding) {
        "domains" -> EntriesDialog(tr("Сайты", "Sites"), "youtube.com\ndiscord.com", { Rules.domain(it) }, onDismiss = { adding = null }, settings = settings, action = set.action, kind = RuleKind.Domain) {
            update(set.copy(domains = (set.domains + it).distinct()))
        }

        "geosite" -> EntriesDialog(tr("Категории geosite", "Geosite categories"), "github\ncategory-ads-all", RuleSets::geosite, onDismiss = { adding = null }) {
            update(set.copy(geosite = (set.geosite + it).distinct()))
        }

        "ips" -> EntriesDialog(tr("Сети", "Networks"), "10.0.0.0/8\n2001:db8::/32", { Rules.cidr(it)?.first }, onDismiss = { adding = null }, settings = settings, action = set.action, kind = RuleKind.Ip) {
            update(set.copy(ips = (set.ips + it).distinct()))
        }

        "apps" -> AppsSheet(apps, set.apps, onDismiss = { adding = null }) { update(set.copy(apps = it)) }
    }
    if (sharing) ShareSetSheet(set) { sharing = false }
    if (deleting) {
        DeleteDialog(
            tr("Удалить набор?", "Delete the set?"),
            setName(set),
            tr("Удалить", "Delete"),
            onConfirm = {
                onChange(settings.copy(sets = settings.sets.filter { it.id != setId }))
                onBack()
            },
            onDismiss = { deleting = false }
        )
    }
}

@Composable
private fun LookCard(set: RuleSet, onChange: (RuleSet) -> Unit) {
    val c = MaterialTheme.colorScheme
    var name by rememberSaveable(set.id) { mutableStateOf(set.name) }
    Box(Modifier.clip(RoundedCornerShape(Segments.Outer))) {
        GroupCard {
            Column(Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    ShapeIcon(setIcon(set.icon), setShape(set), c.primaryContainer, c.onPrimaryContainer, size = 64.dp)
                    Spacer(Modifier.width(16.dp))
                    TextField(
                        name,
                        {
                            name = it.take(40)
                            onChange(set.copy(name = name.trim()))
                        },
                        singleLine = true,
                        label = { Text(tr("Название", "Name")) },
                        shape = MaterialTheme.shapes.large,
                        colors = TextFieldDefaults.colors(
                            focusedIndicatorColor = Color.Transparent,
                            unfocusedIndicatorColor = Color.Transparent,
                            focusedContainerColor = c.surfaceContainerHighest,
                            unfocusedContainerColor = c.surfaceContainerHighest
                        ),
                        modifier = Modifier.weight(1f).remoteField(MaterialTheme.shapes.large)
                    )
                }
                Spacer(Modifier.height(16.dp))
                Picker(SET_ICONS.size, set.icon.mod(SET_ICONS.size), { onChange(set.copy(icon = it)) }) { i, on ->
                    val bg by animateColorAsState(if (on) c.primaryContainer else Color.Transparent, label = "icon-bg")
                    val fg by animateColorAsState(if (on) c.onPrimaryContainer else c.onSurfaceVariant, label = "icon-fg")
                    ShapeIcon(SET_ICONS[i], if (on) SET_SHAPES[i.mod(SET_SHAPES.size)] else MaterialShapes.Circle, bg, fg)
                }
            }
        }
    }
}

@Composable
private fun Picker(n: Int, selected: Int, onPick: (Int) -> Unit, item: @Composable (Int, Boolean) -> Unit) {
    val view = LocalView.current
    val c = MaterialTheme.colorScheme
    var open by rememberSaveable { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open) 180f else 0f, Springs.spatial(), label = "pickerTurn")
    BoxWithConstraints {
        val side = minOf(48.dp, maxWidth / 6)

        @Composable
        fun cell(i: Int) = Box(
            Modifier.size(side).clip(CircleShape).clickable {
                view.haptic(Haptic.Segment)
                onPick(i)
            },
            contentAlignment = Alignment.Center
        ) { item(i, i == selected) }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                (0 until minOf(5, n)).forEach { cell(it) }
                Box(
                    Modifier.size(side).clip(CircleShape).clickable {
                        view.haptic(Haptic.Tap)
                        open = !open
                    },
                    contentAlignment = Alignment.Center
                ) {
                    Icon(
                        Glyph.ExpandMore,
                        contentDescription = tr(if (open) "Свернуть" else "Ещё", if (open) "Less" else "More"),
                        tint = c.onSurfaceVariant,
                        modifier = Modifier.graphicsLayer { rotationZ = turn }
                    )
                }
            }
            AnimatedVisibility(
                open,
                enter = fadeIn(tween(180)) + expandVertically(tween(220)),
                exit = fadeOut(tween(140)) + shrinkVertically(tween(180))
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    (5 until n).chunked(6).forEach { row ->
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { row.forEach { cell(it) } }
                    }
                }
            }
        }
    }
}

@Composable
private fun Entries(title: String, items: List<String>, icon: ImageVector, add: String, onAdd: () -> Unit, onOpen: ((String) -> Unit)? = null, onRemove: (String) -> Unit) {
    var all by rememberSaveable { mutableStateOf(false) }
    Header(listOf(title, items.size.takeIf { it > 0 }?.toString()).filterNotNull().joinToString(" · "))
    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        val shown = if (all) items else items.take(PREVIEW)
        shown.forEach { v -> Entry(v, icon, onOpen = onOpen?.let { f -> { f(v) } }) { onRemove(v) } }
        if (shown.size < items.size) {
            Chip(tr("Ещё ${items.size - shown.size}", "${items.size - shown.size} more"), Modifier.clickable { all = true }, container = MaterialTheme.colorScheme.secondaryContainer)
        }
        AddEntry(add, onAdd)
    }
}

private const val PREVIEW = 30

private val PILL = RoundedCornerShape(8.dp)

@Composable
private fun Entry(text: String, icon: ImageVector?, leading: (@Composable () -> Unit)? = null, faded: Boolean = false, onOpen: (() -> Unit)? = null, onRemove: (() -> Unit)? = null) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    Row(
        Modifier
            .heightIn(min = 32.dp)
            .clip(PILL)
            .border(1.dp, c.outlineVariant, PILL)
            .then(
                when {
                    onOpen != null -> Modifier.clickable {
                        view.haptic(Haptic.Tap)
                        onOpen()
                    }

                    onRemove != null -> Modifier.clickable {
                        view.haptic(Haptic.Tap)
                        onRemove()
                    }

                    else -> Modifier
                }
            )
            .padding(start = if (leading != null || icon != null) 8.dp else 12.dp, end = if (onRemove != null) 6.dp else 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (leading != null || icon != null) {
            Box(Modifier.alpha(if (faded) 0.38f else 1f)) {
                leading?.invoke() ?: icon?.let { Icon(it, null, tint = c.onSurfaceVariant, modifier = Modifier.size(18.dp)) }
            }
            Spacer(Modifier.width(8.dp))
        }
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            color = c.onSurface,
            maxLines = 1,
            overflow = TextOverflow.MiddleEllipsis,
            modifier = Modifier.weight(1f, fill = false).alpha(if (faded) 0.38f else 1f)
        )
        if (onRemove != null) {
            Spacer(Modifier.width(4.dp))
            Icon(
                Icons.Rounded.Close,
                tr("Убрать", "Remove"),
                tint = c.onSurfaceVariant,
                modifier = Modifier.size(18.dp).then(
                    if (onOpen != null) {
                        Modifier.clip(CircleShape).clickable {
                            view.haptic(Haptic.Tap)
                            onRemove()
                        }
                    } else {
                        Modifier
                    }
                )
            )
        }
    }
}

@Composable
private fun AppEntry(pkg: String, label: String?, installed: Boolean, onRemove: () -> Unit) {
    Entry(label ?: pkg, null, leading = { AppIcon(pkg, 20) }, faded = !installed, onRemove = onRemove)
}

@Composable
private fun AppIcon(pkg: String, size: Int) {
    val ctx = LocalContext.current
    val icon by produceState<ImageBitmap?>(null, pkg) {
        value = withContext(Dispatchers.IO) {
            runCatching { ctx.packageManager.getApplicationIcon(pkg).toBitmap(size * 3, size * 3).asImageBitmap() }.getOrNull()
        }
    }
    Box(Modifier.size(size.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest)) {
        icon?.let { Image(it, null, Modifier.fillMaxSize()) }
    }
}

@Composable
private fun AddEntry(text: String, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    Row(
        Modifier.heightIn(min = 32.dp).clip(PILL).background(c.secondaryContainer).clickable {
            view.haptic(Haptic.Tap)
            onClick()
        }.padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(Icons.Rounded.Add, null, tint = c.onSecondaryContainer, modifier = Modifier.size(16.dp))
        Text(text, color = c.onSecondaryContainer, style = MaterialTheme.typography.labelLarge, maxLines = 1)
    }
}

@Composable
internal fun coverWarnings(settings: Settings, action: RuleAction, kind: RuleKind, values: List<String>): List<String> {
    val ctx = LocalContext.current
    val sub = remember { SubscriptionRules.parse(Subs.list(ctx).let { all -> (all.firstOrNull { it.id == Subs.selection(ctx)?.sub } ?: all.firstOrNull())?.info?.routing }) }
    val smart = remember { ModeRules.smart() }
    val found by produceState(emptyList<String>(), settings, action, kind, values) {
        value = if (values.isEmpty()) {
            emptyList()
        } else {
            withContext(Dispatchers.IO) {
                val sources = RuleCover.sources(settings, smart, ModeRules.subscription(sub))
                val geo = GeoLists.lookup(ctx)
                values.mapNotNull { v -> RuleCover.find(kind, v, sources, geo)?.let { RuleCover.message(v, action, it) } }.take(3)
            }
        }
    }
    return found
}

@Composable
internal fun CoverNote(warnings: List<String>) {
    warnings.forEach {
        Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary, modifier = Modifier.padding(top = 8.dp, start = 4.dp))
    }
}

@Composable
private fun EntriesDialog(
    title: String,
    placeholder: String,
    norm: (String) -> String?,
    onDismiss: () -> Unit,
    settings: Settings? = null,
    action: RuleAction = RuleAction.Proxy,
    kind: RuleKind? = null,
    onAdd: (List<String>) -> Unit
) {
    var text by rememberSaveable { mutableStateOf("") }
    val parts = RuleSets.split(text)
    val good = parts.mapNotNull(norm)
    val ok = good.isNotEmpty() && good.size == parts.size
    val warnings = if (settings != null && kind != null && ok) coverWarnings(settings, action, kind, good) else emptyList()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(
                    text, { text = it },
                    placeholder = { Text(placeholder) },
                    isError = parts.isNotEmpty() && !ok,
                    minLines = 3,
                    maxLines = 8,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier.fillMaxWidth().remoteField(MaterialTheme.shapes.large)
                )
                CoverNote(warnings)
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onAdd(good.distinct())
                onDismiss()
            }, enabled = ok) {
                val n = if (good.size > 1) " · ${good.size}" else ""
                Text(if (warnings.isNotEmpty()) tr("Всё равно добавить$n", "Add anyway$n") else tr("Добавить$n", "Add$n"))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

@Composable
private fun AppsSheet(apps: List<App>, initial: List<String>, onDismiss: () -> Unit, onDone: (List<String>) -> Unit) {
    var picked by rememberSaveable { mutableStateOf(initial) }
    var query by rememberSaveable { mutableStateOf("") }
    var system by rememberSaveable { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    val searching = focused || query.isNotBlank()
    val q = query.trim()
    val found = remember(q, apps, system, picked) { apps.filter { (system || !it.system || it.pkg in picked) && (q.isEmpty() || it.label.contains(q, true) || it.pkg.contains(q, true)) } }
    AppSheet(
        title = tr("Приложения", "Apps"),
        onDismiss = onDismiss,
        fillMaxHeight = searching,
        animateHeight = true,
        imePadding = true,
        footer = {
            Box(Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) {
                PillButton(
                    if (picked.isNotEmpty()) tr("Готово · ${picked.size}", "Done · ${picked.size}") else tr("Готово", "Done"),
                    {
                        onDone(picked)
                        onDismiss()
                    },
                    Modifier.fillMaxWidth()
                )
            }
        }
    ) {
        Column(
            Modifier
                .padding(horizontal = 20.dp)
                .then(if (searching) Modifier.fillMaxHeight() else Modifier)
                .hideKeyboardOnScroll(focused)
        ) {
            TextField(
                query,
                { query = it },
                placeholder = { Text(tr("Поиск", "Search")) },
                singleLine = true,
                shape = MaterialTheme.shapes.large,
                colors = TextFieldDefaults.colors(focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent),
                modifier = Modifier.fillMaxWidth().onFocusChanged { focused = it.hasFocus }.remoteField(MaterialTheme.shapes.large)
            )
            Row(Modifier.padding(top = 8.dp)) { ToggleChip(tr("Системные", "System"), system) { system = !system } }
            LazyColumn(Modifier.padding(top = 8.dp).then(if (searching) Modifier.weight(1f) else Modifier.heightIn(max = 420.dp))) {
                itemsIndexed(found, key = { _, a -> a.pkg }) { i, app ->
                    val prev = found.getOrNull(i - 1)?.pkg in picked
                    val next = found.getOrNull(i + 1)?.pkg in picked
                    AppChoice(app, app.pkg in picked, joinTop = prev, joinBottom = next, modifier = Modifier.animateItem()) {
                        picked = if (app.pkg in picked) picked - app.pkg else picked + app.pkg
                    }
                }
            }
        }
    }
}

@Composable
private fun Counts(set: RuleSet) {
    FlowRow(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        if (set.domains.isNotEmpty()) Chip(plural(set.domains.size, "сайт", "сайта", "сайтов", "site"), icon = Glyph.Globe)
        if (set.apps.isNotEmpty()) Chip(plural(set.apps.size, "приложение", "приложения", "приложений", "app"), icon = Glyph.Apps)
        if (set.geosite.isNotEmpty()) Chip(plural(set.geosite.size, "категория", "категории", "категорий", "category", "categories"), icon = Glyph.List)
        if (set.ips.isNotEmpty()) Chip(plural(set.ips.size, "сеть", "сети", "сетей", "network"), icon = Glyph.Route)
    }
}

@Composable
internal fun ShareSetSheet(set: RuleSet, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val link = remember(set) { RuleSets.link(set) }
    AppSheet(title = setName(set), onDismiss = onDismiss) {
        Column(
            Modifier.padding(horizontal = 24.dp).padding(bottom = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            if (link.length <= QR_MAX) {
                QrCode(link, Modifier.fillMaxWidth(0.8f))
            } else {
                val parts = remember(link) { RuleSets.parts(link) }
                var frame by remember { mutableIntStateOf(0) }
                val lifecycle = androidx.lifecycle.compose.LocalLifecycleOwner.current.lifecycle
                LaunchedEffect(parts) {
                    lifecycle.repeatOnLifecycle(androidx.lifecycle.Lifecycle.State.STARTED) {
                        while (true) {
                            delay(QR_FRAME_MS)
                            frame = (frame + 1) % parts.size
                        }
                    }
                }
                QrCode(parts[frame], Modifier.fillMaxWidth(0.8f), stream = true)
                LinearWavyProgressIndicator(progress = { (frame + 1f) / parts.size }, modifier = Modifier.fillMaxWidth(0.8f))
                Text(
                    tr("Часть ${frame + 1} из ${parts.size}", "Part ${frame + 1} of ${parts.size}"),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Counts(set)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton(
                    tr("Скопировать", "Copy"),
                    { clipboard.setText(AnnotatedString(link)) },
                    Modifier.weight(1f),
                    accent = false,
                    container = MaterialTheme.colorScheme.surfaceContainerHighest,
                    padding = 12.dp,
                    leading = { Icon(Glyph.Copy, null, modifier = Modifier.size(20.dp)) }
                )
                PillButton(
                    tr("Поделиться", "Share"),
                    {
                        val send = if (link.length <= TEXT_MAX) {
                            Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, link)
                        } else {
                            val dir = java.io.File(ctx.cacheDir, "export").apply { mkdirs() }
                            val file = java.io.File(dir, "${setName(set).replace(Regex("[^\\p{L}\\p{N} _-]"), "").ifBlank { "set" }}.rynet.txt").apply { writeText(link) }
                            Intent(Intent.ACTION_SEND).setType("text/plain")
                                .putExtra(Intent.EXTRA_STREAM, androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file))
                                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        }
                        runCatching { ctx.startActivity(Intent.createChooser(send, setName(set))) }
                    },
                    Modifier.weight(1f),
                    padding = 12.dp,
                    leading = { Icon(Glyph.Upload, null, modifier = Modifier.size(20.dp)) }
                )
            }
        }
    }
}

private const val QR_MAX = 900
private const val QR_FRAME_MS = 750L
private const val TEXT_MAX = 3800

@Composable
internal fun ImportSetSheet(set: RuleSet, apps: List<App>, onDismiss: () -> Unit, onAdd: (RuleSet) -> Unit) {
    val c = MaterialTheme.colorScheme
    var action by rememberSaveable { mutableStateOf(set.action) }
    var server by remember { mutableStateOf(set.server) }
    val labels = remember(apps) { apps.associate { it.pkg to it.label } }
    AppSheet(
        title = tr("Набор по ссылке", "Set from a link"),
        onDismiss = onDismiss,
        footer = {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton(tr("Отмена", "Cancel"), onDismiss, Modifier.weight(1f), accent = false)
                PillButton(
                    tr("Добавить", "Add"),
                    {
                        onAdd(set.copy(action = action, server = server.takeIf { action == RuleAction.Server }))
                        onDismiss()
                    },
                    Modifier.weight(1f),
                    enabled = action != RuleAction.Server || server != null
                )
            }
        }
    ) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                ShapeIcon(setIcon(set.icon), setShape(set), c.primaryContainer, c.onPrimaryContainer, size = 56.dp)
                Spacer(Modifier.width(16.dp))
                Text(setName(set), style = MaterialTheme.typography.headlineSmallEmphasized, modifier = Modifier.weight(1f))
            }
            Spacer(Modifier.height(16.dp))
            Counts(set)
            Preview(tr("Сайты", "Sites"), set.domains)
            if (set.apps.isNotEmpty()) {
                Header(tr("Приложения", "Apps"))
                FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    set.apps.take(IMPORT_PREVIEW).forEach { pkg ->
                        val have = pkg in labels
                        Entry(labels[pkg] ?: pkg, null, leading = { AppIcon(pkg, 20) }, faded = !have)
                    }
                    if (set.apps.size > IMPORT_PREVIEW) Chip(tr("Ещё ${set.apps.size - IMPORT_PREVIEW}", "${set.apps.size - IMPORT_PREVIEW} more"))
                }
            }
            Preview(tr("Категории", "Categories"), set.geosite, geo = true)
            Preview(tr("Сети", "Networks"), set.ips)
            Header(tr("Куда", "Where"))
            Choice(RuleAction.entries, action, { it.title }, { action = it })
            AnimatedVisibility(action == RuleAction.Server, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                ServerPicker(server) { server = it }
            }
        }
    }
}

private const val IMPORT_PREVIEW = 8

@Composable
private fun Preview(title: String, items: List<String>, geo: Boolean = false) {
    if (items.isEmpty()) return
    Header(title)
    val uri = androidx.compose.ui.platform.LocalUriHandler.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.take(IMPORT_PREVIEW).forEach { v -> Entry(v, null, onOpen = if (geo) ({ runCatching { uri.openUri(geoLink("geosite", v)) } }) else null) }
        if (items.size > IMPORT_PREVIEW) Chip(tr("Ещё ${items.size - IMPORT_PREVIEW}", "${items.size - IMPORT_PREVIEW} more"))
    }
}

@Composable
internal fun SetLinkDialog(initial: String, onDismiss: () -> Unit, onResult: (RuleSet) -> Unit) {
    val clipboard = LocalClipboardManager.current
    var text by rememberSaveable {
        mutableStateOf(initial.ifEmpty { clipboard.getText()?.text?.trim()?.takeIf { RuleSets.isLink(it) }.orEmpty() })
    }
    var scan by remember { mutableStateOf(false) }
    val set = remember(text) { RuleSets.parse(text) }
    val ctx = LocalContext.current
    val file = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            runCatching {
                ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() }?.takeIf { it.size <= 1 shl 20 }?.decodeToString()?.trim()
            }.getOrNull()?.let { text = it }
        }
    }
    if (scan) {
        val parts = remember { RuleSets.Assembler() }
        var got by remember { mutableIntStateOf(0) }
        QrScanDialog(
            onResult = {
                text = it
                scan = false
            },
            onDismiss = { scan = false },
            title = tr("QR-код набора", "Set QR code"),
            keep = { t ->
                if (!RuleSets.isPart(t)) return@QrScanDialog false
                val whole = parts.add(t)
                got = parts.count
                if (whole != null) {
                    text = whole
                    scan = false
                }
                true
            },
            progress = if (got > 0) tr("Собрано $got из ${parts.total}", "Got $got of ${parts.total}") else null
        )
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("Набор по ссылке", "Set from a link")) },
        text = {
            OutlinedTextField(
                text,
                { text = it },
                placeholder = { Text("rynet://set/…") },
                isError = text.isNotBlank() && set == null,
                maxLines = 4,
                shape = MaterialTheme.shapes.large,
                trailingIcon = {
                    androidx.compose.material3.IconButton(onClick = { scan = true }) { Icon(Glyph.QrScan, tr("Сканировать QR", "Scan QR")) }
                },
                modifier = Modifier.fillMaxWidth().remoteField(MaterialTheme.shapes.large)
            )
        },
        confirmButton = {
            TextButton(onClick = {
                set?.let {
                    onDismiss()
                    onResult(it)
                }
            }, enabled = set != null) { Text(tr("Открыть", "Open")) }
        },
        dismissButton = {
            Row {
                TextButton(onClick = { file.launch(arrayOf("text/*", "application/octet-stream")) }) { Text(tr("Из файла", "From file")) }
                TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) }
            }
        }
    )
}

internal fun geoLink(kind: String, name: String): String = "${Endpoints.GEO_PAGES}/$kind/${name.lowercase()}.list"
