@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.AppIcon
import dev.yzewe.rynet.data.SectionDivider
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.ThemeMode
import dev.yzewe.rynet.tr
import kotlin.math.abs

private val RADII = listOf(0, 4, 8, 12, 16, 20, 24, 28)

@Composable
fun AppearanceScreen(settings: Settings, onChange: (Settings) -> Unit, onBack: () -> Unit) {
    var iconSheet by remember { mutableStateOf(false) }
    var animSheet by remember { mutableStateOf(false) }
    val orb = OrbStyle.of(settings.orbStyle) ?: OrbStyle.Wave
    PageScaffold(tr("Оформление", "Appearance"), onBack = onBack) {
        item {
            Column(Modifier.enter(0)) {
                ChoiceBlock(tr("Тема", "Theme"), ThemeMode.entries, settings.theme, { it.title }) { onChange(settings.copy(theme = it)) }
                Group(
                    items = listOfNotNull(
                        Item(
                            Glyph.Wallpaper,
                            MaterialShapes.Circle,
                            tr("Динамические цвета", "Dynamic colors"),
                            tr("Цвета берутся из обоев", "Colors come from the wallpaper"),
                            settings.wallpaperColors
                        ) { onChange(settings.copy(wallpaperColors = !settings.wallpaperColors)) }.takeIf { HasWallpaperColors }
                    ),
                    after = {
                        if (HasWallpaperColors) {
                            Fold(!settings.wallpaperColors) { GroupCard { SeedSwatches(settings, onChange) } }
                        } else {
                            GroupCard { SeedSwatches(settings, onChange) }
                        }
                        SegmentDivider()
                        IconRow(iconSheet) { iconSheet = true }
                        SegmentDivider()
                        AnimationRow(orb) { animSheet = true }
                    }
                )
            }
        }
        item {
            Column(Modifier.enter(1)) {
                StepBlock(
                    tr("Скругление секций", "Section corners"),
                    RADII,
                    RADII.minBy { abs(it - settings.sectionRadius) },
                    { "$it dp" },
                    tick = { "$it" },
                    live = true
                ) { onChange(settings.copy(sectionRadius = it)) }
                ChoiceBlock(
                    tr("Разделитель", "Divider"),
                    SectionDivider.entries,
                    settings.sectionDivider,
                    { it.title }
                ) { onChange(settings.copy(sectionDivider = it)) }
            }
        }
    }
    if (iconSheet) IconSheet(onDismiss = { iconSheet = false })
    if (animSheet) StylePicker(orb, onPick = { onChange(settings.copy(orbStyle = it.key)) }, onDismiss = { animSheet = false })
}

@Composable
private fun AnimationRow(style: OrbStyle, onClick: () -> Unit) {
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    SettingsRow(
        shape = RoundedCornerShape(Segments.Inner),
        onClick = {
            view.haptic(Haptic.Tap)
            onClick()
        },
        leading = { RowIcon(Glyph.Pulse, src) },
        title = tr("Анимация", "Animation"),
        summary = style.title,
        interactionSource = src
    ) { Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant) }
}

@Composable
private fun IconRow(sheetOpen: Boolean, onClick: () -> Unit) {
    val ctx = LocalContext.current
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    val icon = remember(sheetOpen) { AppIcon.current(ctx) }
    SettingsRow(
        shape = RoundedCornerShape(Segments.Inner),
        onClick = {
            view.haptic(Haptic.Tap)
            onClick()
        },
        leading = { RowIcon(Glyph.Apps, src) },
        title = tr("Иконка", "Icon"),
        summary = icon.title,
        interactionSource = src
    ) {
        AnimatedContent(
            targetState = icon,
            transitionSpec = { (fadeIn(Springs.effects()) + scaleIn(Springs.bouncy(), 0.6f)) togetherWith (fadeOut(Springs.effects()) + scaleOut(targetScale = 0.6f)) },
            label = "appIcon"
        ) { AppIconImage(it, 36.dp) }
    }
}
