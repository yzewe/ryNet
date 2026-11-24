package dev.yzewe.rynet.ui

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.text.format.DateUtils
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material3.CircularWavyProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.graphics.shapes.RoundedPolygon
import dev.yzewe.rynet.Lang
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.SpeedResult
import dev.yzewe.rynet.data.SpeedRun
import dev.yzewe.rynet.data.SpeedTest
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlin.math.ln

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SpeedScreen(profile: Profile?, onStart: () -> Unit, onCancel: () -> Unit, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var run by remember { mutableStateOf(SpeedRun()) }
    var history by remember { mutableStateOf(SpeedTest.history(ctx)) }
    DisposableEffect(Unit) {
        val r = object : BroadcastReceiver() {
            override fun onReceive(c: Context, i: Intent) {
                run = SpeedTest.read(i)
                if (run.phase == SpeedRun.Phase.Done) history = SpeedTest.history(ctx)
            }
        }
        ContextCompat.registerReceiver(ctx, r, IntentFilter(SpeedTest.ACTION), ContextCompat.RECEIVER_NOT_EXPORTED)
        onDispose {
            ctx.unregisterReceiver(r)
            if (run.running) onCancel()
        }
    }
    var confirmClear by remember { mutableStateOf(false) }
    if (confirmClear) {
        DeleteDialog(
            tr("Очистить историю?", "Clear history?"),
            if (history.size == 1) tr("Замер будет удалён.", "The test will be deleted.") else plural(history.size, "замер", "замера", "замеров", "test").let { tr("$it будут удалены.", "$it will be deleted.") },
            tr("Очистить", "Clear"),
            onConfirm = {
                SpeedTest.clear(ctx)
                history = emptyList()
            },
            onDismiss = { confirmClear = false }
        )
    }
    PageScaffold(
        tr("Скорость", "Speed"),
        onBack,
        actions = {
            if (history.isNotEmpty() && !run.running) {
                CircleIconButton(
                    Icons.Rounded.Delete,
                    tr("Очистить историю", "Clear history"),
                    { confirmClear = true },
                    container = MaterialTheme.colorScheme.surfaceContainerHigh,
                    tint = MaterialTheme.colorScheme.onSurface
                )
            }
        }
    ) {
        item(key = "speed") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Hero(run, profile, onStart = {
                    run = SpeedRun(SpeedRun.Phase.Servers)
                    onStart()
                }, onCancel = onCancel)
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    val big = 28.dp
                    val small = 8.dp
                    Tile(
                        tr("Пинг", "Ping"), Glyph.Pulse, MaterialShapes.Cookie4Sided, run.ping?.let { "${it.toInt()}" }, tr("мс", "ms"),
                        run.jitter?.let { tr("джиттер ${it.toInt()}", "jitter ${it.toInt()}") }, run.phase == SpeedRun.Phase.Ping,
                        RoundedCornerShape(topStart = big, bottomStart = big, topEnd = small, bottomEnd = small), Modifier.weight(1f)
                    )
                    Tile(
                        tr("Загрузка", "Download"), Glyph.Down, MaterialShapes.Cookie9Sided, run.download?.let(::mbps), tr("Мбит/с", "Mbps"), null,
                        run.phase == SpeedRun.Phase.Download, RoundedCornerShape(small), Modifier.weight(1f)
                    )
                    Tile(
                        tr("Отдача", "Upload"), Glyph.Up, MaterialShapes.Sunny, run.upload?.let(::mbps), tr("Мбит/с", "Mbps"), null,
                        run.phase == SpeedRun.Phase.Upload,
                        RoundedCornerShape(topStart = small, bottomStart = small, topEnd = big, bottomEnd = big), Modifier.weight(1f)
                    )
                }
                AnimatedVisibility(run.phase == SpeedRun.Phase.Error, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                    NoticeBlock(Icons.Rounded.Close, run.error.orEmpty(), error = true)
                }
                if (history.isNotEmpty()) {
                    Column {
                        Header(tr("История", "History"))
                        Column(verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
                            history.forEachIndexed { i, h -> HistoryRow(h, Segments.shape(i, history.size)) }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Hero(run: SpeedRun, profile: Profile?, onStart: () -> Unit, onCancel: () -> Unit) {
    val speed = run.phase == SpeedRun.Phase.Download || run.phase == SpeedRun.Phase.Upload
    val target = when {
        speed -> scale(run.value / 1e6)
        run.phase == SpeedRun.Phase.Ping -> run.fraction.toFloat()
        run.phase == SpeedRun.Phase.Done -> scale((run.download ?: 0.0) / 1e6)
        else -> 0f
    }
    val progress by animateFloatAsState(target, spring(dampingRatio = 0.8f, stiffness = Spring.StiffnessLow), label = "gauge")
    val color by animateColorAsState(if (run.phase == SpeedRun.Phase.Upload) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.primary, label = "color")
    val density = LocalDensity.current
    Box {
        Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            profile?.let { p ->
                Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    FlagIcon(p.flag, Modifier.size(width = 40.dp, height = 30.dp), MaterialTheme.shapes.small)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(p.name, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(p.kind, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            val target = run.server ?: run.result?.let { listOf(it.sponsor, it.target).filter { s -> s.isNotBlank() }.joinToString(" · ") }
            AnimatedVisibility(target != null || run.phase == SpeedRun.Phase.Servers, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                Surface(shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp)) {
                    Row(Modifier.padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Glyph.Globe, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(tr("Сервер Ookla", "Ookla server"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(
                                target ?: tr("Ищу ближайший к выходу VPN…", "Finding one near the VPN exit…"),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurface,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                }
            }
            Box(Modifier.size(248.dp), contentAlignment = Alignment.Center) {
                val stroke = with(density) { Stroke(6.dp.toPx(), cap = StrokeCap.Round) }
                CircularWavyProgressIndicator(
                    progress = { progress },
                    modifier = Modifier.size(248.dp),
                    color = color,
                    trackColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                    stroke = stroke,
                    trackStroke = stroke,
                    amplitude = { if (run.running) 0.6f else 0f },
                    wavelength = 28.dp
                )
                Center(run, speed)
            }
            Spacer(Modifier.height(16.dp))
            AnimatedContent(run.running, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "button") { running ->
                if (running) {
                    PillButton(
                        tr("Остановить", "Stop"),
                        onCancel,
                        Modifier.fillMaxWidth(),
                        accent = false,
                        container = MaterialTheme.colorScheme.surfaceContainerHighest,
                        leading = { Icon(Icons.Rounded.Close, null, Modifier.size(22.dp)) }
                    )
                } else {
                    val again = run.phase == SpeedRun.Phase.Done || run.phase == SpeedRun.Phase.Error
                    PillButton(
                        if (again) tr("Ещё раз", "Again") else tr("Начать", "Start"),
                        onStart,
                        Modifier.fillMaxWidth(),
                        enabled = profile?.supported == true,
                        leading = { Icon(Glyph.Speed, null, Modifier.size(22.dp)) }
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Center(run: SpeedRun, speed: Boolean) {
    val shown by animateFloatAsState(run.value.toFloat(), spring(stiffness = Spring.StiffnessMediumLow), label = "value")
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            when (run.phase) {
                SpeedRun.Phase.Servers -> tr("Ищу сервер", "Finding server")
                SpeedRun.Phase.Ping -> tr("Задержка", "Latency")
                SpeedRun.Phase.Download, SpeedRun.Phase.Done -> tr("Загрузка", "Download")
                SpeedRun.Phase.Upload -> tr("Отдача", "Upload")
                else -> "Ookla"
            },
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        if (run.phase == SpeedRun.Phase.Servers) {
            LoadingIndicator(Modifier.size(72.dp), color = MaterialTheme.colorScheme.primary)
            return@Column
        }
        val (num, unit) = when {
            speed -> mbps(shown.toDouble()) to tr("Мбит/с", "Mbps")
            run.phase == SpeedRun.Phase.Ping -> shown.toInt().toString() to tr("мс", "ms")
            run.phase == SpeedRun.Phase.Done -> (run.download?.let(::mbps) ?: "—") to tr("Мбит/с", "Mbps")
            else -> "—" to tr("Мбит/с", "Mbps")
        }
        Text(
            num,
            color = MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
            style = MaterialTheme.typography.displayLargeEmphasized.merge(TextStyle(fontFeatureSettings = "tnum"))
        )
        Text(unit, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

internal fun mbps(bps: Double): String {
    val m = bps / 1e6
    return if (m < 100) String.format(Lang.locale, "%.1f", m) else m.toInt().toString()
}

private fun scale(mbps: Double): Float = (ln(1 + mbps.coerceAtLeast(0.0)) / ln(1 + 1000.0)).toFloat().coerceIn(0f, 1f)

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Tile(
    title: String,
    icon: ImageVector,
    shape: RoundedPolygon,
    value: String?,
    unit: String,
    sub: String?,
    active: Boolean,
    outline: Shape,
    modifier: Modifier
) {
    val c = MaterialTheme.colorScheme
    val bg by animateColorAsState(if (active) c.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh, label = "tile")
    val fg = if (active) c.onPrimaryContainer else MaterialTheme.colorScheme.onSurface
    Surface(shape = outline, color = bg, modifier = modifier) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            ShapeIcon(icon, shape, if (active) c.primary else c.primaryContainer, if (active) c.onPrimary else c.onPrimaryContainer, size = 32.dp)
            Spacer(Modifier.height(4.dp))
            Text(title, style = MaterialTheme.typography.labelMedium, color = fg.copy(alpha = 0.75f), maxLines = 1)
            Box(Modifier.height(32.dp), contentAlignment = Alignment.CenterStart) {
                when {
                    value != null -> Row(verticalAlignment = Alignment.Bottom) {
                        Text(value, style = MaterialTheme.typography.titleLargeEmphasized, color = fg, maxLines = 1)
                        Spacer(Modifier.width(3.dp))
                        Text(unit, style = MaterialTheme.typography.labelSmall, color = fg.copy(alpha = 0.7f), modifier = Modifier.padding(bottom = 3.dp), maxLines = 1)
                    }

                    active -> LoadingIndicator(Modifier.size(30.dp), color = MaterialTheme.colorScheme.primary)

                    else -> Text("—", style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.outline)
                }
            }
            Text(sub ?: " ", style = MaterialTheme.typography.labelSmall, color = fg.copy(alpha = 0.7f), maxLines = 1)
        }
    }
}

@Composable
private fun HistoryRow(h: SpeedResult, shape: Shape) {
    val ctx = LocalContext.current
    Surface(shape = shape, color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            FlagIcon(h.flag, Modifier.size(width = 36.dp, height = 27.dp), MaterialTheme.shapes.small)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(h.name, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    DateUtils.formatDateTime(ctx, h.at, DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_TIME or DateUtils.FORMAT_ABBREV_MONTH) +
                        tr(" · ${h.ping.toInt()} мс", " · ${h.ping.toInt()} ms"),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1
                )
            }
            Column(horizontalAlignment = Alignment.End) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Glyph.Down, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
                    Text(mbps(h.download), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurface)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Glyph.Up, null, tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(16.dp))
                    Text(mbps(h.upload), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurface)
                }
            }
        }
    }
}
