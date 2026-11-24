@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.tr
import kotlinx.coroutines.delay

class Snackbars {
    internal var current by mutableStateOf<Msg?>(null)

    internal data class Msg(val title: String, val body: String, val copy: Boolean = true, val id: Long = System.nanoTime())

    fun error(body: String) = show(tr("Ошибка", "Error"), body)
    fun notice(body: String) {
        current = Msg(tr("Готово", "Done"), body, copy = false)
    }
    fun show(title: String, body: String) {
        current = Msg(title, body)
    }
}

val LocalSnackbars = staticCompositionLocalOf { Snackbars() }

@Composable
fun BoxScope.SnackbarHost(s: Snackbars) {
    val msg = s.current
    var visible by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf<Snackbars.Msg?>(null) }
    val ctx = LocalContext.current
    LaunchedEffect(msg?.id) {
        if (msg == null) return@LaunchedEffect
        if (visible) {
            visible = false
            delay(EXIT.toLong())
        }
        shown = msg
        visible = true
        delay(6_000)
        visible = false
        delay(EXIT.toLong())
        if (s.current?.id == msg.id) s.current = null
    }
    val motion = MaterialTheme.motionScheme
    AnimatedVisibility(
        visible = visible,
        modifier = Modifier.align(Alignment.BottomCenter),
        enter = slideInVertically(motion.defaultSpatialSpec()) { it } + fadeIn(motion.defaultEffectsSpec()),
        exit = slideOutVertically(motion.fastSpatialSpec()) { it } + fadeOut(motion.fastEffectsSpec())
    ) {
        val m = shown ?: return@AnimatedVisibility
        Snackbar(
            modifier = Modifier
                .navigationBarsPadding()
                .padding(12.dp)
                .clickable(remember { MutableInteractionSource() }, null) { visible = false },
            action = if (m.copy) {
                (
                    {
                        TextButton(
                            onClick = hapticOnClick {
                                copy(ctx, m.body)
                                visible = false
                            },
                            colors = ButtonDefaults.textButtonColors(contentColor = SnackbarDefaults.actionColor)
                        ) { Text(tr("Скопировать", "Copy")) }
                    }
                    )
            } else {
                null
            },
            actionOnNewLine = m.copy && m.body.length > 60
        ) { Text(m.body) }
    }
}

private const val EXIT = 200

private fun copy(ctx: Context, text: String) {
    (ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager)?.setPrimaryClip(ClipData.newPlainText("ryNet", text))
}
