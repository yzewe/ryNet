@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import dev.yzewe.rynet.data.AppIcon
import dev.yzewe.rynet.tr

@Composable
internal fun AppIconImage(icon: AppIcon, size: Dp, modifier: Modifier = Modifier) {
    val ctx = LocalContext.current
    val px = with(LocalDensity.current) { size.roundToPx() }
    val bmp: ImageBitmap? = remember(icon, px) {
        runCatching { ctx.getDrawable(icon.res)?.toBitmap(px, px)?.asImageBitmap() }.getOrNull()
    }
    if (bmp != null) {
        Image(bmp, contentDescription = null, modifier = modifier.size(size).clip(CircleShape))
    } else {
        Box(modifier.size(size))
    }
}

@Composable
fun IconSheet(onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    val current = remember { AppIcon.current(ctx) }
    var picked by remember { mutableStateOf(current) }
    var close by remember { mutableLongStateOf(0L) }
    AppSheet(
        title = tr("Иконка", "Icon"),
        onDismiss = onDismiss,
        dismissRequestId = close,
        footer = {
            PillButton(
                if (picked == current) tr("Оставить такую", "Keep this one") else tr("Поставить", "Apply"),
                {
                    if (picked != current) AppIcon.apply(ctx, picked)
                    close++
                },
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp),
                haptic = if (picked == current) Haptic.Tap else Haptic.Confirm
            )
        }
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            AnimatedContent(
                targetState = picked,
                transitionSpec = { (fadeIn() + scaleIn(initialScale = 0.9f)) togetherWith fadeOut() },
                label = "preview"
            ) { icon ->
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    AppIconImage(icon, 112.dp)
                    Spacer(Modifier.height(12.dp))
                    Text(icon.title, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onSurface)
                }
            }
            Spacer(Modifier.height(20.dp))
            AppIcon.entries.chunked(4).forEach { row ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEach { icon ->
                        val bg by animateColorAsState(if (icon == picked) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh, label = "tile")
                        Column(
                            Modifier.weight(1f)
                                .clip(MaterialTheme.shapes.largeIncreased)
                                .background(bg)
                                .clickable { picked = icon }
                                .padding(vertical = 12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            AppIconImage(icon, 56.dp)
                            Spacer(Modifier.height(8.dp))
                            Text(
                                icon.title,
                                style = MaterialTheme.typography.labelMedium,
                                color = if (icon == picked) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.padding(horizontal = 4.dp)
                            )
                        }
                    }
                    repeat(4 - row.size) { Spacer(Modifier.weight(1f)) }
                }
                Spacer(Modifier.height(8.dp))
            }
            Spacer(Modifier.height(8.dp))
        }
    }
}
