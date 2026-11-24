package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import dev.yzewe.rynet.data.CoreProc
import dev.yzewe.rynet.data.CoreStats
import dev.yzewe.rynet.data.Diag
import dev.yzewe.rynet.data.Reports
import dev.yzewe.rynet.data.Root
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.vpn.RynetService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun DiagScreen(settings: Settings, vpnOn: Boolean, onChange: (Settings) -> Unit, onBack: () -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf<String?>(null) }
    var refresh by remember { mutableStateOf(0) }
    var confirmClear by remember { mutableStateOf(false) }
    var confirmSend by remember { mutableStateOf(false) }
    if (confirmSend) SendLogsDialog(onConfirm = { onChange(settings.sendingLogs(true)) }, onDismiss = { confirmSend = false })
    if (confirmClear) {
        DeleteDialog(
            tr("Очистить логи?", "Clear logs?"),
            tr("Записанные логи будут удалены.", "Recorded logs will be deleted."),
            tr("Очистить", "Clear"),
            onConfirm = {
                scope.launch(Dispatchers.IO) {
                    Diag.clear(ctx)
                    refresh++
                }
            },
            onDismiss = { confirmClear = false }
        )
    }

    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val size by produceState(0L, refresh, settings.debugLog) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                value = withContext(Dispatchers.IO) { Diag.size(ctx) }
                delay(2000)
            }
        }
    }
    val sentAt by produceState(0L, refresh, settings.sendLogs) { value = withContext(Dispatchers.IO) { Reports.sentAt(ctx) } }
    val live by produceState<Pair<CoreStats.Snapshot?, CoreProc?>>(null to null, vpnOn) {
        if (!vpnOn) return@produceState
        val stats = withContext(Dispatchers.IO) { CoreStats.open(ctx) }
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                RynetService.probe(ctx)
                delay(300)
                value = withContext(Dispatchers.IO) { stats?.read() to CoreProc.load(ctx) }
                delay(1700)
            }
        }
    }
    val tail by produceState(emptyList<String>(), refresh) {
        value = withContext(Dispatchers.IO) { Diag.tail(ctx).takeLast(80) }
    }

    val root by produceState(false) { value = withContext(Dispatchers.IO) { Root.available() } }

    fun toggleLog() {
        val on = !settings.debugLog
        onChange(if (on) settings.copy(debugLog = true) else settings.sendingLogs(false))
        note = if (on) tr("Запись начнётся при подключении", "Recording starts on connect") else null
        if (on && root) scope.launch(Dispatchers.IO) { Root.run("dumpsys batterystats --reset") }
    }

    fun share() {
        busy = true
        scope.launch {
            if (vpnOn) {
                RynetService.flushLog(ctx)
                delay(400)
            }
            val zip = withContext(Dispatchers.IO) { runCatching { Diag.export(ctx, root) } }
            busy = false
            zip.onSuccess {
                val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", it)
                val send = Intent(Intent.ACTION_SEND).setType("application/zip")
                    .putExtra(Intent.EXTRA_STREAM, uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                ctx.startActivity(Intent.createChooser(send, tr("Журнал ryNet", "ryNet log")))
            }.onFailure { note = it.message }
        }
    }

    PageScaffold(tr("Диагностика", "Diagnostics"), onBack = onBack) {
        item {
            Header(tr("Запись", "Recording"))
            Group(
                listOf(
                    Item(
                        Glyph.Pulse,
                        MaterialShapes.Cookie6Sided,
                        tr("Подробный журнал", "Verbose log"),
                        if (size > 0) bytes(size) else null,
                        settings.debugLog
                    ) { toggleLog() },
                    Item(
                        Glyph.Upload,
                        MaterialShapes.Gem,
                        tr("Отправлять разработчику", "Send to the developer"),
                        sentAt.takeIf { settings.sendLogs && it > 0 }?.let { tr("отправлено ", "sent ") + time(it) },
                        settings.sendLogs
                    ) { if (settings.sendLogs) onChange(settings.sendingLogs(false)) else confirmSend = true },
                    Item(
                        Icons.Rounded.Share,
                        MaterialShapes.Cookie9Sided,
                        tr("Поделиться журналом", "Share log"),
                        null,
                        action = true,
                        busy = busy,
                        enabled = size > 0,
                        onClick = ::share
                    ),
                    Item(
                        Icons.Rounded.Delete,
                        MaterialShapes.Cookie7Sided,
                        tr("Очистить журнал", "Clear log"),
                        null,
                        action = true,
                        enabled = size > 0
                    ) { confirmClear = true }
                )
            )
            note?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 12.dp, top = 12.dp))
            }
        }
        item {
            ProfilingSection(
                vpnOn,
                extra = listOf(
                    Item(
                        Glyph.Grid,
                        MaterialShapes.Cookie12Sided,
                        tr("Отладка интерфейса", "UI debugging"),
                        null,
                        settings.uiDebug
                    ) { onChange(settings.copy(uiDebug = !settings.uiDebug)) }
                )
            )
        }
        item {
            Header(tr("Ядро", "Core"))
            val (s, p) = live
            if (!vpnOn || s == null) {
                Group(listOf(Item(Glyph.Power, MaterialShapes.Square, tr("Состояние", "State"), tr("VPN выключен", "VPN is off"), onClick = null)))
            } else {
                Group(
                    emptyList(),
                    top = {
                        GroupCard {
                            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                Metric(tr("Память", "Memory"), p?.let { bytes(it.rssKb * 1024) } ?: "—")
                                p?.pssKb?.let { pss ->
                                    Metric(
                                        tr("Из неё своя", "Own share"),
                                        listOfNotNull(
                                            "PSS ${bytes(pss * 1024)}",
                                            p.nativeKb?.let { "native ${bytes(it * 1024)}" },
                                            p.javaKb?.let { "Java ${bytes(it * 1024)}" }
                                        ).joinToString(" · ")
                                    )
                                }
                                Metric(tr("Процессор", "CPU"), p?.let { tr("%.1f с", "%.1f s").format(it.cpuMs / 1000.0) } ?: "—")
                                Metric(
                                    tr("Пробуждения", "Wakeups"),
                                    p?.wakeups?.let { w -> listOfNotNull("$w", p.wakeRate?.let { tr("%.1f в секунду", "%.1f per second").format(it) }).joinToString(" · ") } ?: "—"
                                )
                                Metric(tr("Соединения", "Connections"), tr("${s.tcpActive} TCP · ${s.udpActive} UDP · всего ${s.tcpTotal}", "${s.tcpActive} TCP · ${s.udpActive} UDP · total ${s.tcpTotal}"))
                                Metric(tr("DNS-запросы", "DNS queries"), s.dns.toString())
                                if (s.errors.any { it > 0 }) {
                                    val (t, tls, reset, refused, other) = s.errors
                                    Metric(tr("Ошибки", "Errors"), tr("таймаут $t · TLS $tls · сброс $reset · отказ $refused · прочие $other", "timeout $t · TLS $tls · reset $reset · refused $refused · other $other"))
                                }
                                Metric(tr("Трафик", "Traffic"), "↓ ${bytes(s.down)} · ↑ ${bytes(s.up)}")
                            }
                        }
                    }
                )
            }
        }
        if (tail.isNotEmpty()) {
            item {
                Header(tr("Журнал", "Log"))
                Surface(shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.fillMaxWidth()) {
                    Text(
                        tail.joinToString("\n") { it.substringAfter('T').replace(Regex("^(\\d\\d:\\d\\d:\\d\\d)\\.\\d+Z"), "$1") },
                        style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
                        softWrap = false,
                        modifier = Modifier.horizontalScroll(rememberScrollState()).padding(16.dp)
                    )
                }
            }
        }
    }
    LaunchedEffect(settings.debugLog) { refresh++ }
}

@Composable
private fun Metric(label: String, value: String) {
    Row {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, softWrap = false)
        Spacer(Modifier.width(16.dp))
        Text(value.replace(Regex("(?<!·) (?!·)"), "\u00A0"), style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.End, modifier = Modifier.weight(1f))
    }
}
