package dev.yzewe.rynet.ui

import android.annotation.SuppressLint
import android.content.Context
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp

@Composable
fun FlagIcon(
    emoji: String,
    modifier: Modifier,
    shape: Shape = RoundedCornerShape(4.dp),
    outline: Boolean = true
) {
    val ctx = LocalContext.current
    val fade = MaterialTheme.motionScheme.defaultEffectsSpec<Float>()
    val grow = MaterialTheme.motionScheme.defaultSpatialSpec<Float>()
    AnimatedContent(emoji, modifier, transitionSpec = { (fadeIn(fade) + scaleIn(grow, 0.85f)) togetherWith fadeOut(fade) }, label = "flag") { e ->
        val id = remember(e) { flagRes(ctx, e) }
        if (id != 0) {
            Image(
                painterResource(id),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = if (outline) {
                    Modifier.fillMaxSize().clip(shape).border(0.5.dp, MaterialTheme.colorScheme.outlineVariant, shape)
                } else {
                    Modifier.fillMaxSize().clip(shape)
                }
            )
        } else {
            BoxWithConstraints(Modifier.fillMaxSize().clip(shape).background(MaterialTheme.colorScheme.secondaryContainer), contentAlignment = Alignment.Center) {
                Icon(Glyph.Globe, contentDescription = null, tint = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.size(minOf(maxWidth, maxHeight) * 0.7f))
            }
        }
    }
}

private val cache = HashMap<String, Int>()

@SuppressLint("DiscouragedApi")
private fun flagRes(ctx: Context, emoji: String): Int {
    val cps = emoji.codePoints().toArray()
    if (cps.size != 2 || cps.any { it !in 0x1F1E6..0x1F1FF }) return 0
    val code = cps.joinToString("") { ('a' + (it - 0x1F1E6)).toString() }
    return synchronized(cache) {
        cache.getOrPut(code) { ctx.resources.getIdentifier("flag_$code", "drawable", ctx.packageName) }
    }
}
