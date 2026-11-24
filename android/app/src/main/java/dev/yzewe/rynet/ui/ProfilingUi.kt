package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import dev.yzewe.rynet.data.Profiling
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.vpn.RynetService
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun ProfilingSection(vpnOn: Boolean, extra: List<Item> = emptyList()) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var dumping by remember { mutableStateOf(false) }
    var tracing by remember { mutableStateOf(Profiling.isTracing) }
    var note by remember { mutableStateOf<String?>(null) }

    fun share(files: List<File>, title: String) {
        val uris = ArrayList(files.map { FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", it) })
        val send = if (uris.size == 1) {
            Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
        } else {
            Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
        }
        send.setType("application/octet-stream").addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        ctx.startActivity(Intent.createChooser(send, title))
    }

    fun dump() {
        dumping = true
        note = null
        scope.launch {
            val files = withContext(Dispatchers.IO) {
                Profiling.clean(ctx)
                val stamp = Profiling.stamp()
                val ui = File(Profiling.exportDir(ctx), "rynet-ui-$stamp.hprof")
                val core = File(Profiling.exportDir(ctx), "rynet-core-$stamp.hprof")
                if (vpnOn) RynetService.dumpHeap(ctx, core.path)
                Profiling.dumpHeap(ui)
                if (vpnOn) repeat(60) { if (!core.exists()) delay(500) }
                listOf(ui, core).filter { it.exists() }
            }
            dumping = false
            if (files.isEmpty()) note = tr("Не удалось снять дамп", "Couldn't capture the dump") else share(files, tr("Дамп памяти ryNet", "ryNet heap dump"))
        }
    }

    fun trace() {
        if (!tracing) {
            Profiling.clean(ctx)
            runCatching { Profiling.startTrace(ctx) }
                .onSuccess {
                    tracing = true
                    note = tr("Трассировка идёт — повторите проблему и нажмите «Стоп»", "Tracing — reproduce the issue and tap \"Stop\"")
                }
                .onFailure { note = tr("Трассировка не запустилась: ${it.message}", "Tracing failed to start: ${it.message}") }
        } else {
            val f = Profiling.stopTrace()
            tracing = false
            note = null
            if (f != null && f.exists()) share(listOf(f), tr("Трассировка ryNet", "ryNet trace")) else note = tr("Трассировка не записалась", "Trace wasn't recorded")
        }
    }

    Header(tr("Профилирование", "Profiling"))
    Group(
        listOf(
            Item(
                Glyph.Memory,
                MaterialShapes.Clover4Leaf,
                tr("Дамп памяти", "Heap dump"),
                null,
                action = true,
                busy = dumping,
                enabled = !tracing,
                onClick = ::dump
            ),
            Item(
                Glyph.Timer,
                MaterialShapes.Sunny,
                if (tracing) tr("Остановить трассировку", "Stop tracing") else tr("Трассировка", "Trace"),
                null,
                action = true,
                enabled = !dumping,
                onClick = ::trace
            )
        ) + extra
    )
    note?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 12.dp, top = 12.dp))
    }
}
