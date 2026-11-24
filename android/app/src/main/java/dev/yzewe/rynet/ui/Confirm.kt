package dev.yzewe.rynet.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Endpoints
import dev.yzewe.rynet.data.hostOf
import dev.yzewe.rynet.tr

@Composable
fun DeleteDialog(title: String, text: String, confirmText: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Rounded.Delete, contentDescription = null) },
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm()
            }) {
                Text(confirmText, color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

@Composable
fun RenameDialog(current: String, onConfirm: (String) -> Unit, onDismiss: () -> Unit) {
    var name by androidx.compose.runtime.saveable.rememberSaveable { androidx.compose.runtime.mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("Название", "Name")) },
        text = {
            androidx.compose.material3.OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.remoteField(MaterialTheme.shapes.large)
            )
        },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm(name.trim())
            }, enabled = name.isNotBlank()) { Text(tr("Готово", "Done")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

@Composable
fun ServerDnsDialog(name: String, current: String, onConfirm: (String) -> Unit, onDismiss: () -> Unit) {
    val presets = DNS_PRESETS + ("Google" to Endpoints.DNS_GOOGLE)
    var custom by androidx.compose.runtime.saveable.rememberSaveable { androidx.compose.runtime.mutableStateOf(current.isNotBlank() && presets.none { it.second == current }) }
    var choice by androidx.compose.runtime.saveable.rememberSaveable { androidx.compose.runtime.mutableStateOf(current) }
    var text by androidx.compose.runtime.saveable.rememberSaveable { androidx.compose.runtime.mutableStateOf(if (custom) current else "") }
    val valid = text.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }.let { it.isNotEmpty() && it.all { a -> DNS_ADDRESS.matches(a) } }
    val options = listOf("" to tr("Как в настройках", "As in settings")) + presets.map { it.second to it.first }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Glyph.Globe, contentDescription = null) },
        title = { Text(tr("DNS для «$name»", "DNS for \"$name\"")) },
        text = {
            androidx.compose.foundation.layout.Column(Modifier.selectableGroup()) {
                val focus = androidx.compose.runtime.remember { androidx.compose.ui.focus.FocusRequester() }
                val focusManager = androidx.compose.ui.platform.LocalFocusManager.current
                for ((value, label) in options) {
                    DnsChoice(label, !custom && choice == value) {
                        custom = false
                        choice = value
                        focusManager.clearFocus()
                    }
                }
                DnsChoice(tr("Свой", "Custom"), custom) {
                    custom = true
                    focus.requestFocus()
                }
                val dim by androidx.compose.animation.core.animateFloatAsState(if (custom) 1f else 0.38f, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "dns field")
                androidx.compose.material3.OutlinedTextField(
                    value = text,
                    onValueChange = {
                        text = it
                        custom = true
                    },
                    placeholder = { Text("https://dns.example/dns-query") },
                    isError = custom && text.isNotBlank() && !valid,
                    maxLines = 3,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp).alpha(dim).focusRequester(focus)
                        .onFocusChanged { if (it.isFocused) custom = true }.remoteField(MaterialTheme.shapes.large)
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm(if (custom) text.trim() else choice)
            }, enabled = !custom || valid) { Text(tr("Готово", "Done")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

@Composable
private fun DnsChoice(label: String, selected: Boolean, onClick: () -> Unit) {
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().clip(MaterialTheme.shapes.large)
            .selectable(selected, onClick = onClick, role = androidx.compose.ui.semantics.Role.RadioButton)
            .padding(vertical = 4.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically
    ) {
        androidx.compose.material3.RadioButton(selected, onClick = null)
        androidx.compose.foundation.layout.Spacer(Modifier.width(8.dp))
        Text(label, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
fun SendLogsDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Glyph.Upload, contentDescription = null) },
        title = { Text(tr("Отправлять логи разработчику?", "Send logs to the developer?")) },
        text = {
            val link = TextLinkStyles(
                SpanStyle(color = MaterialTheme.colorScheme.primary, textDecoration = TextDecoration.Underline)
            )
            Text(
                buildAnnotatedString {
                    append(
                        tr(
                            "Включится подробный журнал, и каждые 2 часа работы VPN он будет уходить на api.rynet.app. В нём адреса сайтов и серверов, к которым шли подключения, модель телефона и версия Android. Ключей и ссылок подписок там нет.",
                            "The verbose log turns on and is sent to api.rynet.app every 2 hours of VPN use. It holds the addresses of sites and servers that were connected to, the phone model and Android version. No keys or subscription links."
                        )
                    )
                    append("\n\n")
                    withLink(LinkAnnotation.Url(Endpoints.PRIVACY, link)) {
                        append(tr("Политика конфиденциальности", "Privacy policy"))
                    }
                }
            )
        },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm()
            }) { Text(tr("Включить", "Turn on")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

fun dev.yzewe.rynet.data.Settings.sendingLogs(on: Boolean) = copy(sendLogs = on, debugLog = on)

@Composable
fun AddServersDialog(text: String, switches: Boolean, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    val found by androidx.compose.runtime.produceState<List<String>?>(null, text) {
        value = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { dev.yzewe.rynet.data.Local.preview(text) }
    }
    val list = found
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Glyph.Link, contentDescription = null) },
        title = { Text(if ((list?.size ?: 1) == 1) tr("Добавить сервер?", "Add server?") else tr("Добавить серверы?", "Add servers?")) },
        text = {
            Text(
                when {
                    list == null -> "…"

                    list.isEmpty() -> tr("Не похоже на ссылку на сервер или конфиг", "Doesn't look like a server link or config")

                    else -> (
                        list.take(5) +
                            listOfNotNull(
                                (list.size - 5).takeIf { it > 0 }?.let { tr("и ещё $it", "and $it more") },
                                tr("VPN переключится на добавленный сервер", "The VPN will switch to the added server").takeIf { switches }
                            )
                        ).joinToString("\n")
                }
            )
        },
        confirmButton = { if (!list.isNullOrEmpty()) TextButton(onClick = onConfirm) { Text(tr("Добавить", "Add")) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}

@Composable
fun AddLinkDialog(link: dev.yzewe.rynet.data.DeepLink.Target.Subscription, title: String?, exists: Boolean, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    val host = hostOf(link.url)
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Glyph.Link, contentDescription = null) },
        title = { Text(if (exists) tr("Подписка уже добавлена", "Subscription already added") else tr("Добавить подписку?", "Add subscription?")) },
        text = { Text(listOfNotNull(title ?: link.name, host).distinct().joinToString(" · ")) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(if (exists) tr("Обновить", "Update") else tr("Добавить", "Add")) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Отмена", "Cancel")) } }
    )
}
