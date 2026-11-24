package dev.yzewe.rynet.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.abs

@Composable
fun <T> Choice(
    options: List<T>,
    selected: T,
    label: (T) -> String,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true
) {
    if (options.isEmpty()) return
    val c = MaterialTheme.colorScheme
    val n = options.size
    val selectedIndex = options.indexOf(selected).coerceAtLeast(0)
    var active by remember { mutableIntStateOf(selectedIndex) }
    LaunchedEffect(selectedIndex) { active = selectedIndex }
    var pressed by remember { mutableStateOf(false) }
    var rowWidth by remember { mutableIntStateOf(0) }
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    val density = androidx.compose.ui.platform.LocalDensity.current
    val labelStyle = MaterialTheme.typography.labelLarge
    val pad = with(density) { 20.dp.toPx() }
    val weights = options.map { measurer.measure(label(it), labelStyle, maxLines = 1).size.width + pad }
    val total = weights.sum()
    val latestOptions by rememberUpdatedState(options)
    val latestSelect by rememberUpdatedState(onSelect)
    val latestWeights by rememberUpdatedState(weights)
    val view = LocalView.current

    Box(modifier.fillMaxWidth().clip(CircleShape)) {
        Row(
            Modifier
                .fillMaxWidth()
                .onSizeChanged { rowWidth = it.width }
                .pointerInput(n, enabled) {
                    if (!enabled) return@pointerInput
                    fun indexAt(x: Float): Int {
                        if (rowWidth <= 0) return active
                        val w = latestWeights
                        val sum = w.sum()
                        var edge = 0f
                        w.forEachIndexed { i, wi ->
                            edge += wi / sum * rowWidth
                            if (x < edge) return i
                        }
                        return w.lastIndex
                    }
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        val slop = viewConfiguration.touchSlop
                        val start = active
                        var horizontal = false
                        var cancelled = false
                        pressed = true
                        active = indexAt(down.position.x)
                        view.haptic(Haptic.Segment)
                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            val d = change.position - down.position
                            if (!horizontal && (change.isConsumed || (abs(d.y) > slop && abs(d.y) > abs(d.x)))) {
                                cancelled = true
                                break
                            }
                            if (abs(d.x) > slop) horizontal = true
                            val next = indexAt(change.position.x)
                            if (next != active) {
                                active = next
                                view.haptic(Haptic.Segment)
                            }
                            if (!change.pressed) break
                            if (horizontal) change.consume()
                        }
                        pressed = false
                        if (cancelled) {
                            active = start
                        } else if (active in latestOptions.indices) {
                            latestSelect(latestOptions[active])
                        }
                    }
                },
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            options.forEachIndexed { i, o ->
                val src = remember { MutableInteractionSource() }
                val isActive = i == active
                val grow by animateFloatAsState(
                    if (pressed && isActive) 1.35f else 1f,
                    tween(200, easing = FastOutSlowInEasing),
                    label = "chipWeight"
                )
                val bg by animateColorAsState(
                    if (isActive) c.primary else c.surfaceContainerHighest,
                    tween(220, easing = FastOutSlowInEasing),
                    label = "chipBg"
                )
                val fg by animateColorAsState(
                    if (isActive) c.onPrimary else c.onSurfaceVariant,
                    tween(220, easing = FastOutSlowInEasing),
                    label = "chipFg"
                )
                val corner by animateDpAsState(
                    if (isActive) 100.dp else 8.dp,
                    tween(220, easing = FastOutSlowInEasing),
                    label = "chipCorner"
                )
                Box(
                    Modifier
                        .weight(weights[i] / total * n * grow)
                        .clip(RoundedCornerShape(corner))
                        .background(bg)
                        .remoteFocus(src, RoundedCornerShape(corner), inward = true)
                        .onKeyEvent { event ->
                            if (event.key != Key.DirectionCenter && event.key != Key.Enter && event.key != Key.NumPadEnter && event.key != Key.Spacebar) {
                                false
                            } else {
                                if (event.type == KeyEventType.KeyUp) {
                                    view.haptic(Haptic.Segment)
                                    latestSelect(o)
                                }
                                true
                            }
                        }
                        .focusable(enabled, interactionSource = src)
                        .padding(horizontal = 8.dp, vertical = 12.dp)
                        .semantics {
                            role = Role.RadioButton
                            this.selected = o == selected
                            if (enabled) {
                                onClick {
                                    latestSelect(o)
                                    true
                                }
                            }
                        },
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        label(o),
                        color = fg,
                        style = MaterialTheme.typography.labelLarge,
                        maxLines = 1,
                        softWrap = false,
                        autoSize = androidx.compose.foundation.text.TextAutoSize.StepBased(minFontSize = 10.sp, maxFontSize = 14.sp, stepSize = 0.5.sp)
                    )
                }
            }
        }
    }
}

@Composable
fun SmallChip(text: String, selected: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val bg by animateColorAsState(if (selected) c.primary else c.surfaceContainerHigh, label = "smallChipBg")
    val fg by animateColorAsState(if (selected) c.onPrimary else c.onSurfaceVariant, label = "smallChipFg")
    Row(
        modifier
            .heightIn(min = 32.dp)
            .clip(MaterialTheme.shapes.small)
            .background(bg)
            .clickable {
                view.haptic(Haptic.Segment)
                onClick()
            }
            .padding(horizontal = 12.dp)
            .semantics { this.selected = selected },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center
    ) {
        Text(text, color = fg, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, softWrap = false)
    }
}
