package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.KeyboardArrowUp
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.data.Endpoints
import dev.yzewe.rynet.data.Pings
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.SpeedTest
import dev.yzewe.rynet.tr
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun ServerDetails(
    p: Profile,
    ping: Int?,
    error: String?,
    pinging: Boolean,
    selected: Boolean,
    onCheck: () -> Unit,
    onDismiss: () -> Unit,
    waiting: Boolean = false,
    onSpeed: (() -> Unit)? = null,
    edit: ServerEdit? = null,
    prefs: ServerPrefs? = null,
    onConnect: () -> Unit
) {
    val c = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    var close by remember { mutableLongStateOf(0L) }
    var connect by remember { mutableStateOf(false) }
    var speedNext by remember { mutableStateOf(false) }
    LaunchedEffect(speedNext) {
        if (!speedNext) return@LaunchedEffect
        delay(220)
        onSpeed?.invoke()
    }
    var config by remember { mutableStateOf(false) }
    var dns by remember { mutableStateOf(false) }
    val scroll = rememberScrollState()
    val speed = remember(p.name) { SpeedTest.history(ctx).firstOrNull { it.name == p.name } }
    if (config) ConfigSheet(p, prefs?.body) { config = false }
    val pinned = prefs != null && p.remarks in prefs.pinned
    val serverDns = prefs?.dns?.get(p.remarks).orEmpty()
    if (dns && prefs != null) ServerDnsDialog(p.name, serverDns, onConfirm = { prefs.onDns(p, it) }, onDismiss = { dns = false })
    AppSheet(
        title = tr("Сервер", "Server"),
        onDismiss = {
            if (connect) onConnect()
            onDismiss()
        },
        topDivider = scroll.canScrollBackward,
        bottomDivider = scroll.canScrollForward,
        dismissRequestId = close,
        footer = {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp)) {
                val connectNow: () -> Unit = {
                    connect = true
                    close++
                }
                if (selected) {
                    ToggleButton(
                        checked = true,
                        onCheckedChange = { connectNow() },
                        modifier = Modifier.weight(1f).height(ButtonDefaults.MediumContainerHeight),
                        enabled = p.supported,
                        shapes = ToggleButtonDefaults.shapesFor(ButtonDefaults.MediumContainerHeight),
                        colors = ToggleButtonDefaults.colors(checkedContainerColor = c.secondaryContainer, checkedContentColor = c.onSecondaryContainer)
                    ) {
                        Icon(Icons.Rounded.Check, null, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(ButtonDefaults.iconSpacingFor(ButtonDefaults.MediumContainerHeight)))
                        Text(tr("Подключён", "Connected"), style = MaterialTheme.typography.labelLarge, maxLines = 1)
                    }
                } else {
                    PillButton(tr("Подключиться", "Connect"), connectNow, Modifier.weight(1f), enabled = p.supported)
                }
            }
        }
    ) {
        Column(
            Modifier.verticalScroll(scroll).padding(horizontal = 16.dp).padding(bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                FlagIcon(p.flag, Modifier.size(width = 64.dp, height = 48.dp), MaterialTheme.shapes.large)
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(p.name, color = c.onSurface, style = MaterialTheme.typography.titleLargeEmphasized, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(listOfNotNull(p.kind, p.description).joinToString(" · "), color = c.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            if (p.supported) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Segments.Gap)) {
                    Stat(
                        Glyph.Bolt,
                        c.primary,
                        tr("мс", "ms"),
                        Segments.row(0, 3),
                        Modifier.weight(1f),
                        ready = ping != null && ping >= 0,
                        value = { if (ping != null && ping >= 0) PingText(ping, MaterialTheme.typography.titleLargeEmphasized, short = true) else Text("—", style = MaterialTheme.typography.titleLargeEmphasized, color = c.outline) },
                        trailing = {
                            if (waiting) {
                                LoadingIndicator(Modifier.size(24.dp), color = c.primary)
                            } else {
                                Icon(Icons.Rounded.Refresh, tr("Проверить", "Check"), tint = c.onSurfaceVariant, modifier = Modifier.size(20.dp).clip(RoundedCornerShape(50)).clickable(enabled = !pinging, onClick = onCheck))
                            }
                        }
                    )
                    Stat(Glyph.Down, c.primary, tr("Мбит/с", "Mbps"), Segments.row(1, 3), Modifier.weight(1f), ready = (speed?.download ?: 0.0) > 0, value = { StatValue(speed?.download?.takeIf { it > 0 }?.let(::mbps)) })
                    Stat(Glyph.Up, c.tertiary, tr("Мбит/с", "Mbps"), Segments.row(2, 3), Modifier.weight(1f), ready = (speed?.upload ?: 0.0) > 0, value = { StatValue(speed?.upload?.takeIf { it > 0 }?.let(::mbps)) })
                }
            }
            val problem = when {
                !p.supported -> p.reason?.let { if ("не поддерживается" in it || "not supported" in it) it else tr("Не поддерживается: $it", "Not supported: $it") } ?: tr("Не поддерживается", "Not supported")
                ping == Pings.FAILED -> error
                ping == Pings.TIMEOUT -> error ?: tr("Таймаут: сервер не ответил вовремя", "Timeout: the server didn't respond in time")
                else -> null
            }
            AnimatedVisibility(problem != null) { NoticeBlock(Icons.Rounded.Close, problem.orEmpty(), error = true) }
            val pair = prefs == null
            if (p.supported) {
                Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(if (pair) Segments.Gap else 8.dp)) {
                    Action(Glyph.Bolt, tr("Пинг", "Ping"), Modifier.weight(1f), enabled = !pinging, onClick = onCheck)
                    if (onSpeed != null) {
                        Action(Glyph.Speed, tr("Скорость", "Speed"), Modifier.weight(if (pair) 2f else 1f)) {
                            speedNext = true
                            close++
                        }
                    }
                    if (prefs != null) {
                        Action(Glyph.Pin, if (pinned) tr("Закреплён", "Pinned") else tr("Закрепить", "Pin"), Modifier.weight(1f), checked = pinned, toggle = true) { prefs.onPin(p, !pinned) }
                        Action(Glyph.Globe, dnsName(serverDns), Modifier.weight(1f)) { dns = true }
                    }
                }
            }
            val rows = listOfNotNull<@Composable (Shape) -> Unit>(
                { shape -> NameRow(p.name, shape) },
                prefs?.body?.let { { shape -> LinkRow(Icons.Rounded.Info, tr("Подробности", "Details"), tr("Весь конфиг: адрес, ключи, транспорт", "Full config: address, keys, transport"), shape) { config = true } } },
                p.link?.let { link -> { shape -> ShareRow(p.name, link, shape) } }
            )
            if (rows.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
                    rows.forEachIndexed { i, r -> r(Segments.shape(i, rows.size)) }
                }
            }
            if (edit != null) EditActions(p, edit, then = { close++ })
        }
    }
}

internal fun dnsName(dns: String) = when {
    dns.isEmpty() -> "DNS"
    dns == Endpoints.DNS_GOOGLE -> "Google"
    else -> DNS_PRESETS.firstOrNull { it.second == dns }?.first ?: tr("Свой DNS", "Custom DNS")
}

@Composable
private fun StatValue(v: String?) {
    Text(v ?: "—", style = MaterialTheme.typography.titleLargeEmphasized.merge(TextStyle(fontFeatureSettings = "tnum")), color = if (v == null) MaterialTheme.colorScheme.outline else MaterialTheme.colorScheme.onSurface, maxLines = 1)
}

@Composable
private fun Stat(icon: ImageVector, tint: androidx.compose.ui.graphics.Color, unit: String, shape: Shape, modifier: Modifier, ready: Boolean, value: @Composable () -> Unit, trailing: (@Composable () -> Unit)? = null) {
    Column(modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(horizontal = 14.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = tint, modifier = Modifier.size(16.dp).popOnChange(ready))
            Spacer(Modifier.width(4.dp))
            Text(unit, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
        }
        Row(Modifier.fillMaxWidth().heightIn(min = 28.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) { value() }
            trailing?.invoke()
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Action(icon: ImageVector, label: String, modifier: Modifier, enabled: Boolean = true, checked: Boolean = false, toggle: Boolean = false, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val src = remember { MutableInteractionSource() }
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        FilledIconToggleButton(
            checked = checked,
            onCheckedChange = { onClick() },
            enabled = enabled,
            interactionSource = src,
            modifier = Modifier.size(width = 72.dp, height = 56.dp),
            shapes = IconButtonDefaults.toggleableShapes(),
            colors = IconButtonDefaults.filledIconToggleButtonColors(
                containerColor = c.secondaryContainer,
                contentColor = c.onSecondaryContainer,
                checkedContainerColor = c.primary,
                checkedContentColor = c.onPrimary
            )
        ) { LiveIcon(icon, if (toggle) checked else null, src, LocalContentColor.current) }
        Text(label, style = MaterialTheme.typography.labelMedium, color = c.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun LinkRow(icon: ImageVector, title: String, subtitle: String?, shape: Shape, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val src = remember { MutableInteractionSource() }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(shape).background(c.surfaceContainerHigh).clickable(src, ripple(), onClick = onClick).padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        RowIcon(icon, src)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = c.onSurface)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        Icon(Icons.Rounded.KeyboardArrowRight, null, tint = c.onSurfaceVariant)
    }
}

@Composable
private fun RowIcon(icon: ImageVector, source: InteractionSource = remember { MutableInteractionSource() }) {
    androidx.compose.foundation.layout.Box(
        Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.secondaryContainer),
        contentAlignment = Alignment.Center
    ) { LiveIcon(icon, null, source, MaterialTheme.colorScheme.onSecondaryContainer, size = 22.dp) }
}

@Composable
private fun NameRow(name: String, shape: Shape) {
    val c = MaterialTheme.colorScheme
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1400)
            copied = false
        }
    }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(shape).background(c.surfaceContainerHigh).padding(start = 12.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        RowIcon(Glyph.Caption)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(tr("Имя", "Name"), style = MaterialTheme.typography.bodyLarge, color = c.onSurface)
            Text(name, style = MaterialTheme.typography.bodyMedium, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.MiddleEllipsis)
        }
        CircleIconButton(if (copied) Icons.Rounded.Check else Glyph.Copy, tr("Скопировать имя", "Copy name"), {
            clipboard.setText(AnnotatedString(name))
            copied = true
        }, container = androidx.compose.ui.graphics.Color.Transparent, tint = if (copied) c.primary else c.onSurfaceVariant)
    }
}

@Composable
private fun ShareRow(name: String, link: String, shape: Shape) {
    val c = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var qr by remember { mutableStateOf(false) }
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1400)
            copied = false
        }
    }
    if (qr) QrSheet(name, link) { qr = false }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(shape).background(c.surfaceContainerHigh).padding(start = 12.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        RowIcon(Glyph.Link)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(tr("Ссылка", "Link"), style = MaterialTheme.typography.bodyLarge, color = c.onSurface)
            Text(link.substringBefore("://") + "://" + link.substringAfter("://").substringAfter('@').substringBefore('?').substringBefore('#').substringBefore('/'), style = MaterialTheme.typography.bodyMedium, color = c.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        CircleIconButton(Glyph.Qr, tr("QR-код", "QR code"), { qr = true }, container = androidx.compose.ui.graphics.Color.Transparent, tint = c.onSurfaceVariant)
        CircleIconButton(if (copied) Icons.Rounded.Check else Glyph.Copy, tr("Скопировать ссылку", "Copy link"), {
            clipboard.setText(AnnotatedString(link))
            copied = true
        }, container = androidx.compose.ui.graphics.Color.Transparent, tint = if (copied) c.primary else c.onSurfaceVariant)
        CircleIconButton(Icons.Rounded.Share, tr("Поделиться", "Share"), {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, link)
            runCatching { ctx.startActivity(Intent.createChooser(send, name)) }
        }, container = androidx.compose.ui.graphics.Color.Transparent, tint = c.onSurfaceVariant)
    }
}

private class ConfigRow(val key: String, val value: String, val mono: Boolean, val secret: Boolean, val copy: Boolean)
private class ConfigGroup(val title: String, val rows: List<ConfigRow>)
private class Config(val groups: List<ConfigGroup>, val json: String?, val error: String?)

private fun parseConfig(s: String): Config {
    val o = JSONObject(s)
    val groups = o.optJSONArray("groups")?.let { a ->
        (0 until a.length()).map { i ->
            val g = a.getJSONObject(i)
            val rows = g.optJSONArray("rows")?.let { r ->
                (0 until r.length()).map { j ->
                    val x = r.getJSONObject(j)
                    ConfigRow(x.optString("k"), x.optString("v"), x.optBoolean("mono"), x.optBoolean("secret"), x.optBoolean("copy") || x.optBoolean("secret"))
                }
            }.orEmpty()
            ConfigGroup(g.optString("title"), rows)
        }
    }.orEmpty()
    return Config(groups, o.optString("json").takeIf { !o.isNull("json") && it.isNotBlank() }, null)
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ConfigSheet(p: Profile, body: (() -> String?)?, onDismiss: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val clipboard = LocalClipboardManager.current
    val scroll = rememberScrollState()
    val config by produceState<Config?>(null, p.remarks) {
        value = withContext(Dispatchers.IO) {
            val text = body?.invoke() ?: return@withContext Config(emptyList(), null, tr("Нет подписки", "No subscription"))
            runCatching { parseConfig(Native.details(text, p.remarks)) }.getOrElse { Config(emptyList(), null, it.message ?: it.toString()) }
        }
    }
    AppSheet(title = tr("Подробности", "Details"), onDismiss = onDismiss, topDivider = scroll.canScrollBackward, fillMaxHeight = true) {
        Column(Modifier.verticalScroll(scroll).padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            val cfg = config
            when {
                cfg == null -> LoadingIndicator(Modifier.align(Alignment.CenterHorizontally).padding(32.dp), color = c.primary)

                cfg.error != null -> NoticeBlock(Icons.Rounded.Close, cfg.error, error = true)

                else -> {
                    cfg.groups.forEach { g ->
                        Column {
                            Text(g.title, style = PageType.header, color = c.primary, modifier = Modifier.padding(top = 8.dp, bottom = 12.dp))
                            Column(verticalArrangement = Arrangement.spacedBy(Segments.Gap)) {
                                g.rows.forEachIndexed { i, r -> ConfigLine(r, Segments.shape(i, g.rows.size)) { clipboard.setText(AnnotatedString(r.value)) } }
                            }
                        }
                    }
                    cfg.json?.let { JsonCard(it) { clipboard.setText(AnnotatedString(it)) } }
                }
            }
        }
    }
}

@Composable
private fun ConfigLine(r: ConfigRow, shape: Shape, onCopy: () -> Unit) {
    val c = MaterialTheme.colorScheme
    var shown by remember { mutableStateOf(false) }
    val value = if (r.secret && !shown) "•".repeat(8) + r.value.takeLast(4) else r.value
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).clip(shape).background(c.surfaceContainerHigh).padding(start = 16.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(r.key, style = MaterialTheme.typography.labelLarge, color = c.onSurfaceVariant, modifier = Modifier.width(112.dp), maxLines = 2)
        Text(
            value,
            color = c.onSurface,
            maxLines = 3,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).padding(end = 8.dp),
            style = if (r.mono) MaterialTheme.typography.bodyMedium.merge(TextStyle(fontFamily = FontFamily.Monospace)) else MaterialTheme.typography.bodyLarge
        )
        if (r.secret) {
            CircleIconButton(
                if (shown) Glyph.EyeOff else Glyph.Eye,
                tr("Показать", "Show"),
                { shown = !shown },
                container = androidx.compose.ui.graphics.Color.Transparent,
                tint = c.onSurfaceVariant
            )
        }
        if (r.copy) CopyIcon(onCopy)
    }
}

@Composable
private fun CopyIcon(onCopy: () -> Unit) {
    var done by remember { mutableStateOf(false) }
    LaunchedEffect(done) {
        if (done) {
            delay(1400)
            done = false
        }
    }
    CircleIconButton(if (done) Icons.Rounded.Check else Glyph.Copy, tr("Скопировать", "Copy"), {
        onCopy()
        done = true
    }, container = androidx.compose.ui.graphics.Color.Transparent, tint = if (done) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun JsonCard(json: String, onCopy: () -> Unit) {
    val c = MaterialTheme.colorScheme
    var open by remember { mutableStateOf(false) }
    val lines = json.lines()
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Segments.Outer)).background(c.surfaceContainerHigh).animateContentSize().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Конфиг JSON", "Config JSON"), style = MaterialTheme.typography.titleSmall, color = c.onSurface, modifier = Modifier.weight(1f))
            CopyIcon(onCopy)
            CircleIconButton(
                if (open) Icons.Rounded.KeyboardArrowUp else Icons.Rounded.KeyboardArrowDown,
                if (open) tr("Свернуть", "Collapse") else tr("Развернуть", "Expand"),
                { open = !open },
                container = androidx.compose.ui.graphics.Color.Transparent,
                tint = c.onSurfaceVariant
            )
        }
        Text(
            if (open || lines.size <= 10) json else lines.take(10).joinToString("\n") + "\n…",
            style = MaterialTheme.typography.bodySmall.merge(TextStyle(fontFamily = FontFamily.Monospace)),
            color = c.onSurfaceVariant,
            modifier = Modifier.padding(end = 12.dp)
        )
    }
}
