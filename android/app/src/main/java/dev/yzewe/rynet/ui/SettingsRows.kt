@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuGroup
import androidx.compose.material3.DropdownMenuPopup
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.SelectableDropdownMenuItem
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import androidx.graphics.shapes.CornerRounding
import androidx.graphics.shapes.RoundedPolygon
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.data.Hwid
import dev.yzewe.rynet.data.PingMode
import dev.yzewe.rynet.data.RoutingMode
import dev.yzewe.rynet.data.ServerGrid
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.data.ServerView
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.SubClient
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal class Item(
    val icon: ImageVector,
    val shape: RoundedPolygon,
    val title: String,
    val summary: String?,
    val checked: Boolean? = null,
    val action: Boolean = false,
    val busy: Boolean = false,
    val enabled: Boolean = true,
    val onClick: (() -> Unit)?
)

@Composable
internal fun Header(title: String) {
    Text(
        title,
        color = MaterialTheme.colorScheme.primary,
        style = PageType.header,
        modifier = Modifier.padding(top = 20.dp, bottom = 12.dp)
    )
}

@Composable
internal fun Group(
    items: List<Item>,
    top: (@Composable () -> Unit)? = null,
    more: List<@Composable () -> Unit> = emptyList(),
    after: (@Composable () -> Unit)? = null,
    circles: Boolean = false
) {
    val cards = listOfNotNull(top) + more
    Column(Modifier.fillMaxWidth().padding(bottom = Segments.Gap).clip(RoundedCornerShape(Segments.Outer))) {
        var first = true
        fun gap(): Boolean = !first.also { first = false }
        cards.forEach { card ->
            if (gap()) SegmentDivider()
            card()
        }
        items.forEach {
            if (gap()) SegmentDivider()
            SettingRow(it, circles)
        }
        after?.invoke()
    }
}

@Composable
internal fun GroupCard(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(Segments.Inner)).background(MaterialTheme.colorScheme.surfaceContainerHigh)) { content() }
}

@Composable
internal fun Reveal(visible: Boolean, content: @Composable () -> Unit) {
    AnimatedVisibility(
        visible,
        enter = fadeIn(tween(180)) + expandVertically(tween(220)),
        exit = fadeOut(tween(140)) + shrinkVertically(tween(180))
    ) {
        Column { content() }
    }
}

@Composable
internal fun Fold(visible: Boolean, content: @Composable () -> Unit) {
    Reveal(visible) {
        SegmentDivider()
        content()
    }
}

@Composable
internal fun RowIcon(icon: ImageVector, source: InteractionSource, checked: Boolean? = null, circle: Boolean = false) {
    val c = MaterialTheme.colorScheme
    if (circle) {
        Box(Modifier.size(40.dp).clip(CircleShape).background(c.primaryContainer), contentAlignment = Alignment.Center) {
            LiveIcon(icon, checked, source, c.onPrimaryContainer, size = 22.dp)
        }
    } else {
        val tint by animateColorAsState(if (checked == true) c.primary else c.onSurfaceVariant, Springs.effects(), label = "rowIcon")
        LiveIcon(icon, checked, source, tint)
    }
}

@Composable
internal fun SettingRow(it: Item, circle: Boolean = false) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val click = it.onClick
    val src = remember { MutableInteractionSource() }
    SettingsRow(
        shape = RoundedCornerShape(Segments.Inner),
        onClick = click?.let { f ->
            {
                view.haptic(
                    when (it.checked) {
                        null -> Haptic.Tap
                        true -> Haptic.ToggleOff
                        false -> Haptic.ToggleOn
                    }
                )
                f()
            }
        },
        enabled = it.enabled && !it.busy,
        leading = { RowIcon(it.icon, src, it.checked, circle) },
        title = it.title,
        summary = it.summary,
        modifier = Modifier.semantics { if (it.checked != null) role = Role.Switch },
        interactionSource = src
    ) {
        when {
            it.checked != null -> Switch(checked = it.checked, onCheckedChange = null)
            it.busy -> LoadingIndicator(Modifier.size(24.dp))
            it.action || click == null -> {}
            else -> Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null, tint = c.onSurfaceVariant)
        }
    }
}

@Composable
internal fun SettingsRow(
    shape: androidx.compose.ui.graphics.Shape,
    onClick: (() -> Unit)?,
    leading: @Composable () -> Unit,
    title: String,
    summary: String?,
    modifier: Modifier = Modifier,
    oneLine: Boolean = false,
    stackedTrailing: Boolean = false,
    overflow: TextOverflow = TextOverflow.Ellipsis,
    enabled: Boolean = true,
    faded: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    interactionSource: MutableInteractionSource? = null,
    below: (@Composable () -> Unit)? = null,
    trailing: @Composable () -> Unit
) {
    val c = MaterialTheme.colorScheme
    val src = interactionSource ?: remember { MutableInteractionSource() }
    val alpha by androidx.compose.animation.core.animateFloatAsState(if (enabled && !faded) 1f else 0.38f, Springs.effects(), label = "rowAlpha")
    val dim = Modifier.alpha(alpha)
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .remoteFocus(src, shape, inward = true)
            .then(
                when {
                    onClick != null && onLongClick != null -> Modifier.combinedClickable(interactionSource = src, indication = ripple(), enabled = enabled, onLongClick = onLongClick, onClick = onClick)
                    onClick != null -> Modifier.clickable(interactionSource = src, indication = ripple(), enabled = enabled, onClick = onClick)
                    else -> Modifier
                }
            )
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(dim) { leading() }
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f).then(dim), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, color = c.onSurface, style = PageType.rowTitle, maxLines = if (oneLine) 1 else Int.MAX_VALUE, overflow = overflow)
            summary?.let { s -> Text(s, color = c.onSurfaceVariant, style = PageType.rowSummary, maxLines = if (oneLine) 1 else 3, overflow = if (oneLine) TextOverflow.MiddleEllipsis else TextOverflow.Ellipsis) }
            below?.let {
                Spacer(Modifier.height(6.dp))
                it()
            }
            if (stackedTrailing) {
                Spacer(Modifier.height(6.dp))
                trailing()
            }
        }
        if (!stackedTrailing) {
            Spacer(Modifier.width(16.dp))
            Box(dim) { trailing() }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun BackButton(onBack: () -> Unit) {
    IconButton(onClick = onBack, shapes = IconButtonDefaults.shapes()) {
        Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = tr("Назад", "Back"))
    }
}

@Composable
internal fun <T> DropdownRow(
    icon: ImageVector,
    title: String,
    options: List<T>,
    selected: T,
    label: (T) -> String,
    summary: ((T) -> String)? = null,
    enabled: Boolean = true,
    shape: androidx.compose.ui.graphics.Shape = RoundedCornerShape(Segments.Inner),
    onSelect: (T) -> Unit
) {
    val view = LocalView.current
    DropdownShell(icon, title, label(selected), enabled, shape) { close ->
        options.forEach { o ->
            SegmentDivider()
            OptionRow(label(o), summary?.invoke(o)?.takeIf { it.isNotEmpty() }, o == selected) {
                view.haptic(Haptic.Segment)
                close()
                if (o != selected) onSelect(o)
            }
        }
    }
}

@Composable
internal fun <T> DropdownChecks(
    icon: ImageVector,
    title: String,
    options: List<T>,
    checked: Set<T>,
    label: (T) -> String,
    none: String,
    enabled: Boolean = true,
    onToggle: (T, Boolean) -> Unit
) {
    val view = LocalView.current
    val shown = options.filter { it in checked }.joinToString(" · ", transform = label).ifEmpty { none }
    DropdownShell(icon, title, shown, enabled) {
        options.forEach { o ->
            SegmentDivider()
            OptionRow(label(o), null, o in checked, multi = true) {
                view.haptic(if (o in checked) Haptic.ToggleOff else Haptic.ToggleOn)
                onToggle(o, o !in checked)
            }
        }
    }
}

@Composable
private fun DropdownShell(
    icon: ImageVector,
    title: String,
    summary: String,
    enabled: Boolean,
    shape: androidx.compose.ui.graphics.Shape = RoundedCornerShape(Segments.Inner),
    options: @Composable (close: () -> Unit) -> Unit
) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    var open by rememberSaveable { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open && enabled) 180f else 0f, Springs.spatial(), label = "dropTurn")
    Column {
        SettingsRow(
            shape = shape,
            onClick = {
                view.haptic(Haptic.Tap)
                open = !open
            },
            leading = { RowIcon(icon, src) },
            title = title,
            summary = summary,
            enabled = enabled,
            interactionSource = src
        ) {
            Icon(Glyph.ExpandMore, contentDescription = null, tint = c.onSurfaceVariant, modifier = Modifier.graphicsLayer { rotationZ = turn })
        }
        Reveal(open && enabled) { options { open = false } }
    }
}

@Composable
private fun OptionRow(text: String, summary: String?, selected: Boolean, multi: Boolean = false, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(Segments.Inner)
    val src = remember { MutableInteractionSource() }
    val mark by animateFloatAsState(if (selected) 1f else 0f, Springs.bouncy(), label = "optionMark")
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(c.surfaceContainerHigh)
            .remoteFocus(src, shape, inward = true)
            .clickable(interactionSource = src, indication = ripple(), onClick = onClick)
            .semantics {
                role = if (multi) Role.Checkbox else Role.RadioButton
                if (multi) toggleableState = androidx.compose.ui.state.ToggleableState(selected) else this.selected = selected
            }
            .padding(start = 56.dp, end = 16.dp, top = 14.dp, bottom = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(text, color = if (selected && !multi) c.primary else c.onSurface, style = PageType.rowTitle)
            summary?.let { Text(it, color = c.onSurfaceVariant, style = PageType.rowSummary) }
        }
        if (multi) {
            Checkbox(checked = selected, onCheckedChange = null, modifier = Modifier.padding(start = 12.dp))
        } else {
            Icon(
                Icons.Rounded.Check,
                contentDescription = null,
                tint = c.primary,
                modifier = Modifier.padding(start = 12.dp).size(24.dp).graphicsLayer {
                    scaleX = mark
                    scaleY = mark
                    alpha = mark.coerceIn(0f, 1f)
                }
            )
        }
    }
}

@Composable
internal fun BlockTitle(title: String, value: String? = null) {
    val c = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().padding(top = 20.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, color = c.primary, style = PageType.header, modifier = Modifier.weight(1f))
        value?.let { Text(it, color = c.onSurfaceVariant, style = PageType.header) }
    }
}

@Composable
internal fun <T> ChoiceBlock(
    title: String,
    options: List<T>,
    selected: T,
    label: (T) -> String,
    summary: ((T) -> String)? = null,
    enabled: Boolean = true,
    extra: @Composable (T) -> Unit = {},
    onSelect: (T) -> Unit
) {
    val c = MaterialTheme.colorScheme
    val alpha by animateFloatAsState(if (enabled) 1f else 0.38f, Springs.effects(), label = "choiceAlpha")
    Column(Modifier.fillMaxWidth().padding(bottom = 12.dp)) {
        BlockTitle(title)
        Column(Modifier.alpha(alpha)) {
            Choice(options, selected, label, onSelect, enabled = enabled)
            if (summary != null && options.any { summary(it).isNotEmpty() }) {
                AnimatedContent(
                    targetState = selected,
                    transitionSpec = { fadeIn(tween(180)) togetherWith fadeOut(tween(140)) },
                    label = "choiceSummary"
                ) { o ->
                    Text(summary(o), style = PageType.rowSummary, color = c.onSurfaceVariant, modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
                }
            }
        }
        extra(selected)
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun <T> IconToggleRow(
    icon: ImageVector,
    title: String,
    options: List<T>,
    selected: T,
    label: (T) -> String,
    optionIcon: (T) -> ImageVector,
    onSelect: (T) -> Unit
) {
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    var shown by remember { mutableStateOf(selected) }
    LaunchedEffect(selected) {
        if (selected == shown) return@LaunchedEffect
        shown = selected
        val press = PressInteraction.Press(Offset.Zero)
        src.emit(press)
        src.emit(PressInteraction.Release(press))
    }
    SettingsRow(
        shape = RoundedCornerShape(Segments.Inner),
        onClick = null,
        leading = { RowIcon(icon, src) },
        title = title,
        summary = label(selected),
        interactionSource = src
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            options.forEachIndexed { i, o ->
                val on = o == selected
                val press = remember { MutableInteractionSource() }
                ToggleButton(
                    checked = on,
                    onCheckedChange = {
                        if (!on) {
                            view.haptic(Haptic.Segment)
                            onSelect(o)
                        }
                    },
                    shapes = when (i) {
                        0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                        options.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                        else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                    },
                    interactionSource = press,
                    contentPadding = PaddingValues(horizontal = 14.dp),
                    modifier = Modifier.semantics { contentDescription = label(o) }
                ) {
                    Icon(optionIcon(o), contentDescription = null, modifier = Modifier.size(20.dp).iconMotion(press, IconMotion.Pop).popOnChange(on))
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun <T> StepBlock(
    title: String,
    steps: List<T>,
    selected: T,
    label: (T) -> String,
    tick: (T) -> String = label,
    enabled: Boolean = true,
    live: Boolean = false,
    onSelect: (T) -> Unit
) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val index = steps.indexOf(selected).coerceAtLeast(0)
    val latestSteps by rememberUpdatedState(steps)
    val latestSelected by rememberUpdatedState(selected)
    val latestSelect by rememberUpdatedState(onSelect)
    val state = remember(steps.size) {
        SliderState(index.toFloat(), (steps.size - 2).coerceAtLeast(0), 0f..(steps.size - 1).toFloat())
    }
    LaunchedEffect(index) { if (state.value.roundToInt() != index) state.value = index.toFloat() }
    val at = state.value.roundToInt().coerceIn(steps.indices)
    var heard by remember { mutableIntStateOf(at) }
    if (at != heard) {
        heard = at
        view.haptic(Haptic.Segment)
    }
    LaunchedEffect(at) { if (live && steps[at] != latestSelected) latestSelect(steps[at]) }
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    val tickStyle = MaterialTheme.typography.labelMedium
    val alpha by animateFloatAsState(if (enabled) 1f else 0.38f, Springs.effects(), label = "stepAlpha")
    Column(Modifier.fillMaxWidth().padding(bottom = 4.dp)) {
        BlockTitle(title, label(steps[at]))
        Slider(
            state = state,
            onValueChange = { state.value = it },
            onValueChangeFinished = {
                latestSteps[state.value.roundToInt().coerceIn(latestSteps.indices)].let { if (it != latestSelected) latestSelect(it) }
            },
            enabled = enabled,
            modifier = Modifier.alpha(alpha).fillMaxWidth().height(60.dp).semantics { stateDescription = label(steps[at]) },
            thumb = {
                Box(Modifier.size(16.dp, 60.dp)) {
                    Box(Modifier.align(Alignment.TopCenter).width(4.dp).height(36.dp).background(c.primary, RoundedCornerShape(2.dp)))
                }
            },
            track = { state ->
                Canvas(Modifier.fillMaxWidth().height(60.dp)) {
                    val h = 16.dp.toPx()
                    val mid = 18.dp.toPx()
                    val top = mid - h / 2
                    val w = size.width
                    val x = state.coercedValueAsFraction * w
                    val gap = 6.dp.toPx()
                    val half = 2.dp.toPx()
                    val small = CornerRadius(2.dp.toPx())
                    val round = CornerRadius(h / 2)
                    val activeEnd = x - half - gap
                    if (activeEnd > -h / 2) {
                        drawPath(Path().apply { addRoundRect(RoundRect(Rect(-h / 2, top, activeEnd, top + h), round, small, small, round)) }, c.primary)
                    }
                    val inactiveStart = x + half + gap
                    if (inactiveStart < w + h / 2) {
                        drawPath(Path().apply { addRoundRect(RoundRect(Rect(inactiveStart, top, w + h / 2, top + h), small, round, round, small)) }, c.secondaryContainer)
                    }
                    val n = steps.size
                    steps.forEachIndexed { i, step ->
                        val tx = if (n == 1) 0f else i.toFloat() / (n - 1) * w
                        when {
                            tx < x - half - gap -> drawCircle(c.onPrimary, 2.dp.toPx(), Offset(tx, mid))
                            tx > x + half + gap -> drawCircle(c.onSecondaryContainer.copy(alpha = 0.6f), 2.dp.toPx(), Offset(tx, mid))
                        }
                        val text = measurer.measure(tick(step), tickStyle)
                        val lx = (tx - text.size.width / 2f).coerceIn(-h / 2, w + h / 2 - text.size.width)
                        drawText(text, color = if (i == at) c.primary else c.onSurfaceVariant, topLeft = Offset(lx, mid + h / 2 + 12.dp.toPx()))
                    }
                }
            }
        )
    }
}
