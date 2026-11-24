package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

class SlideItem(
    val id: String,
    val title: String,
    val subtitle: String? = null,
    val flag: String? = null,
    val trailing: String? = null,
    val selected: Boolean = false,
    val icon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    val danger: Boolean = false,
    val checked: Boolean? = null
) {
    internal fun flipped() = SlideItem(id, title, subtitle, flag, trailing, selected, icon, danger, checked?.not())
}

internal class SlideMenu(val items: List<SlideItem>, val anchor: Rect, val onPick: (SlideItem) -> Unit, val corner: Dp = 28.dp, val ring: Boolean = true)

@Stable
class MenuLayer {
    var state by mutableStateOf<SlideMenuState?>(null)
}

val LocalMenuLayer = staticCompositionLocalOf<MenuLayer?> { null }

@Composable
fun BoxScope.SlideMenuHere(state: SlideMenuState) {
    val layer = LocalMenuLayer.current
    if (layer == null) {
        SlideMenuHost(state)
        return
    }
    DisposableEffect(layer, state) {
        layer.state = state
        onDispose { if (layer.state === state) layer.state = null }
    }
}

@Stable
class SlideMenuState {
    private var menu by mutableStateOf<SlideMenu?>(null)
    private var last: SlideMenu? = null
    internal var hovered by mutableStateOf<Int?>(null)
    internal val bounds = mutableStateMapOf<Int, Rect>()
    val isOpen get() = menu != null

    internal fun open(items: List<SlideItem>, anchor: Rect, onPick: (SlideItem) -> Unit, corner: Dp = 28.dp, ring: Boolean = true) {
        bounds.clear()
        hovered = null
        menu = SlideMenu(items, anchor, onPick, corner, ring).also { last = it }
    }

    internal fun hit(p: Offset): Int? = bounds.entries.firstOrNull { it.value.contains(p) }?.key

    internal fun step(dir: Int) {
        val m = menu ?: return
        val order = bounds.entries.sortedBy { it.value.top }.map { it.key }
        if (order.isEmpty()) return
        val cur = order.indexOf(hovered)
        hovered = when {
            cur >= 0 -> order[(cur + dir).coerceIn(0, order.lastIndex)]
            bounds.getValue(order.first()).top < m.anchor.top -> order.last()
            else -> order.first()
        }
    }

    internal fun pick(i: Int) {
        val m = menu ?: return
        val item = m.items.getOrNull(i)
        if (item?.checked != null) {
            val next = item.flipped()
            hovered = null
            menu = SlideMenu(m.items.toMutableList().also { it[i] = next }, m.anchor, m.onPick, m.corner, m.ring).also { last = it }
            m.onPick(next)
            return
        }
        close()
        item?.let(m.onPick)
    }

    fun close() {
        menu = null
        hovered = null
    }

    internal val shown get() = menu
    internal val lastShown get() = last
}

fun Modifier.holdToSlide(
    state: SlideMenuState,
    enabled: Boolean = true,
    indicate: Boolean = true,
    items: () -> List<SlideItem>,
    onPick: (SlideItem) -> Unit,
    onTap: () -> Unit = {},
    tapOpens: Boolean = false,
    corner: Dp = 28.dp,
    ring: Boolean = true
): Modifier = composed {
    val items by androidx.compose.runtime.rememberUpdatedState(items)
    val onPick by androidx.compose.runtime.rememberUpdatedState(onPick)
    val onTap by androidx.compose.runtime.rememberUpdatedState(onTap)
    var anchor by remember { mutableStateOf(Rect.Zero) }
    val view = LocalView.current
    val scope = rememberCoroutineScope()
    val interaction = remember { MutableInteractionSource() }
    var held by remember { mutableStateOf(false) }
    this
        .onGloballyPositioned { anchor = it.boundsInRoot() }
        .then(if (indicate) Modifier.indication(interaction, ripple()) else Modifier)
        .onKeyEvent { e ->
            val down = e.type == KeyEventType.KeyDown
            if (state.isOpen) {
                when (e.key) {
                    Key.DirectionUp -> if (down) state.step(-1)
                    Key.DirectionDown -> if (down) state.step(1)
                    Key.DirectionCenter, Key.Enter, Key.NumPadEnter -> if (!down) {
                        if (held) held = false else state.hovered?.let { state.pick(it) } ?: state.close()
                    }
                    Key.Back, Key.Escape -> if (!down) state.close()
                    Key.DirectionLeft, Key.DirectionRight -> Unit
                    else -> return@onKeyEvent false
                }
                return@onKeyEvent true
            }
            if (e.key != Key.DirectionCenter && e.key != Key.Enter && e.key != Key.NumPadEnter) return@onKeyEvent false
            if (down) {
                if (e.nativeKeyEvent.repeatCount == 1 && enabled) {
                    held = true
                    view.haptic(Haptic.LongPress)
                    state.open(items(), anchor, onPick, corner, ring)
                }
            } else if (!held) {
                if (tapOpens && enabled) state.open(items(), anchor, onPick, corner, ring) else onTap()
            }
            true
        }
        .focusable(interactionSource = interaction)
        .pointerInput(enabled) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                val press = PressInteraction.Press(down.position)
                scope.launch { interaction.emit(press) }
                val long = if (enabled) awaitLongPressOrCancellation(down.id) else null
                if (long == null) {
                    val up = currentEvent.changes.firstOrNull { it.id == down.id }
                    val tapped = up != null && !up.pressed && !up.isConsumed &&
                        up.position.x in 0f..size.width.toFloat() && up.position.y in 0f..size.height.toFloat()
                    if (!enabled) {
                        while (currentEvent.changes.any { it.pressed }) awaitPointerEvent()
                        val end = currentEvent.changes.firstOrNull { it.id == down.id }
                        scope.launch { interaction.emit(PressInteraction.Release(press)) }
                        if (end != null && !end.isConsumed && end.position.x in 0f..size.width.toFloat() && end.position.y in 0f..size.height.toFloat()) onTap()
                        return@awaitEachGesture
                    }
                    scope.launch { interaction.emit(if (tapped) PressInteraction.Release(press) else PressInteraction.Cancel(press)) }
                    if (tapped && tapOpens) {
                        view.haptic(Haptic.Tap)
                        state.open(items(), anchor, onPick, corner, ring)
                    } else if (tapped) {
                        onTap()
                    }
                    return@awaitEachGesture
                }
                scope.launch { interaction.emit(PressInteraction.Release(press)) }
                view.haptic(Haptic.LongPress)
                state.open(items(), anchor, onPick, corner, ring)
                val origin = long.position
                var moved = false
                long.consume()
                while (true) {
                    val event = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    change.consume()
                    if ((change.position - origin).getDistance() > viewConfiguration.touchSlop) moved = true
                    val at = state.hit(change.position + anchor.topLeft)
                    if (at != state.hovered) {
                        state.hovered = at
                        if (at != null) view.haptic(Haptic.Segment)
                    }
                    if (!change.pressed) break
                }
                val at = state.hovered
                when {
                    at != null -> {
                        view.haptic(state.shown?.items?.getOrNull(at)?.checked?.let { if (it) Haptic.ToggleOff else Haptic.ToggleOn } ?: Haptic.Confirm)
                        state.pick(at)
                    }

                    moved -> state.close()
                }
            }
        }
}

@Composable
fun BoxScope.SlideMenuHost(state: SlideMenuState) {
    var origin by remember { mutableStateOf(Offset.Zero) }
    var hostH by remember { mutableStateOf(0f) }
    val density = LocalDensity.current
    val c = MaterialTheme.colorScheme
    val scrim by animateFloatAsState(if (state.isOpen) 0.32f else 0f, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "scrim")
    val m = state.shown ?: state.lastShown
    Box(
        Modifier.matchParentSize().onGloballyPositioned {
            origin = it.positionInRoot()
            hostH = it.size.height.toFloat()
        }
            .then(if (state.isOpen) Modifier.clickable(interactionSource = null, indication = null) { state.close() } else Modifier)
            .drawBehind {
                if (scrim <= 0f) return@drawBehind
                if (m == null) return@drawBehind drawRect(c.scrim.copy(alpha = scrim))
                val ring = if (m.ring) 2.dp.toPx() else 0f
                val hole = Path().apply {
                    addRoundRect(RoundRect(m.anchor.translate(-origin).inflate(ring / 2), CornerRadius(m.corner.toPx() + ring / 2)))
                }
                clipPath(hole, ClipOp.Difference) { drawRect(c.scrim.copy(alpha = scrim)) }
                if (m.ring) drawPath(hole, c.primary.copy(alpha = scrim / 0.32f), style = Stroke(ring))
            }
    )
    m ?: return
    val above = m.anchor.center.y - origin.y > hostH / 2
    val gap = with(density) { 8.dp.toPx() }
    AnimatedVisibility(
        state.isOpen,
        enter = fadeIn(MaterialTheme.motionScheme.fastEffectsSpec()) + scaleIn(MaterialTheme.motionScheme.fastSpatialSpec(), 0.86f, TransformOrigin(0.5f, if (above) 1f else 0f)),
        exit = fadeOut(MaterialTheme.motionScheme.fastEffectsSpec()) + scaleOut(MaterialTheme.motionScheme.fastSpatialSpec(), 0.92f, TransformOrigin(0.5f, if (above) 1f else 0f)),
        modifier = Modifier
            .layoutAt(above, m.anchor, origin, gap)
            .width(with(density) { m.anchor.width.toDp() }.coerceAtLeast(260.dp))
    ) {
        Column(
            Modifier.clip(RoundedCornerShape(Segments.Outer + 6.dp)).background(c.surfaceContainerHigh).padding(6.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp)
        ) {
            val order = if (above) m.items.indices.reversed() else m.items.indices
            order.forEachIndexed { k, i -> SlideRow(state, i, m.items[i], Segments.shape(k, m.items.size)) }
        }
    }
}

private fun Modifier.layoutAt(above: Boolean, anchor: Rect, origin: Offset, gap: Float): Modifier = this.then(
    Modifier.layout { measurable, constraints ->
        val p = measurable.measure(constraints.copy(minWidth = 0, minHeight = 0))
        layout(constraints.maxWidth, constraints.maxHeight) {
            val x = (anchor.left - origin.x).roundToInt().coerceIn(0, (constraints.maxWidth - p.width).coerceAtLeast(0))
            val y = if (above) (anchor.top - origin.y - gap - p.height).roundToInt() else (anchor.bottom - origin.y + gap).roundToInt()
            p.place(x, y.coerceIn(0, (constraints.maxHeight - p.height).coerceAtLeast(0)))
        }
    }
)

@Composable
private fun SlideRow(state: SlideMenuState, i: Int, item: SlideItem, shape: androidx.compose.ui.graphics.Shape) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val hot = state.hovered == i
    val bg by animateColorAsState(
        if (hot) {
            c.secondaryContainer
        } else if (item.selected) {
            c.surfaceContainerHighest
        } else {
            c.surfaceContainer
        },
        MaterialTheme.motionScheme.fastEffectsSpec(),
        label = "row"
    )
    val grow by animateFloatAsState(if (hot) 0.98f else 1f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "grow")
    Row(
        Modifier.fillMaxWidth().scale(grow).heightIn(min = 56.dp).clip(shape).background(bg)
            .onGloballyPositioned { state.bounds[i] = it.boundsInRoot() }
            .clickable {
                item.checked?.let { view.haptic(if (it) Haptic.ToggleOff else Haptic.ToggleOn) }
                state.pick(i)
            }
            .then(if (item.checked != null) Modifier.semantics { role = Role.Switch } else Modifier)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically
    ) {
        if (item.flag != null) {
            FlagIcon(item.flag, Modifier.size(width = 40.dp, height = 30.dp), MaterialTheme.shapes.small)
            Spacer(Modifier.width(12.dp))
        }
        if (item.icon != null) {
            Icon(
                item.icon,
                null,
                tint = if (item.danger) {
                    c.error
                } else if (hot) {
                    c.onSecondaryContainer
                } else {
                    c.onSurfaceVariant
                },
                modifier = Modifier.padding(start = 4.dp).size(24.dp)
            )
            Spacer(Modifier.width(16.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(
                item.title,
                style = MaterialTheme.typography.titleSmall,
                color = if (item.danger) {
                    c.error
                } else if (hot) {
                    c.onSecondaryContainer
                } else {
                    c.onSurface
                },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            item.subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        item.trailing?.let {
            Spacer(Modifier.width(8.dp))
            Text(it, style = MaterialTheme.typography.labelLarge, color = c.primary)
        }
        if (item.selected) {
            Spacer(Modifier.width(8.dp))
            Icon(Icons.Rounded.Check, null, tint = c.primary, modifier = Modifier.size(20.dp))
        }
        item.checked?.let {
            Spacer(Modifier.width(12.dp))
            Switch(checked = it, onCheckedChange = null)
        }
    }
}
