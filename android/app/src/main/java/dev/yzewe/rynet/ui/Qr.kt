package dev.yzewe.rynet.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import dev.yzewe.rynet.tr

object QrScanner {
    fun subUrl(text: String): String? {
        val t = text.trim()
        if (t.startsWith("http://", true) || t.startsWith("https://", true)) return t
        val decoded = runCatching { Uri.decode(t) }.getOrDefault(t)
        val i = decoded.indexOf("https://", ignoreCase = true).takeIf { it >= 0 } ?: decoded.indexOf("http://", ignoreCase = true)
        if (i < 0) return null
        return decoded.substring(i).let { if (t.startsWith("sing-box://")) it.substringBefore('#') else it }.trim()
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun QrCode(text: String, modifier: Modifier = Modifier, stream: Boolean = false) {
    val matrix = remember(text) {
        runCatching {
            QRCodeWriter().encode(
                text,
                BarcodeFormat.QR_CODE,
                0,
                0,
                mapOf(EncodeHintType.MARGIN to 0, EncodeHintType.ERROR_CORRECTION to if (stream) ErrorCorrectionLevel.Q else ErrorCorrectionLevel.H, EncodeHintType.CHARACTER_SET to "UTF-8")
            )
        }.getOrNull()
    }
    val bg = MaterialTheme.colorScheme.primaryFixed
    val ink = MaterialTheme.colorScheme.onPrimaryFixed
    val key = if (stream) Unit else text
    val appear = remember(key) { Animatable(0f) }
    LaunchedEffect(key) { appear.animateTo(1f, tween(700, easing = FastOutSlowInEasing)) }
    Box(modifier.aspectRatio(1f).clip(MaterialTheme.shapes.extraLargeIncreased).background(bg).padding(22.dp), contentAlignment = Alignment.Center) {
        if (matrix == null) return@Box
        val n = matrix.width
        val hole = ((n * 0.22f).toInt() or 1).coerceAtLeast(5)
        val h0 = (n - hole) / 2
        val shapes = listOf(MaterialShapes.Cookie9Sided.toShape(), MaterialShapes.Clover4Leaf.toShape(), MaterialShapes.Sunny.toShape())
        Canvas(
            Modifier.fillMaxWidth().aspectRatio(1f).graphicsLayer {
                alpha = appear.value
                val k = 0.94f + 0.06f * appear.value
                scaleX = k
                scaleY = k
            }
        ) {
            val cell = size.width / n
            fun on(x: Int, y: Int): Boolean {
                if (x !in 0 until n || y !in 0 until n || !matrix[x, y]) return false
                if ((x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7)) return false
                return !(x in h0 until h0 + hole && y in h0 until h0 + hole)
            }
            val r = cell * 0.43f
            for (y in 0 until n) {
                for (x in 0 until n) {
                    if (!on(x, y)) continue
                    val cx = (x + 0.5f) * cell
                    val cy = (y + 0.5f) * cell
                    drawCircle(ink, r, Offset(cx, cy))
                    if (on(x + 1, y)) drawRect(ink, Offset(cx, cy - r), Size(cell, 2 * r))
                    if (on(x, y + 1)) drawRect(ink, Offset(cx - r, cy), Size(2 * r, cell))
                }
            }
            listOf(0 to 0, n - 7 to 0, 0 to n - 7).forEachIndexed { i, (ex, ey) ->
                val o = Offset(ex * cell, ey * cell)
                drawRoundRect(ink, o + Offset(cell / 2, cell / 2), Size(cell * 6, cell * 6), CornerRadius(cell * 2.2f), style = Stroke(cell))
                val side = cell * 3.4f
                val outline = shapes[i].createOutline(Size(side, side), layoutDirection, this)
                translate(o.x + (cell * 7 - side) / 2, o.y + (cell * 7 - side) / 2) {
                    rotate(appear.value * 45f, Offset(side / 2, side / 2)) { drawOutline(outline, ink) }
                }
            }
        }
        Box(
            Modifier.fillMaxWidth((hole - 1f) / n).aspectRatio(1f).graphicsLayer {
                scaleX = appear.value
                scaleY = appear.value
            }.clip(MaterialShapes.Cookie12Sided.toShape()).background(ink),
            contentAlignment = Alignment.Center
        ) {
            LogoMark(bg, Modifier.fillMaxSize(0.62f))
        }
    }
}

@Composable
fun QrSheet(title: String, text: String, onDismiss: () -> Unit) {
    val clipboard = LocalClipboardManager.current
    val ctx = LocalContext.current
    AppSheet(title = title, onDismiss = onDismiss) {
        Column(
            Modifier.padding(horizontal = 24.dp).padding(bottom = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            QrCode(text, Modifier.fillMaxWidth(0.9f))
            val shown = text.substringBefore("://") + "://" + text.substringAfter("://").substringBefore('#').substringBefore('?').substringAfterLast('@').substringBefore('/')
            Text(
                shown,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton(
                    tr("Скопировать", "Copy"),
                    { clipboard.setText(AnnotatedString(text)) },
                    Modifier.weight(1f),
                    accent = false,
                    container = MaterialTheme.colorScheme.surfaceContainerHighest,
                    padding = 12.dp,
                    leading = { Icon(Glyph.Copy, null, modifier = Modifier.size(20.dp)) }
                )
                PillButton(
                    tr("Поделиться", "Share"),
                    {
                        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
                        runCatching { ctx.startActivity(Intent.createChooser(send, title)) }
                    },
                    Modifier.weight(1f),
                    padding = 12.dp,
                    leading = { Icon(Icons.Rounded.Share, null, modifier = Modifier.size(20.dp)) }
                )
            }
        }
    }
}
