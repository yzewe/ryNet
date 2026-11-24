package dev.yzewe.rynet.ui

import android.content.Context
import android.content.pm.PackageManager
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Hwid
import dev.yzewe.rynet.data.Local
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.TvLink
import dev.yzewe.rynet.data.hostOf
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

fun isTv(ctx: Context): Boolean = ctx.packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK)

private enum class Receive { Waiting, Busy, NoNetwork }

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun TvReceivePanel(onPayload: suspend (TvLink.Payload) -> String?) {
    val c = MaterialTheme.colorScheme
    val scope = rememberCoroutineScope()
    var offer by remember { mutableStateOf<TvLink.Offer?>(null) }
    var state by remember { mutableStateOf(Receive.Waiting) }
    val latest by rememberUpdatedState(onPayload)
    val ctx = LocalContext.current
    DisposableEffect(Unit) {
        val server = TvLink.Server { p ->
            withContext(Dispatchers.Main) { state = Receive.Busy }
            val err = withContext(Dispatchers.Main) { latest(p) }
            withContext(Dispatchers.Main) { state = Receive.Waiting }
            err
        }
        val job = scope.launch {
            offer = withContext(Dispatchers.IO) {
                val (host, port) = TvLink.debugEndpoint(ctx)
                runCatching { server.open(host ?: TvLink.lanAddress(), port) }.getOrNull()
            }
            if (offer == null) state = Receive.NoNetwork else server.serve()
        }
        onDispose {
            server.close()
            job.cancel()
        }
    }
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        AnimatedContent(offer, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "tvQr") { o ->
            if (o != null) {
                QrCode(o.url, Modifier.size(220.dp))
            } else {
                Spacer(Modifier.size(220.dp))
            }
        }
        AnimatedContent(state, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "tvState") { st ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (st == Receive.Busy) {
                    LoadingIndicator(Modifier.size(24.dp))
                    Spacer(Modifier.width(8.dp))
                }
                Text(
                    when (st) {
                        Receive.Waiting -> tr("Отсканируйте камерой телефона", "Scan with your phone camera")
                        Receive.Busy -> tr("Получаем подписки…", "Receiving subscriptions…")
                        Receive.NoNetwork -> tr("Телевизор не в локальной сети", "The TV is not on a local network")
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = if (st == Receive.NoNetwork) c.error else c.onSurface,
                    textAlign = TextAlign.Center
                )
            }
        }
        Text(
            tr(
                "Телефон и телевизор должны быть в одной сети Wi-Fi. На телефоне откроется ryNet: выберите, какие подписки передать.",
                "The phone and the TV must be on the same Wi-Fi. ryNet opens on the phone: pick the subscriptions to send."
            ),
            style = PageType.rowSummary,
            color = c.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 8.dp)
        )
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun TvSendSheet(offer: TvLink.Offer, subs: List<Sub>, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    val snackbars = LocalSnackbars.current
    val scope = rememberCoroutineScope()
    var picked by rememberSaveable { mutableStateOf(subs.firstOrNull()?.id?.let { setOf(it) } ?: emptySet()) }
    var extra by rememberSaveable { mutableStateOf("") }
    var syncHwid by rememberSaveable { mutableStateOf(true) }
    var sending by remember { mutableStateOf(false) }
    var dismiss by remember { mutableStateOf(0L) }
    val newUrl = extra.trim().takeIf { it.startsWith("http://", true) || it.startsWith("https://", true) }
    val ready = !sending && (picked.isNotEmpty() || newUrl != null)
    val send = {
        sending = true
        scope.launch {
            val payload = withContext(Dispatchers.IO) {
                val chosen = subs.filter { it.id in picked }
                TvLink.Payload(
                    subs = chosen.filterNot(Local::isLocal).map { it.info.url } + listOfNotNull(newUrl),
                    servers = chosen.firstOrNull(Local::isLocal)?.let { Subs.body(ctx, it.id) },
                    hwid = if (syncHwid) Hwid.current(ctx) else null
                )
            }
            runCatching { TvLink.send(offer, payload) }
                .onSuccess {
                    snackbars.notice(tr("Подписки на телевизоре", "Subscriptions are on the TV"))
                    dismiss = System.nanoTime()
                }
                .onFailure { snackbars.error(tr("Не удалось передать: ${it.message}", "Couldn't send: ${it.message}")) }
            sending = false
        }
        Unit
    }
    AppSheet(
        title = tr("Передать на телевизор", "Send to TV"),
        onDismiss = onDismiss,
        dismissRequestId = dismiss,
        imePadding = true,
        footer = {
            Button(
                onClick = send,
                enabled = ready,
                shapes = ButtonDefaults.shapes(),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp).height(ButtonDefaults.MediumContainerHeight)
            ) {
                if (sending) {
                    LoadingIndicator(Modifier.size(28.dp), color = MaterialTheme.colorScheme.onPrimary)
                } else {
                    Text(tr("Передать", "Send"), style = MaterialTheme.typography.labelLarge)
                }
            }
        }
    ) {
        TvSendForm(subs, picked, { id -> picked = if (id in picked) picked - id else picked + id }, extra, { extra = it }, syncHwid) { syncHwid = !syncHwid }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun TvSendForm(
    subs: List<Sub>,
    picked: Set<String>,
    onPick: (String) -> Unit,
    extra: String,
    onExtra: (String) -> Unit,
    syncHwid: Boolean,
    onSyncHwid: () -> Unit
) {
    Column(Modifier.padding(horizontal = 16.dp)) {
        if (subs.isNotEmpty()) {
            Header(tr("Подписки", "Subscriptions"))
            Group(
                subs.map { s ->
                    val on = s.id in picked
                    Item(
                        if (Local.isLocal(s)) Glyph.List else Glyph.Link,
                        MaterialShapes.Circle,
                        s.info.title?.takeIf { it.isNotBlank() } ?: if (Local.isLocal(s)) tr("Свои серверы", "My servers") else hostOf(s.info.url),
                        if (Local.isLocal(s) || s.info.title.isNullOrBlank()) null else hostOf(s.info.url),
                        on
                    ) { onPick(s.id) }
                }
            )
        }
        BlockTitle(tr("Новая подписка для телевизора", "New subscription for the TV"))
        TextField(
            value = extra,
            onValueChange = onExtra,
            placeholder = { Text("https://…") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            shape = RoundedCornerShape(Segments.Outer),
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh
            ),
            modifier = Modifier.fillMaxWidth().remoteField(RoundedCornerShape(Segments.Outer))
        )
        Spacer(Modifier.height(20.dp))
        Group(
            listOf(
                Item(Glyph.Smartphone, MaterialShapes.Circle, tr("Синхронизировать HWID", "Sync HWID"), null, syncHwid, onClick = onSyncHwid)
            )
        )
        Spacer(Modifier.height(8.dp))
    }
}
