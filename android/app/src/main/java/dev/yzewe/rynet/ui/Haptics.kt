package dev.yzewe.rynet.ui

import android.os.Build
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalView

enum class Haptic { Tap, Context, LongPress, ToggleOn, ToggleOff, Confirm, Reject, Segment, GestureEnd }

fun View.haptic(type: Haptic = Haptic.Tap) {
    val u = Build.VERSION.SDK_INT >= 34
    val r = Build.VERSION.SDK_INT >= 30
    val (primary, fallback) = when (type) {
        Haptic.Tap -> HapticFeedbackConstants.VIRTUAL_KEY to HapticFeedbackConstants.KEYBOARD_TAP
        Haptic.Context -> HapticFeedbackConstants.CONTEXT_CLICK to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.LongPress -> HapticFeedbackConstants.LONG_PRESS to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.ToggleOn -> (if (u) HapticFeedbackConstants.TOGGLE_ON else HapticFeedbackConstants.VIRTUAL_KEY) to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.ToggleOff -> (if (u) HapticFeedbackConstants.TOGGLE_OFF else HapticFeedbackConstants.VIRTUAL_KEY) to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.Confirm -> (if (r) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.VIRTUAL_KEY) to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.Reject -> (if (r) HapticFeedbackConstants.REJECT else HapticFeedbackConstants.LONG_PRESS) to HapticFeedbackConstants.VIRTUAL_KEY
        Haptic.Segment -> (if (u) HapticFeedbackConstants.SEGMENT_TICK else HapticFeedbackConstants.CLOCK_TICK) to HapticFeedbackConstants.CLOCK_TICK
        Haptic.GestureEnd -> (if (r) HapticFeedbackConstants.GESTURE_END else HapticFeedbackConstants.CLOCK_TICK) to HapticFeedbackConstants.CLOCK_TICK
    }
    if (!performHapticFeedback(primary) && fallback != primary) performHapticFeedback(fallback)
}

@Composable
fun hapticOnClick(type: Haptic = Haptic.Tap, onClick: () -> Unit): () -> Unit {
    val view = LocalView.current
    return {
        view.haptic(type)
        onClick()
    }
}

@Composable
fun hapticOnCheckedChange(onCheckedChange: (Boolean) -> Unit): (Boolean) -> Unit {
    val view = LocalView.current
    return { on ->
        view.haptic(if (on) Haptic.ToggleOn else Haptic.ToggleOff)
        onCheckedChange(on)
    }
}
