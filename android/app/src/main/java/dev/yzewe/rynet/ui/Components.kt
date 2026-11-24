@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.yzewe.rynet.data.SectionDivider
import dev.yzewe.rynet.data.Settings

object PageType {
    val title = TextStyle(fontSize = 36.sp, lineHeight = 44.sp, fontWeight = FontWeight.SemiBold)
    val bar = TextStyle(fontSize = 24.sp, lineHeight = 32.sp, fontWeight = FontWeight.SemiBold)
    val rowTitle = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
    val rowSummary = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal)
    val header = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium)
}

data class SectionStyle(val outer: Dp = 16.dp, val inner: Dp = 4.dp, val gap: Dp = 2.dp) {
    companion object {
        fun of(s: Settings) = SectionStyle(s.sectionRadius.dp, innerOf(s), gapOf(s))

        @Composable
        fun animated(s: Settings): SectionStyle {
            val spec = spring<Dp>(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow)
            val outer by animateDpAsState(s.sectionRadius.dp, spec, label = "sectionOuter")
            val inner by animateDpAsState(innerOf(s), spec, label = "sectionInner")
            val gap by animateDpAsState(gapOf(s), spec, label = "sectionGap")
            return SectionStyle(outer.coerceAtLeast(0.dp), inner.coerceAtLeast(0.dp), gap.coerceAtLeast(0.dp))
        }

        private fun innerOf(s: Settings) = if (s.sectionDivider == SectionDivider.Gap) minOf(4.dp, s.sectionRadius.dp) else 0.dp
        private fun gapOf(s: Settings) = if (s.sectionDivider == SectionDivider.Gap) 2.dp else 0.dp
    }
}

val LocalSectionStyle = compositionLocalOf { SectionStyle() }

object Segments {
    val Gap: Dp @Composable get() = LocalSectionStyle.current.gap

    val Outer: Dp @Composable get() = LocalSectionStyle.current.outer

    val Inner: Dp @Composable get() = LocalSectionStyle.current.inner

    @Composable
    fun shape(index: Int, count: Int): Shape = RoundedCornerShape(
        topStart = if (index == 0) Outer else Inner,
        topEnd = if (index == 0) Outer else Inner,
        bottomStart = if (index == count - 1) Outer else Inner,
        bottomEnd = if (index == count - 1) Outer else Inner
    )

    @Composable
    fun row(index: Int, count: Int): Shape = RoundedCornerShape(
        topStart = if (index == 0) Outer else Inner,
        bottomStart = if (index == 0) Outer else Inner,
        topEnd = if (index == count - 1) Outer else Inner,
        bottomEnd = if (index == count - 1) Outer else Inner
    )

    @Composable
    fun animated(index: Int, count: Int): Shape {
        val top by androidx.compose.animation.core.animateDpAsState(if (index == 0) Outer else Inner, Springs.spatial(), label = "segTop")
        val bottom by androidx.compose.animation.core.animateDpAsState(if (index == count - 1) Outer else Inner, Springs.spatial(), label = "segBottom")
        return RoundedCornerShape(top.coerceAtLeast(0.dp), top.coerceAtLeast(0.dp), bottom.coerceAtLeast(0.dp), bottom.coerceAtLeast(0.dp))
    }
}

@Composable
fun SegmentDivider() {
    Spacer(Modifier.height(Segments.Gap))
}

@Composable
fun CircleIconButton(
    icon: ImageVector,
    contentDescription: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    accent: Boolean = false,
    size: Dp = 40.dp,
    container: Color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceContainerHigh,
    tint: Color = if (accent) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface
) {
    FilledIconButton(
        onClick = hapticOnClick(onClick = onClick),
        modifier = modifier.minimumInteractiveComponentSize().size(size),
        shapes = IconButtonDefaults.shapes(),
        colors = IconButtonDefaults.filledIconButtonColors(containerColor = container, contentColor = tint)
    ) {
        Icon(icon, contentDescription, modifier = Modifier.size(size * 0.6f))
    }
}

@Composable
fun PillButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    accent: Boolean = true,
    enabled: Boolean = true,
    height: Dp = ButtonDefaults.MediumContainerHeight,
    container: Color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondaryContainer,
    contentColor: Color = if (accent) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer,
    haptic: Haptic = Haptic.Tap,
    padding: Dp? = null,
    leading: (@Composable () -> Unit)? = null
) {
    val src = remember { MutableInteractionSource() }
    Button(
        onClick = hapticOnClick(haptic, onClick),
        modifier = modifier.remoteFocus(src, CircleShape).height(height),
        enabled = enabled,
        interactionSource = src,
        shapes = ButtonDefaults.shapes(),
        colors = ButtonDefaults.buttonColors(containerColor = container, contentColor = contentColor),
        contentPadding = padding?.let { PaddingValues(horizontal = it) } ?: ButtonDefaults.contentPaddingFor(height)
    ) {
        if (leading != null) {
            leading()
            Spacer(Modifier.width(ButtonDefaults.iconSpacingFor(height)))
        }
        Text(text, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
fun NoticeBlock(icon: ImageVector, text: String, modifier: Modifier = Modifier, error: Boolean = false) {
    val c = MaterialTheme.colorScheme
    Row(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.largeIncreased)
            .background(if (error) c.errorContainer else c.secondaryContainer)
            .padding(horizontal = 20.dp, vertical = 16.dp),
        verticalAlignment = Alignment.Top
    ) {
        Icon(icon, null, tint = if (error) c.onErrorContainer else c.primary, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = if (error) c.onErrorContainer else c.onSecondaryContainer, style = MaterialTheme.typography.bodyMediumEmphasized)
    }
}

@Composable
fun Chip(
    text: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    container: Color = MaterialTheme.colorScheme.secondaryContainer,
    content: Color = MaterialTheme.colorScheme.onSecondaryContainer
) {
    Row(
        modifier.heightIn(min = 32.dp).clip(MaterialTheme.shapes.small).background(container).padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (icon != null) Icon(icon, null, tint = content, modifier = Modifier.size(16.dp))
        Text(text, color = content, style = MaterialTheme.typography.labelLarge, maxLines = 1)
    }
}

@Composable
fun navBottom(): Dp = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
