package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import dev.yzewe.rynet.R
import dev.yzewe.rynet.data.Update
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun UpdateSection() {
    val ctx = LocalContext.current
    if (ctx.resources.getBoolean(R.bool.play)) return
    val scope = rememberCoroutineScope()
    val version = remember { runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull().orEmpty() }
    var release by remember { mutableStateOf<Update.Release?>(null) }
    var busy by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf<String?>(null) }

    fun fail(e: Throwable) {
        note = tr("Не удалось: ${e.message ?: e.javaClass.simpleName}", "Failed: ${e.message ?: e.javaClass.simpleName}")
    }

    val found = release
    Header(tr("Обновление", "Update"))
    Group(
        listOf(
            Item(
                Glyph.Download,
                MaterialShapes.Cookie9Sided,
                if (found == null) tr("Проверить обновления", "Check for updates") else tr("Установить ${found.version}", "Install ${found.version}"),
                if (found == null) tr("Версия $version", "Version $version") else tr("Доступна новая версия, текущая $version", "A new version is available, installed $version"),
                action = true,
                busy = busy
            ) {
                note = null
                if (busy) {
                } else if (found == null) {
                    busy = true
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { Update.check(ctx) } }
                            .onSuccess { r ->
                                release = r
                                if (r == null) note = tr("Установлена последняя версия", "You have the latest version")
                            }
                            .onFailure(::fail)
                        busy = false
                    }
                } else if (!Update.canInstall(ctx)) {
                    note = tr("Разрешите установку из ryNet и нажмите ещё раз", "Allow installs from ryNet and tap again")
                    runCatching { ctx.startActivity(Update.permissionIntent(ctx)) }
                } else {
                    busy = true
                    note = tr("Скачивание…", "Downloading…")
                    scope.launch {
                        runCatching {
                            val file = withContext(Dispatchers.IO) {
                                var last = -1
                                Update.download(ctx, found) { f ->
                                    val pct = (f * 100).toInt()
                                    if (f >= 0 && pct != last) {
                                        last = pct
                                        note = tr("Скачивание: $pct%", "Downloading: $pct%")
                                    }
                                }
                            }
                            note = null
                            Update.install(ctx, file)
                        }.onFailure(::fail)
                        busy = false
                    }
                }
            }
        )
    )
    found?.news?.let { news ->
        Text(tr("Что нового", "What's new"), color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(start = 12.dp, top = 12.dp))
        Column(Modifier.padding(start = 12.dp, top = 4.dp).heightIn(max = 220.dp).verticalScroll(rememberScrollState())) {
            Text(news.text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        news.link?.let { link ->
            TextButton({ runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, link.toUri())) } }) { Text(tr("Открыть пост", "Open the post")) }
        }
    }
    note?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 12.dp, top = 12.dp))
    }
}
