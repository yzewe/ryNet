@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.toSize
import kotlinx.coroutines.launch

@Stable
class HoldPick {
    internal val bounds = mutableStateMapOf<Int, Rect>()
    internal val run = HashMap<Int, () -> Unit>()
    var hovered by mutableStateOf<Int?>(null)
        internal set
}

internal fun Modifier.holdMenu(
    pick: HoldPick,
    onTap: (() -> Unit)? = null,
    interaction: MutableInteractionSource? = null,
    onLong: (Offset) -> Unit
): Modifier = composed {
    val tap by rememberUpdatedState(onTap)
    val long by rememberUpdatedState(onLong)
    val view = LocalView.current
    val scope = rememberCoroutineScope()
    var coords by remember { mutableStateOf<LayoutCoordinates?>(null) }
    onGloballyPositioned { coords = it }.pointerInput(pick) {
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false)
            val press = PressInteraction.Press(down.position)
            if (interaction != null) scope.launch { interaction.emit(press) }
            val held = awaitLongPressOrCancellation(down.id)
            if (held == null) {
                val up = currentEvent.changes.firstOrNull { it.id == down.id }
                val tapped = up != null && !up.pressed && !up.isConsumed &&
                    up.position.x in 0f..size.width.toFloat() && up.position.y in 0f..size.height.toFloat()
                if (interaction != null) scope.launch { interaction.emit(if (tapped) PressInteraction.Release(press) else PressInteraction.Cancel(press)) }
                if (tapped) {
                    tap?.let {
                        view.haptic(Haptic.Segment)
                        it()
                    }
                }
                return@awaitEachGesture
            }
            if (interaction != null) scope.launch { interaction.emit(PressInteraction.Release(press)) }
            held.consume()
            view.haptic(Haptic.LongPress)
            pick.hovered = null
            long(held.position)
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                change.consume()
                val screen = coords?.takeIf { it.isAttached }?.localToScreen(change.position)
                val at = screen?.let { p -> pick.bounds.entries.firstOrNull { it.value.contains(p) }?.key }
                if (at != pick.hovered) {
                    pick.hovered = at
                    if (at != null) view.haptic(Haptic.Segment)
                }
                if (!change.pressed) break
            }
            val at = pick.hovered
            pick.hovered = null
            if (at != null) {
                view.haptic(Haptic.Confirm)
                pick.run[at]?.invoke()
            }
        }
    }
}

@Composable
internal fun HoldItem(
    pick: HoldPick,
    index: Int,
    text: String,
    icon: @Composable () -> Unit,
    shape: Shape,
    danger: Boolean = false,
    onClick: () -> Unit
) {
    val c = MaterialTheme.colorScheme
    pick.run[index] = onClick
    DisposableEffect(pick, index) {
        onDispose {
            pick.bounds.remove(index)
            pick.run.remove(index)
        }
    }
    val hot = pick.hovered == index
    DropdownMenuItem(
        onClick = onClick,
        text = { Text(text, color = if (danger) c.error else androidx.compose.ui.graphics.Color.Unspecified) },
        shape = shape,
        leadingIcon = icon,
        modifier = Modifier
            .onGloballyPositioned {
                val p = it.localToScreen(Offset.Zero)
                pick.bounds[index] = Rect(p, it.size.toSize())
            }
            .clip(shape)
            .background(if (hot) c.onSurface.copy(alpha = 0.10f) else androidx.compose.ui.graphics.Color.Transparent)
    )
}

@Composable
internal fun HoldIcon(icon: ImageVector, danger: Boolean = false) = Icon(icon, null, tint = if (danger) MaterialTheme.colorScheme.error else androidx.compose.material3.LocalContentColor.current)
