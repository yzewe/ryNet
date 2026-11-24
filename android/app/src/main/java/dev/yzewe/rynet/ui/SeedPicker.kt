@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.yzewe.rynet.ui

import android.os.Build
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.SliderState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.tr

private val PRESETS = listOf(
    0 to { tr("Фирменный", "Default") },
    0xFF0061A4.toInt() to { tr("Синий", "Blue") },
    0xFF6750A4.toInt() to { tr("Фиолетовый", "Violet") },
    0xFFB4236B.toInt() to { tr("Розовый", "Rose") },
    0xFFC4501F.toInt() to { tr("Оранжевый", "Orange") },
    0xFF5E7C16.toInt() to { tr("Зелёный", "Green") }
)

internal val HasWallpaperColors: Boolean get() = Build.VERSION.SDK_INT >= 31

@Composable
internal fun SeedSwatches(settings: Settings, onChange: (Settings) -> Unit) {
    val c = MaterialTheme.colorScheme
    val dark = c.surface.luminance() < 0.5f
    var picking by remember { mutableStateOf(false) }
    val brand = if (dark) Dark else Light
    val presets = remember(dark) {
        PRESETS.map { (seed, _) -> if (seed == 0) Triple(brand.primary, brand.secondaryContainer, brand.tertiaryContainer) else Seed.swatch(seed, dark) }
    }
    val preset = PRESETS.any { it.first == settings.seedColor }
    val custom = settings.seedColor.takeIf { !preset }
    val customColors = remember(custom, dark) { custom?.let { Seed.swatch(it, dark) } }
    val scroll = rememberScrollState()
    val step = with(LocalDensity.current) { 66.dp.roundToPx() }
    LaunchedEffect(Unit) {
        val at = PRESETS.indexOfFirst { it.first == settings.seedColor }.takeIf { it >= 0 } ?: PRESETS.size
        scroll.scrollTo((at - 2) * step)
    }
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(scroll)
            .padding(horizontal = 12.dp, vertical = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(18.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        PRESETS.forEachIndexed { i, (seed, name) ->
            Swatch(presets[i], settings.seedColor == seed, name()) {
                onChange(settings.copy(wallpaperColors = false, seedColor = seed))
            }
        }
        Swatch(customColors, custom != null, tr("Свой цвет", "Custom color"), badge = if (custom != null) Icons.Rounded.Edit else Glyph.Palette) {
            picking = true
        }
    }
    if (picking) {
        val start = if (settings.seedColor != 0) {
            Seed.hue(settings.seedColor)
        } else {
            Seed.hue(android.graphics.Color.argb(255, (c.primary.red * 255).toInt(), (c.primary.green * 255).toInt(), (c.primary.blue * 255).toInt()))
        }
        SeedSheet(start, dark, onPick = { hue -> onChange(settings.copy(wallpaperColors = false, seedColor = Seed.fromHue(hue))) }, onDismiss = { picking = false })
    }
}

@Composable
private fun Swatch(colors: Triple<Color, Color, Color>?, selected: Boolean, label: String, badge: ImageVector? = null, onClick: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    val ring by animateColorAsState(if (selected) c.primary else Color.Transparent, Springs.effects(), label = "swatchRing")
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .border(2.dp, ring, CircleShape)
            .clickable(src, indication = null) {
                if (badge == null && selected) return@clickable
                view.haptic(Haptic.Segment)
                onClick()
            }
            .semantics {
                contentDescription = label
                role = Role.RadioButton
                this.selected = selected
            }
            .padding(4.dp)
            .pressSquish(src, 0.88f)
            .clip(CircleShape)
            .drawBehind {
                if (colors == null) {
                    drawRect(Brush.sweepGradient(HUES))
                } else {
                    val (a, b, cc) = colors
                    drawRect(a, size = Size(size.width, size.height / 2))
                    drawRect(b, topLeft = Offset(0f, size.height / 2), size = Size(size.width / 2, size.height / 2))
                    drawRect(cc, topLeft = Offset(size.width / 2, size.height / 2), size = Size(size.width / 2, size.height / 2))
                }
            },
        contentAlignment = Alignment.Center
    ) {
        if (badge != null) {
            Box(Modifier.size(22.dp).clip(CircleShape).background(c.surfaceContainerHigh), contentAlignment = Alignment.Center) {
                Icon(badge, null, tint = c.onSurface, modifier = Modifier.size(14.dp))
            }
        }
    }
}

private val HUES: List<Color> = (0..12).map { Color(Hct.toArgb(it * 30.0, 48.0, 62.0)) }

@Composable
private fun SeedSheet(start: Double, dark: Boolean, onPick: (Double) -> Unit, onDismiss: () -> Unit) {
    val slider = remember { SliderState(start.toFloat().coerceIn(0f, 359f), 0, 0f..359f) }
    val hue = slider.value
    var close by remember { mutableLongStateOf(0L) }
    val preview = remember(hue.toInt(), dark) { Seed.swatch(Seed.fromHue(hue.toDouble()), dark) }
    AppSheet(
        title = tr("Свой цвет", "Custom color"),
        onDismiss = onDismiss,
        dismissRequestId = close,
        footer = {
            PillButton(
                tr("Готово", "Done"),
                {
                    onPick(hue.toDouble())
                    close++
                },
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp),
                haptic = Haptic.Confirm
            )
        }
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Preview(preview, 112.dp)
            Spacer(Modifier.height(24.dp))
            Slider(
                state = slider,
                onValueChange = { slider.value = it },
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = tr("Оттенок", "Hue") },
                colors = SliderDefaults.colors(thumbColor = preview.first),
                track = { HueTrack() }
            )
            Spacer(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun Preview(colors: Triple<Color, Color, Color>, size: Dp) {
    Box(
        Modifier.size(size).clip(CircleShape).drawBehind {
            val (a, b, c) = colors
            drawRect(a, size = Size(this.size.width, this.size.height / 2))
            drawRect(b, topLeft = Offset(0f, this.size.height / 2), size = Size(this.size.width / 2, this.size.height / 2))
            drawRect(c, topLeft = Offset(this.size.width / 2, this.size.height / 2), size = Size(this.size.width / 2, this.size.height / 2))
        }
    )
}

@Composable
private fun HueTrack() {
    Canvas(Modifier.fillMaxWidth().height(16.dp)) {
        drawRoundRect(Brush.horizontalGradient(HUES), cornerRadius = CornerRadius(size.height / 2))
    }
}
