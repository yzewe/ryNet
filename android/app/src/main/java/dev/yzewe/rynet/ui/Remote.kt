package dev.yzewe.rynet.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.unit.dp

private val EDIT_KEYS = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Modifier.remoteField(shape: Shape = MaterialTheme.shapes.medium): Modifier {
    val ctx = LocalContext.current
    val tv = remember(ctx) { isTv(ctx) }
    if (!tv) return this
    var editing by remember { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    val field = remember { FocusRequester() }
    val self = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val ime = WindowInsets.isImeVisible
    var shown by remember { mutableStateOf(false) }
    LaunchedEffect(ime, editing) {
        if (editing && ime) shown = true
        if (editing && shown && !ime) {
            shown = false
            editing = false
            self.requestFocus()
        }
    }
    val ring = if (focused && !editing) MaterialTheme.colorScheme.primary else Color.Transparent
    return this
        .focusRequester(self)
        .onFocusChanged { focused = it.isFocused }
        .border(3.dp, ring, shape)
        .onPreviewKeyEvent { e ->
            if (editing || e.key !in EDIT_KEYS) return@onPreviewKeyEvent false
            if (e.type == KeyEventType.KeyUp) {
                editing = true
                field.requestFocus()
                keyboard?.show()
            }
            true
        }
        .focusable()
        .focusProperties { canFocus = editing }
        .focusRequester(field)
        .onFocusChanged {
            if (editing && !it.hasFocus) {
                editing = false
                shown = false
            }
        }
}
