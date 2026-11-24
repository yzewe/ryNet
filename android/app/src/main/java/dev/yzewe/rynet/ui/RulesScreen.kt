@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FloatingActionButtonMenu
import androidx.compose.material3.FloatingActionButtonMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.ToggleFloatingActionButton
import androidx.compose.material3.ToggleFloatingActionButtonDefaults.animateIcon
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import dev.yzewe.rynet.data.ModeEntry
import dev.yzewe.rynet.data.ModeRules
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.RoutingMode
import dev.yzewe.rynet.data.Rule
import dev.yzewe.rynet.data.RuleAction
import dev.yzewe.rynet.data.RuleKind
import dev.yzewe.rynet.data.RuleSet
import dev.yzewe.rynet.data.RuleSets
import dev.yzewe.rynet.data.Rules
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.Subscription
import dev.yzewe.rynet.data.SubscriptionRules
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

internal data class App(val pkg: String, val label: String, val system: Boolean = false)

@Composable
internal fun rememberApps(): List<App> {
    val ctx = LocalContext.current
    val apps by produceState<List<App>>(emptyList()) {
        val pm = ctx.packageManager
        val launcher = withContext(Dispatchers.IO) {
            pm.queryIntentActivities(Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER), 0)
                .map { App(it.activityInfo.packageName, it.loadLabel(pm).toString()) }
                .filter { it.pkg != ctx.packageName }
                .distinctBy { it.pkg }
                .sortedBy { it.label.lowercase() }
        }
        value = launcher
        value = withContext(Dispatchers.IO) {
            val known = launcher.mapTo(HashSet()) { it.pkg } + ctx.packageName
            val all = if (Build.VERSION.SDK_INT >= 33) {
                pm.getInstalledApplications(PackageManager.ApplicationInfoFlags.of(0))
            } else {
                @Suppress("DEPRECATION")
                pm.getInstalledApplications(0)
            }
            val system = all.filter { it.packageName !in known && it.flags and ApplicationInfo.FLAG_HAS_CODE != 0 }
                .map { App(it.packageName, runCatching { it.loadLabel(pm).toString() }.getOrDefault(it.packageName), system = true) }
            (launcher + system).sortedBy { it.label.lowercase() }
        }
    }
    return apps
}

@Composable
fun RulesScreen(
    settings: Settings,
    onChange: (Settings) -> Unit,
    onRaw: () -> Unit,
    onSet: (String?) -> Unit,
    onBack: () -> Unit,
    incoming: String? = null
) {
    val ctx = LocalContext.current
    val subscription = remember {
        val all = Subs.list(ctx)
        val selected = Subs.selection(ctx)
        val sub = all.firstOrNull { it.id == selected?.sub }
        val happ = SubscriptionRules.parse(sub?.info?.routing)
        val own = sub?.let { ModeRules.fromConfig(Subs.body(ctx, it.id), selected?.profile) }
        when {
            own != null -> SubscriptionView(happ?.name.orEmpty(), own.first, own.second)
            happ != null -> SubscriptionView(happ.name, ModeRules.subscription(happ), happ.rest)
            else -> null
        }
    }
    var subscriptionOpen by remember { mutableStateOf(false) }
    var smartOpen by remember { mutableStateOf(false) }
    if (smartOpen) {
        val smart = remember { ModeRules.smart() }
        ModeRulesSheet(tr("Режим Умный", "Smart mode"), smart, null, { smartOpen = false }, privateNets = remember { ModeRules.privateNets() })
    }
    if (subscriptionOpen) {
        (subscription ?: SubscriptionView("", emptyList(), RuleAction.Proxy)).let { v ->
            ModeRulesSheet(v.name.ifBlank { tr("Правила подписки", "Subscription rules") }, v.entries, v.rest, { subscriptionOpen = false })
        }
    }
    var editing by remember { mutableStateOf<Pair<Int, Rule?>?>(null) }
    val apps = rememberApps()
    val labels = remember(apps) { apps.associate { it.pkg to it.label } }
    val c = MaterialTheme.colorScheme
    var importing by remember { mutableStateOf(incoming?.let(RuleSets::parse)) }
    var linking by rememberSaveable { mutableStateOf(incoming != null && importing == null) }
    var tailscale by rememberSaveable { mutableStateOf(false) }
    if (tailscale) {
        TailscaleSetSheet(onDismiss = { tailscale = false }) { server ->
            onChange(settings.copy(sets = settings.sets + RuleSets.tailscale(server)))
            tailscale = false
        }
    }
    var menu by rememberSaveable { mutableStateOf(false) }
    BackHandler(menu) { menu = false }
    val list = rememberLazyListState()
    val current by rememberUpdatedState(settings)
    val change by rememberUpdatedState(onChange)
    val view = LocalView.current
    var setMenu by remember { mutableStateOf<String?>(null) }
    val pick = remember { HoldPick() }
    var sharing by remember { mutableStateOf<RuleSet?>(null) }
    var deleting by remember { mutableStateOf<RuleSet?>(null) }
    fun toggle(id: String) {
        val t = current.sets.firstOrNull { it.id == id } ?: return
        view.haptic(if (t.enabled) Haptic.ToggleOff else Haptic.ToggleOn)
        change(current.copy(sets = current.sets.map { if (it.id == id) it.copy(enabled = !it.enabled) else it }))
    }
    PageScaffold(tr("Правила", "Rules"), onBack = onBack, list = list, floatingActionButton = {
        AddMenu(
            menu,
            { menu = it },
            onRule = { editing = -1 to null },
            onSet = { onSet(null) },
            onLink = { linking = true },
            onTailscale = { tailscale = true }
        )
    }, bottom = 96.dp) {
        val smart = settings.routing == RoutingMode.Smart
        val bySub = if (settings.routing == RoutingMode.Subscription) subscription ?: SubscriptionView("", emptyList(), RuleAction.Proxy) else null
        if (smart || bySub != null) {
            item(key = "modes") {
                Header(tr("Закреплённые", "Pinned"))
                if (smart) {
                    ModeSetRow(
                        tr("Режим Умный", "Smart mode"),
                        tr("Российское и локальная сеть — напрямую", "Russian and local traffic goes direct"),
                        0,
                        1,
                        true
                    ) { smartOpen = true }
                }
                bySub?.let { rules ->
                    SubscriptionSetRow(rules, true, 0, 1) { subscriptionOpen = true }
                }
            }
        }
        if (settings.sets.isNotEmpty()) item(key = "sets") { Header(tr("Наборы", "Sets")) }
        itemsIndexed(settings.sets, key = { _, set -> "set:${set.id}" }) { i, set ->
            Box(Modifier.animateItem()) {
                var at by remember { mutableStateOf(androidx.compose.ui.geometry.Offset.Zero) }
                SetRow(
                    set,
                    i,
                    settings.sets.size,
                    Modifier.holdMenu(pick) {
                        at = it
                        setMenu = set.id
                    },
                    onClick = { toggle(set.id) }
                )
                Box(Modifier.offset { androidx.compose.ui.unit.IntOffset(at.x.toInt(), at.y.toInt()) }.size(1.dp)) {
                    SetMenu(
                        pick,
                        open = setMenu == set.id,
                        shareable = set.size > 0,
                        onDismiss = { setMenu = null },
                        onEdit = {
                            setMenu = null
                            onSet(set.id)
                        },
                        onShare = {
                            setMenu = null
                            sharing = set
                        },
                        onDelete = {
                            setMenu = null
                            deleting = set
                        }
                    )
                }
            }
        }
        if (settings.rules.isNotEmpty()) item(key = "rules") { Header(tr("Отдельные правила", "Single rules")) }
        itemsIndexed(settings.rules, key = { i, r -> "$i:${r.kind.key}:${r.value}" }) { i, rule ->
            RuleRow(rule, labels[rule.value], i, settings.rules.size, Modifier.animateItem()) { editing = i to rule }
        }
        item(key = "rest") {
            Column(Modifier.padding(top = 16.dp)) {
                val extra = settings.userRules.lines().count { it.isNotBlank() && !it.trim().startsWith("#") }
                val appRules = settings.rules.any { it.kind == RuleKind.App && it.action != RuleAction.Direct } ||
                    settings.sets.any { it.enabled && it.apps.isNotEmpty() && it.action != RuleAction.Direct }
                Group(
                    items = listOfNotNull(
                        Item(Glyph.Apps, MaterialShapes.Clover4Leaf, tr("Остальные приложения мимо VPN", "Other apps bypass VPN"), tr("Через VPN идут только приложения из правил", "Only apps from the rules go through the VPN"), settings.vpnAppsOnly) {
                            onChange(settings.copy(vpnAppsOnly = !settings.vpnAppsOnly))
                        }.takeIf { appRules || settings.vpnAppsOnly },
                        Item(Icons.Rounded.Edit, MaterialShapes.Pill, tr("Дополнительно (mihomo)", "Advanced (mihomo)"), if (extra > 0) plural(extra, "строка", "строки", "строк", "line") else tr("Свои правила в формате mihomo", "Your own rules in mihomo format"), onClick = onRaw)
                    )
                )
            }
        }
    }
    sharing?.let { ShareSetSheet(it) { sharing = null } }
    deleting?.let { set ->
        DeleteDialog(
            tr("Удалить набор?", "Delete the set?"),
            setName(set),
            tr("Удалить", "Delete"),
            onConfirm = { change(current.copy(sets = current.sets.filter { it.id != set.id })) },
            onDismiss = { deleting = null }
        )
    }
    if (linking) SetLinkDialog(incoming.orEmpty().takeIf { importing == null }.orEmpty(), onDismiss = { linking = false }) { importing = it }
    importing?.let { set ->
        ImportSetSheet(set, apps, onDismiss = { importing = null }) { onChange(settings.copy(sets = settings.sets + it)) }
    }
    editing?.let { (index, rule) ->
        RuleEditor(
            settings = settings,
            initial = rule,
            apps = apps,
            onDismiss = { editing = null },
            onDelete = if (index >= 0) {
                (
                    {
                        onChange(settings.copy(rules = settings.rules.toMutableList().also { it.removeAt(index) }))
                        editing = null
                    }
                    )
            } else {
                null
            }
        ) { rules ->
            val list = settings.rules.toMutableList()
            if (index >= 0) {
                list[index] = rules.first()
            } else {
                for (r in rules) {
                    val at = list.indexOfFirst { it.kind == r.kind && it.value == r.value }
                    if (at >= 0) list[at] = r else list.add(r)
                }
            }
            onChange(settings.copy(rules = list))
            editing = null
        }
    }
}

@Composable
private fun AddMenu(open: Boolean, onOpen: (Boolean) -> Unit, onRule: () -> Unit, onSet: () -> Unit, onLink: () -> Unit, onTailscale: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val bounds = remember { mutableStateMapOf<Int, Rect>() }
    var hovered by remember { mutableStateOf<Int?>(null) }
    var origin by remember { mutableStateOf(Offset.Zero) }
    val actions by rememberUpdatedState(listOf(onLink, onSet, onRule, onTailscale))
    val setOpen by rememberUpdatedState(onOpen)
    fun pick(i: Int) {
        setOpen(false)
        actions[i]()
    }
    FloatingActionButtonMenu(
        expanded = open,
        button = {
            ToggleFloatingActionButton(
                checked = open,
                onCheckedChange = onOpen,
                modifier = Modifier
                    .onGloballyPositioned { origin = it.positionInRoot() }
                    .pointerInput(Unit) {
                        awaitEachGesture {
                            val down = awaitFirstDown(requireUnconsumed = false)
                            val long = awaitLongPressOrCancellation(down.id) ?: return@awaitEachGesture
                            long.consume()
                            view.haptic(Haptic.LongPress)
                            setOpen(true)
                            while (true) {
                                val event = awaitPointerEvent(PointerEventPass.Initial)
                                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                                change.consume()
                                val at = bounds.entries.firstOrNull { it.value.contains(origin + change.position) }?.key
                                if (at != hovered) {
                                    hovered = at
                                    if (at != null) view.haptic(Haptic.Segment)
                                }
                                if (!change.pressed) break
                            }
                            val at = hovered
                            hovered = null
                            if (at != null) {
                                view.haptic(Haptic.Confirm)
                                pick(at)
                            }
                        }
                    }
            ) {
                val icon by remember { derivedStateOf { if (checkedProgress > 0.5f) Icons.Rounded.Close else Icons.Rounded.Add } }
                Icon(rememberVectorPainter(icon), contentDescription = tr("Добавить", "Add"), modifier = Modifier.animateIcon({ checkedProgress }))
            }
        }
    ) {
        val items = listOf(
            Glyph.Link to tr("Набор по ссылке", "Set from a link"),
            Glyph.Grid to tr("Набор", "Set"),
            Glyph.Route to tr("Правило", "Rule"),
            Glyph.Lan to "Tailscale"
        )
        items.forEachIndexed { i, (glyph, label) ->
            val grow by animateFloatAsState(if (hovered == i) 1.08f else 1f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "fab item")
            FloatingActionButtonMenuItem(
                onClick = { pick(i) },
                icon = { Icon(glyph, contentDescription = null) },
                text = { Text(label) },
                containerColor = c.primaryContainer,
                contentColor = c.onPrimaryContainer,
                modifier = Modifier.onGloballyPositioned { bounds[i] = it.boundsInRoot() }.graphicsLayer {
                    transformOrigin = TransformOrigin(1f, 0.5f)
                    scaleX = grow
                    scaleY = grow
                }
            )
        }
    }
}

@Composable
private fun RuleRow(rule: Rule, label: String?, index: Int, count: Int, modifier: Modifier, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val icon by produceState<ImageBitmap?>(null, rule) {
        value = if (rule.kind != RuleKind.App) {
            null
        } else {
            withContext(Dispatchers.IO) {
                runCatching { ctx.packageManager.getApplicationIcon(rule.value).toBitmap(96, 96).asImageBitmap() }.getOrNull()
            }
        }
    }
    val src = remember { MutableInteractionSource() }
    SettingsRow(
        shape = Segments.shape(index, count),
        onClick = onClick,
        leading = {
            icon?.let { Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) { Image(it, contentDescription = null, modifier = Modifier.requiredSize(32.dp)) } }
                ?: RowIcon(kindIcon(rule.kind), src)
        },
        interactionSource = src,
        title = if (rule.kind == RuleKind.App) label ?: rule.value else rule.value,
        summary = if (rule.kind == RuleKind.App && label != null) rule.value else rule.kind.title,
        modifier = modifier.padding(bottom = Segments.Gap),
        oneLine = true,
        stackedTrailing = true,
        overflow = if (rule.kind == RuleKind.App && label != null) TextOverflow.Ellipsis else TextOverflow.MiddleEllipsis
    ) { ActionTag(rule.action, rule.server?.profile) }
}

private fun kindIcon(k: RuleKind) = when (k) {
    RuleKind.Domain -> Glyph.Link
    RuleKind.Ip -> Glyph.Route
    RuleKind.App -> Glyph.Apps
}

@Composable
internal fun ActionTag(a: RuleAction, server: String? = null, modifier: Modifier = Modifier) {
    val c = MaterialTheme.colorScheme
    val (bg, fg) = when (a) {
        RuleAction.Proxy -> c.primaryContainer to c.onPrimaryContainer
        RuleAction.Direct -> c.secondaryContainer to c.onSecondaryContainer
        RuleAction.Block -> c.errorContainer to c.onErrorContainer
        RuleAction.Server -> c.tertiaryContainer to c.onTertiaryContainer
    }
    val (flag, name) = server?.let { Subscription.splitName(it) } ?: ("" to a.title)
    Row(
        modifier.clip(MaterialTheme.shapes.small).background(bg).padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (flag.isNotEmpty()) {
            FlagIcon(flag, Modifier.size(width = 18.dp, height = 13.dp), RoundedCornerShape(3.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(name, style = MaterialTheme.typography.labelLarge, color = fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun RuleEditor(
    settings: Settings,
    initial: Rule?,
    apps: List<App>,
    onDismiss: () -> Unit,
    onDelete: (() -> Unit)?,
    onSave: (List<Rule>) -> Unit
) {
    var kind by rememberSaveable { mutableStateOf(initial?.kind ?: RuleKind.Domain) }
    var action by rememberSaveable { mutableStateOf(initial?.action ?: RuleAction.Proxy) }
    var serverSub by rememberSaveable { mutableStateOf(initial?.server?.sub.orEmpty()) }
    var serverProfile by rememberSaveable { mutableStateOf(initial?.server?.profile.orEmpty()) }
    val server = Selection(serverSub, serverProfile).takeIf { serverSub.isNotEmpty() && serverProfile.isNotEmpty() }
    var text by rememberSaveable { mutableStateOf(initial?.value.orEmpty()) }
    val multi = initial == null
    var picked by rememberSaveable { mutableStateOf(if (initial?.kind == RuleKind.App) listOf(initial.value) else emptyList()) }
    var system by rememberSaveable { mutableStateOf(false) }
    val value = when (kind) {
        RuleKind.App -> picked.firstOrNull() ?: text.trim()
        else -> text.trim()
    }
    val valid = Rule(kind, value, action, server).valid()
    val coverValues = when (kind) {
        RuleKind.Domain -> listOfNotNull(Rules.domain(value))
        RuleKind.App -> picked.ifEmpty { listOf(value) }
        else -> listOf(Rules.cidr(value)?.first ?: "")
    }.filter { it.isNotBlank() }
    val warnings = if (initial == null && valid) coverWarnings(settings, action, kind, coverValues) else emptyList()
    val c = MaterialTheme.colorScheme
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.padding(bottom = 24.dp).imePadding()) {
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp)) {
                Text(if (initial == null) tr("Новое правило", "New rule") else tr("Правило", "Rule"), style = MaterialTheme.typography.headlineSmallEmphasized)
                Spacer(Modifier.height(16.dp))
                Choice(RuleKind.entries, kind, { it.title }, {
                    if (it != kind) {
                        kind = it
                        text = ""
                        picked = emptyList()
                    }
                })
                Spacer(Modifier.height(12.dp))
                TextField(
                    value = if (kind == RuleKind.App && !multi && picked.isNotEmpty()) apps.firstOrNull { it.pkg == picked[0] }?.label ?: picked[0] else text,
                    onValueChange = {
                        text = it
                        if (!multi) picked = emptyList()
                    },
                    placeholder = {
                        Text(
                            when (kind) {
                                RuleKind.Domain -> "youtube.com"
                                RuleKind.Ip -> "10.0.0.0/8"
                                RuleKind.App -> tr("Поиск или пакет", "Search or package name")
                            }
                        )
                    },
                    singleLine = true,
                    isError = text.isNotBlank() && picked.isEmpty() && !valid && kind != RuleKind.App,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = if (kind == RuleKind.App) FontFamily.Default else FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    colors = TextFieldDefaults.colors(focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent),
                    keyboardOptions = KeyboardOptions(keyboardType = if (kind == RuleKind.App) KeyboardType.Text else KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth().remoteField(MaterialTheme.shapes.large)
                )
                AnimatedVisibility(kind == RuleKind.App, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                    val q = text.trim()
                    val found = remember(q, apps, system, picked) { apps.filter { (system || !it.system || it.pkg in picked) && (q.isEmpty() || it.label.contains(q, true) || it.pkg.contains(q, true)) } }
                    Column {
                        Row(Modifier.padding(top = 8.dp)) { ToggleChip(tr("Системные", "System"), system) { system = !system } }
                        LazyColumn(Modifier.padding(top = 8.dp).heightIn(max = 260.dp)) {
                            itemsIndexed(found, key = { _, a -> a.pkg }) { i, app ->
                                val prev = found.getOrNull(i - 1)?.pkg in picked
                                val next = found.getOrNull(i + 1)?.pkg in picked
                                AppChoice(app, app.pkg in picked, joinTop = prev, joinBottom = next, modifier = Modifier.animateItem()) {
                                    if (multi) {
                                        picked = if (app.pkg in picked) picked - app.pkg else picked + app.pkg
                                    } else {
                                        picked = listOf(app.pkg)
                                        text = ""
                                    }
                                }
                            }
                        }
                    }
                }
                Spacer(Modifier.height(12.dp))
                Choice(RuleAction.entries, action, { it.title }, { action = it })
                AnimatedVisibility(action == RuleAction.Server, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                    ServerPicker(server) {
                        serverSub = it.sub
                        serverProfile = it.profile
                    }
                }
                CoverNote(warnings)
                AnimatedVisibility(kind == RuleKind.App && action != RuleAction.Direct && Build.VERSION.SDK_INT < 29) {
                    Text(
                        tr("На этой версии Android для приложений работает только «Напрямую»", "On this Android version, apps can only go \"Direct\""),
                        style = MaterialTheme.typography.bodySmall,
                        color = c.error,
                        modifier = Modifier.padding(top = 8.dp, start = 4.dp)
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.padding(horizontal = 20.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (onDelete != null) {
                    FilledTonalButton(
                        onClick = onDelete,
                        shapes = ButtonDefaults.shapes(),
                        colors = ButtonDefaults.filledTonalButtonColors(containerColor = c.errorContainer, contentColor = c.onErrorContainer),
                        modifier = Modifier.weight(1f).height(52.dp)
                    ) { Text(tr("Удалить", "Delete")) }
                }
                Button(
                    onClick = {
                        val to = server.takeIf { action == RuleAction.Server }
                        val values = when (kind) {
                            RuleKind.Domain -> listOf(Rules.domain(value) ?: value)
                            RuleKind.App -> picked.ifEmpty { listOf(value) }
                            else -> listOf(value)
                        }
                        onSave(values.map { Rule(kind, it, action, to) })
                    },
                    enabled = valid,
                    shapes = ButtonDefaults.shapes(),
                    modifier = Modifier.weight(1f).height(52.dp)
                ) {
                    val n = if (kind == RuleKind.App && picked.size > 1) " · ${picked.size}" else ""
                    Text(if (warnings.isNotEmpty()) tr("Всё равно сохранить$n", "Save anyway$n") else tr("Сохранить$n", "Save$n"))
                }
            }
        }
    }
}

@Composable
internal fun AppChoice(
    app: App,
    selected: Boolean,
    subtitle: String? = null,
    joinTop: Boolean = false,
    joinBottom: Boolean = false,
    modifier: Modifier = Modifier,
    onClick: () -> Unit
) {
    val ctx = LocalContext.current
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    val icon by produceState<ImageBitmap?>(null, app.pkg) {
        value = withContext(Dispatchers.IO) {
            runCatching { ctx.packageManager.getApplicationIcon(app.pkg).toBitmap(72, 72).asImageBitmap() }.getOrNull()
        }
    }
    val bg by animateColorAsState(if (selected) c.secondaryContainer else c.secondaryContainer.copy(alpha = 0f), Springs.effects(), label = "appBg")
    val fg by animateColorAsState(if (selected) c.onSecondaryContainer else c.onSurface, Springs.effects(), label = "appFg")
    val top by animateDpAsState(if (selected && joinTop) Segments.Inner else Segments.Outer, Springs.spatial(), label = "appTop")
    val bottom by animateDpAsState(if (selected && joinBottom) Segments.Inner else Segments.Outer, Springs.spatial(), label = "appBottom")
    val check by animateFloatAsState(if (selected) 1f else 0f, if (selected) Springs.bouncy() else Springs.effects(), label = "appCheck")
    Row(
        modifier
            .padding(vertical = Segments.Gap / 2)
            .fillMaxWidth()
            .pressSquish(src, 0.98f)
            .clip(RoundedCornerShape(top.coerceAtLeast(0.dp), top.coerceAtLeast(0.dp), bottom.coerceAtLeast(0.dp), bottom.coerceAtLeast(0.dp)))
            .drawBehind { drawRect(bg) }
            .clickable(src, LocalIndication.current) {
                view.haptic(if (selected) Haptic.ToggleOff else Haptic.ToggleOn)
                onClick()
            }
            .padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(Modifier.size(32.dp)) { icon?.let { Image(it, contentDescription = null, modifier = Modifier.size(32.dp)) } }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(app.label, style = MaterialTheme.typography.bodyLarge, color = fg)
            Text(subtitle ?: app.pkg, style = MaterialTheme.typography.bodySmall, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.MiddleEllipsis)
        }
        Icon(
            Icons.Rounded.Check,
            contentDescription = null,
            tint = c.onSecondaryContainer,
            modifier = Modifier.graphicsLayer {
                scaleX = check
                scaleY = check
                alpha = check.coerceIn(0f, 1f)
                rotationZ = (1f - check) * -45f
            }
        )
    }
}

@Composable
internal fun ServerPicker(selected: Selection?, onPick: (Selection) -> Unit) {
    val ctx = LocalContext.current
    val c = MaterialTheme.colorScheme
    val subs = remember { Subs.list(ctx) }
    var sub by rememberSaveable { mutableStateOf(selected?.sub ?: Subs.selection(ctx)?.sub ?: subs.firstOrNull()?.id) }
    val profiles by produceState(emptyList<Profile>(), sub) {
        value = sub?.let { id -> withContext(Dispatchers.IO) { runCatching { Subs.profiles(ctx, id) }.getOrDefault(emptyList()) } }.orEmpty()
    }
    Column(Modifier.padding(top = 12.dp)) {
        SubChips(subs, sub) { sub = it }
        LazyColumn(Modifier.heightIn(max = 240.dp)) {
            items(profiles.filter { it.supported }, key = { it.remarks }) { p ->
                val on = selected?.sub == sub && selected?.profile == p.remarks
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(MaterialTheme.shapes.medium)
                        .background(if (on) c.secondaryContainer else Color.Transparent)
                        .clickable { sub?.let { onPick(Selection(it, p.remarks)) } }
                        .padding(horizontal = 8.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    FlagIcon(p.flag, Modifier.size(width = 28.dp, height = 21.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(p.name, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    if (on) Icon(Icons.Rounded.Check, contentDescription = null, tint = c.onSecondaryContainer)
                }
            }
        }
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun SetMenu(pick: HoldPick, open: Boolean, shareable: Boolean, onDismiss: () -> Unit, onEdit: () -> Unit, onShare: () -> Unit, onDelete: () -> Unit) {
    androidx.compose.material3.DropdownMenuPopup(expanded = open, onDismissRequest = onDismiss) {
        val n = if (shareable) 2 else 1
        androidx.compose.material3.DropdownMenuGroup(shapes = androidx.compose.material3.MenuDefaults.groupShape(0, 2)) {
            HoldItem(pick, 0, tr("Изменить", "Edit"), { HoldIcon(Icons.Rounded.Edit) }, androidx.compose.material3.MenuDefaults.itemShape(0, n).shape, onClick = onEdit)
            if (shareable) HoldItem(pick, 1, tr("Поделиться", "Share"), { HoldIcon(Glyph.Upload) }, androidx.compose.material3.MenuDefaults.itemShape(1, n).shape, onClick = onShare)
        }
        Spacer(Modifier.height(androidx.compose.material3.MenuDefaults.GroupSpacing))
        androidx.compose.material3.DropdownMenuGroup(shapes = androidx.compose.material3.MenuDefaults.groupShape(1, 2)) {
            HoldItem(pick, 2, tr("Удалить", "Delete"), { HoldIcon(Icons.Rounded.Delete, danger = true) }, androidx.compose.material3.MenuDefaults.itemShape(0, 1).shape, danger = true, onClick = onDelete)
        }
    }
}

internal class SubscriptionView(val name: String, val entries: List<ModeEntry>, val rest: RuleAction?)
