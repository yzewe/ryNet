package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Modifier
import dev.yzewe.rynet.data.Settings

class Notices(private val dismissed: List<String>, private val onDismiss: (String) -> Unit) {
    fun hidden(scope: String, text: String) = Settings.noticeKey(scope, text) in dismissed

    fun dismiss(scope: String, text: String) = onDismiss(Settings.noticeKey(scope, text))
}

val LocalNotices = compositionLocalOf<Notices?> { null }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SwipeAway(scope: String, text: String, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val notices = LocalNotices.current
    if (notices == null) {
        content()
        return
    }
    val state = rememberSwipeToDismissBoxState()
    LaunchedEffect(state.currentValue) {
        if (state.currentValue != SwipeToDismissBoxValue.Settled) notices.dismiss(scope, text)
    }
    AnimatedVisibility(!notices.hidden(scope, text), modifier, exit = shrinkVertically(spring(1f, 500f)) + fadeOut()) {
        SwipeToDismissBox(state, backgroundContent = {}) { content() }
    }
}
