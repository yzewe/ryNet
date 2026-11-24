package dev.yzewe.rynet.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Backup
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val MAX_FILE = 32 shl 20

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun BackupSection(onImported: (Settings) -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val version = remember { runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull().orEmpty() }
    var note by remember { mutableStateOf<String?>(null) }
    var exportChoice by remember { mutableStateOf<Map<Backup.Part, String?>?>(null) }
    var exportParts by remember { mutableStateOf(emptySet<Backup.Part>()) }
    var snapshot by remember { mutableStateOf<Backup.Snapshot?>(null) }

    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val parts = exportParts
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    val bytes = Backup.export(ctx, parts, version)
                    ctx.contentResolver.openOutputStream(uri, "wt")!!.use { it.write(bytes) }
                }
            }
            note = r.fold({ tr("Настройки сохранены", "Settings saved") }, { tr("Не удалось сохранить: ${it.message}", "Couldn't save: ${it.message}") })
        }
    }
    val open = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    val bytes = ctx.contentResolver.openInputStream(uri)!!.use { s ->
                        val out = java.io.ByteArrayOutputStream()
                        val buf = ByteArray(64 * 1024)
                        while (true) {
                            val n = s.read(buf)
                            if (n < 0) break
                            out.write(buf, 0, n)
                            require(out.size() <= MAX_FILE) { tr("Файл слишком большой", "File is too large") }
                        }
                        out.toByteArray()
                    }
                    Backup.read(bytes)
                }
            }
            r.onSuccess {
                snapshot = it
                note = null
            }.onFailure { note = it.message ?: tr("Не удалось прочитать файл", "Couldn't read the file") }
        }
    }

    Header(tr("Резервная копия", "Backup"))
    Group(
        listOf(
            Item(Glyph.Upload, MaterialShapes.Cookie9Sided, tr("Экспорт настроек", "Export settings"), tr("Сохранить настройки в файл", "Save settings to a file"), action = true) {
                scope.launch { exportChoice = withContext(Dispatchers.IO) { Backup.available(ctx) } }
            },
            Item(Glyph.Download, MaterialShapes.Cookie6Sided, tr("Импорт настроек", "Import settings"), tr("Восстановить настройки из файла", "Restore settings from a file"), action = true) {
                open.launch(arrayOf("*/*"))
            }
        )
    )
    note?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 12.dp, top = 12.dp))
    }

    exportChoice?.let { choice ->
        PartsSheet(tr("Экспорт настроек", "Export settings"), choice, tr("Сохранить", "Save"), onConfirm = { parts ->
            exportParts = parts
            save.launch(Backup.fileName())
        }, onDismiss = { exportChoice = null })
    }
    snapshot?.let { snap ->
        PartsSheet(
            tr("Импорт настроек", "Import settings"),
            snap.parts.keys.associateWith { snap.summary(it) },
            tr("Применить", "Apply"),
            onConfirm = { parts ->
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { Backup.apply(ctx, snap, parts) } }
                    r.onSuccess { (_, new) ->
                        onImported(new)
                        note = tr("Настройки применены", "Settings applied")
                    }.onFailure { note = tr("Не удалось применить: ${it.message}", "Couldn't apply: ${it.message}") }
                }
            },
            onDismiss = { snapshot = null }
        )
    }
}

@Composable
private fun PartsSheet(
    title: String,
    parts: Map<Backup.Part, String?>,
    action: String,
    onConfirm: (Set<Backup.Part>) -> Unit,
    onDismiss: () -> Unit
) {
    var chosen by remember { mutableStateOf(parts.keys) }
    var close by remember { mutableLongStateOf(0L) }
    var then by remember { mutableStateOf<(() -> Unit)?>(null) }
    AppSheet(
        title = title,
        onDismiss = {
            then?.invoke()
            onDismiss()
        },
        dismissRequestId = close,
        footer = {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp)) {
                PillButton(action, {
                    val picked = chosen
                    then = { onConfirm(picked) }
                    close++
                }, Modifier.weight(1f), enabled = chosen.isNotEmpty())
            }
        }
    ) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            parts.forEach { (p, summary) ->
                val on = p in chosen
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(MaterialTheme.shapes.large)
                        .toggleable(on, role = Role.Checkbox) { chosen = if (it) chosen + p else chosen - p }
                        .padding(horizontal = 8.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Checkbox(checked = on, onCheckedChange = null)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(p.title, style = MaterialTheme.typography.bodyLarge)
                        summary?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    }
                }
            }
        }
    }
}
