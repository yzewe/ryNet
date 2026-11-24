package dev.yzewe.rynet.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.TvLink
import dev.yzewe.rynet.tr

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun OnboardingScreen(
    initialUrl: String,
    loading: Boolean,
    error: String?,
    onBack: (() -> Unit)?,
    onFixHwid: (() -> Unit)? = null,
    onSettings: (() -> Unit)? = null,
    onTvOffer: (TvLink.Offer) -> Unit = {},
    tvReceive: (suspend (TvLink.Payload) -> String?)? = null,
    onSubmit: (String) -> Unit
) {
    var url by rememberSaveable { mutableStateOf(initialUrl) }
    val clipboard = LocalClipboardManager.current
    val fieldFocus = remember { FocusRequester() }
    val ctx = androidx.compose.ui.platform.LocalContext.current
    var scanError by rememberSaveable { mutableStateOf<String?>(null) }
    val submit = { if (!loading && url.isNotBlank()) onSubmit(url.trim()) }
    var scanning by rememberSaveable { mutableStateOf(false) }
    val scan = {
        scanError = null
        scanning = true
    }
    if (scanning) {
        QrScanDialog(
            onResult = { text ->
                scanning = false
                val tv = TvLink.parse(text)
                if (tv != null) {
                    onTvOffer(tv)
                } else {
                    when (val u = if (dev.yzewe.rynet.data.Local.isServers(text)) text.trim() else QrScanner.subUrl(text)) {
                        null -> scanError = tr("В QR-коде нет ссылки на подписку или сервер", "No subscription or server link in the QR code")

                        else -> {
                            url = u
                            if (!loading) onSubmit(u)
                        }
                    }
                }
            },
            onDismiss = { scanning = false }
        )
    }
    val bars = WindowInsets.systemBars.union(WindowInsets.displayCutout)
    val ime = WindowInsets.ime
    val minBottom = 16.dp + ButtonDefaults.MediumContainerHeight + 16.dp
    val top = if (onBack != null || onSettings != null) 8.dp + 48.dp + 16.dp else 24.dp
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface).windowInsetsPadding(bars)) {
        BoxWithConstraints(
            Modifier.fillMaxSize().layout { measurable, c ->
                val pad = maxOf(ime.getBottom(this) - bars.getBottom(this) + 16.dp.roundToPx(), minBottom.roundToPx()).coerceIn(0, c.maxHeight)
                val p = measurable.measure(c.copy(minHeight = (c.minHeight - pad).coerceAtLeast(0), maxHeight = c.maxHeight - pad))
                layout(p.width, p.height + pad) { p.place(0, 0) }
            }
        ) {
            Column(
                Modifier.verticalScroll(rememberScrollState()).fillMaxWidth().heightIn(min = maxHeight).padding(start = 16.dp, end = 16.dp, top = top),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                val title = if (onBack == null) tr("Добавьте подписку", "Add a subscription") else tr("Новая подписка", "New subscription")
                val field: @Composable () -> Unit = {
                    OutlinedTextField(
                        value = url,
                        onValueChange = {
                            url = it
                            scanError = null
                        },
                        label = { Text(tr("Подписка или сервер", "Subscription or server")) },
                        maxLines = 3,
                        isError = (scanError ?: error) != null,
                        supportingText = (scanError ?: error)?.let { { Text(it) } },
                        trailingIcon = {
                            IconButton(onClick = { clipboard.getText()?.text?.trim()?.takeIf { it.isNotEmpty() }?.let { url = it } }) {
                                Icon(Glyph.Paste, contentDescription = tr("Вставить", "Paste"))
                            }
                        },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                        keyboardActions = KeyboardActions(onGo = { submit() }),
                        shape = MaterialTheme.shapes.large,
                        modifier = Modifier.fillMaxWidth().focusRequester(fieldFocus).remoteField(MaterialTheme.shapes.large)
                    )
                    Reveal(onFixHwid != null && !loading) {
                        FilledTonalButton(
                            onClick = { onFixHwid?.invoke() },
                            shapes = ButtonDefaults.shapes(),
                            modifier = Modifier.padding(top = 8.dp)
                        ) { Text(tr("Включить HWID и повторить", "Turn on HWID and retry")) }
                    }
                }
                if (tvReceive != null) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f), contentAlignment = Alignment.Center) { TvReceivePanel(tvReceive) }
                        Spacer(Modifier.width(48.dp))
                        Column(Modifier.weight(1f)) {
                            Text(title, style = MaterialTheme.typography.displaySmallEmphasized)
                            Spacer(Modifier.height(24.dp))
                            field()
                        }
                    }
                } else {
                    LoadingLogo(MaterialShapes.Cookie9Sided, Modifier.size(136.dp))
                    Spacer(Modifier.height(32.dp))
                    Text(title, style = MaterialTheme.typography.displaySmallEmphasized, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                    Spacer(Modifier.height(32.dp))
                    field()
                    Spacer(Modifier.height(8.dp))
                    FilledTonalButton(
                        onClick = scan,
                        enabled = !loading,
                        shapes = ButtonDefaults.shapes(),
                        modifier = Modifier.fillMaxWidth().height(ButtonDefaults.MediumContainerHeight)
                    ) {
                        Icon(Glyph.QrScan, null, Modifier.size(22.dp))
                        Spacer(Modifier.size(10.dp))
                        Text(tr("Сканировать QR-код", "Scan QR code"), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        }
        if (onBack != null) {
            Box(Modifier.align(Alignment.TopStart).padding(start = 16.dp, top = 8.dp)) { CircleBack(onBack, Modifier.focusProperties { down = fieldFocus }) }
        }
        if (onSettings != null) {
            CircleIconButton(
                Icons.Rounded.Settings,
                tr("Настройки", "Settings"),
                onSettings,
                Modifier.align(Alignment.TopEnd).padding(end = 16.dp, top = 8.dp).focusProperties { down = fieldFocus }
            )
        }
        Button(
            onClick = submit,
            enabled = !loading && url.isNotBlank(),
            shapes = ButtonDefaults.shapes(),
            contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight),
            modifier = Modifier.align(Alignment.BottomCenter).padding(16.dp).fillMaxWidth().height(ButtonDefaults.MediumContainerHeight)
        ) {
            if (loading) {
                LoadingIndicator(Modifier.size(28.dp), color = MaterialTheme.colorScheme.onPrimary)
            } else {
                Text(tr("Продолжить", "Continue"), style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}
