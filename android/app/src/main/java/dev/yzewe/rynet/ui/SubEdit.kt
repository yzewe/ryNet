@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.hostOf
import dev.yzewe.rynet.tr

val LocalSubEdit = staticCompositionLocalOf<((Sub) -> Unit)?> { null }

@Composable
fun SubEditDialog(
    sub: Sub,
    balancers: Boolean,
    split: Boolean,
    onDismiss: () -> Unit,
    onSave: (name: String?, url: String, split: Boolean, done: (String?) -> Unit) -> Unit
) {
    val host = hostOf(sub.info.url)
    val initialName = sub.name ?: sub.panelTitle.orEmpty()
    var name by rememberSaveable { mutableStateOf(initialName) }
    var url by rememberSaveable { mutableStateOf(sub.info.url) }
    var busy by rememberSaveable { mutableStateOf(false) }
    var failed by rememberSaveable { mutableStateOf<String?>(null) }
    var splitOn by rememberSaveable { mutableStateOf(split) }
    val view = LocalView.current
    val valid = Subs.validUrl(url)
    val nameFocus = remember { FocusRequester() }
    val urlFocus = remember { FocusRequester() }

    fun save() {
        if (!valid || busy) return
        val typed = name.trim()
        val resolved = when {
            typed == initialName.trim() -> sub.name
            typed.isEmpty() || typed == sub.panelTitle -> null
            else -> typed
        }
        if (resolved == sub.name && Subs.sameUrl(Subs.withScheme(url), sub.info.url) && splitOn == split) return onDismiss()
        busy = true
        failed = null
        onSave(resolved, url.trim(), splitOn) { error ->
            busy = false
            failed = error
            view.haptic(if (error == null) Haptic.Confirm else Haptic.Reject)
            if (error != null) view.post { runCatching { urlFocus.requestFocus() } }
        }
    }

    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        properties = DialogProperties(dismissOnClickOutside = !busy, dismissOnBackPress = !busy),
        icon = { Icon(Icons.Rounded.Edit, contentDescription = null) },
        title = { Text(tr("Подписка", "Subscription")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text(tr("Название", "Name")) },
                    placeholder = { Text(sub.panelTitle ?: host) },
                    singleLine = true,
                    enabled = !busy,
                    shape = MaterialTheme.shapes.large,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    trailingIcon = name.takeIf { it.isNotEmpty() }?.let {
                        {
                            IconButton(onClick = {
                                name = ""
                                runCatching { nameFocus.requestFocus() }
                            }, enabled = !busy) { Icon(Icons.Rounded.Close, tr("Очистить", "Clear")) }
                        }
                    },
                    modifier = Modifier.fillMaxWidth().focusRequester(nameFocus).remoteField(MaterialTheme.shapes.large)
                )
                OutlinedTextField(
                    value = url,
                    onValueChange = {
                        url = it
                        failed = null
                    },
                    label = { Text(tr("Ссылка", "Link")) },
                    maxLines = 4,
                    enabled = !busy,
                    isError = !valid || failed != null,
                    supportingText = (failed ?: tr("Нужна ссылка http:// или https://", "Needs an http:// or https:// link").takeIf { !valid })?.let { t -> { Text(t) } },
                    shape = MaterialTheme.shapes.large,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                    keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { save() }),
                    modifier = Modifier.fillMaxWidth().focusRequester(urlFocus).remoteField(MaterialTheme.shapes.large)
                )
                if (balancers) {
                    Row(
                        Modifier.fillMaxWidth().clip(MaterialTheme.shapes.large).toggleable(splitOn, enabled = !busy, role = Role.Switch) { splitOn = it }
                            .padding(vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(tr("Разобрать балансеры", "Split balancers"), style = MaterialTheme.typography.bodyLarge)
                            Text(
                                tr("Каждый сервер балансера — отдельной строкой", "Each balancer server on its own row"),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                        Spacer(Modifier.width(12.dp))
                        Switch(checked = splitOn, onCheckedChange = null, enabled = !busy)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = ::save, enabled = valid && !busy) {
                AnimatedContent(busy, transitionSpec = { fadeIn() togetherWith fadeOut() }, contentAlignment = Alignment.Center, label = "save") { b ->
                    if (b) LoadingIndicator(Modifier.size(24.dp)) else Text(tr("Сохранить", "Save"))
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text(tr("Отмена", "Cancel")) } }
    )
}
