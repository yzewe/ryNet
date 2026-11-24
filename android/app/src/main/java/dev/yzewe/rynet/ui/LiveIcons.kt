package dev.yzewe.rynet.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathOperation
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathNode
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.graphics.vector.VectorGroup
import androidx.compose.ui.graphics.vector.VectorPath
import androidx.compose.ui.graphics.vector.toPath
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.sin
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun LiveIcon(
    icon: ImageVector,
    checked: Boolean?,
    source: InteractionSource,
    tint: Color,
    modifier: Modifier = Modifier,
    size: Dp = 24.dp
) {
    val live = remember(icon) { liveFor(icon) }
    if (live == null) {
        Icon(icon, contentDescription = null, tint = tint, modifier = modifier.iconMotion(source, motionFor(icon)).popOnChange(checked).size(size))
        return
    }
    val parts = remember(live) { Parts(live, checked) }
    var last by remember(live) { mutableStateOf(checked) }
    LaunchedEffect(live, checked) {
        if (checked == last) return@LaunchedEffect
        last = checked
        if (checked == null) {
            parts.fill.snapTo(live.glyph)
        } else {
            if (parts.busy) parts.calm(live)
            parts.busy = true
            coroutineScope { live.toggle(this, checked, parts) }
            parts.busy = false
        }
    }
    val toggles by rememberUpdatedState(checked != null)
    LaunchedEffect(live, source) {
        var release: Job? = null
        source.interactions.collect { i ->
            when (i) {
                is PressInteraction.Press -> {
                    release?.cancel()
                    launch { parts.press.animateTo(1f, spring(dampingRatio = 1f, stiffness = 900f)) }
                }

                is PressInteraction.Release -> release = launch {
                    if (parts.press.value < 0.6f) parts.press.animateTo(1f, tween(80))
                    if (!toggles) launch { live.tap(this, parts) }
                    parts.press.animateTo(0f, spring(dampingRatio = 0.32f, stiffness = 420f))
                }

                is PressInteraction.Cancel -> release = launch { parts.press.animateTo(0f, Springs.pressOut()) }
            }
        }
    }
    Canvas(modifier.size(size)) {
        val s = this.size.minDimension / 24f
        scale(s, s, pivot = Offset.Zero) { clipRect(0f, 0f, 24f, 24f) { live.draw(this, parts, tint) } }
    }
}

private typealias Channel = Animatable<Float, AnimationVector1D>

internal class Parts(live: Live, checked: Boolean?) {
    val fill = Animatable(
        when (checked) {
            null -> live.glyph
            true -> 1f
            false -> 0f
        }
    )
    val press = Animatable(0f)
    private val rest = live.rest(fill.value)
    val a = Animatable(rest[0])
    val b = Animatable(rest[1])
    val c = Animatable(rest[2])
    val d = Animatable(rest[3])
    val e = Animatable(0f)
    var busy = false

    suspend fun calm(live: Live) {
        val r = live.rest(fill.value)
        a.snapTo(r[0])
        b.snapTo(r[1])
        c.snapTo(r[2])
        d.snapTo(r[3])
        e.snapTo(0f)
    }
}

private val Still = floatArrayOf(0f, 0f, 0f, 0f)

internal class Live(
    val glyph: Float = 1f,
    val rest: (Float) -> FloatArray = { Still },
    val toggle: suspend CoroutineScope.(Boolean, Parts) -> Unit,
    val tap: suspend CoroutineScope.(Parts) -> Unit = {},
    val draw: DrawScope.(Parts, Color) -> Unit
)

private fun liveFor(icon: ImageVector): Live? = when (icon) {
    Glyph.Power -> PowerLive

    Glyph.Battery -> BatteryLive

    Glyph.Bolt -> BoltLive

    Glyph.Link -> LinkLive

    Glyph.Cut -> CutLive

    Glyph.Lock -> LockLive

    Glyph.Wallpaper -> WallpaperLive

    Glyph.Pulse -> PulseLive

    Glyph.Upload -> UploadLive

    Glyph.Pin -> PinLive

    Glyph.Speed -> SpeedLive

    Glyph.Data -> DataLive

    Glyph.Apps -> AppsLive

    Glyph.Grid -> GridLive

    Glyph.Swap -> SwapLive

    Glyph.Route -> RouteLive

    Glyph.Palette -> PaletteLive

    Glyph.Sort -> SortLive

    Glyph.Globe -> GlobeLive

    Glyph.Ruler -> RulerLive

    Glyph.Flag -> FlagLive

    Glyph.Wifi -> WifiLive

    Glyph.Timer -> TimerLive

    Glyph.TimerOff -> TimerOffLive

    Glyph.Ping -> PingLive

    Glyph.IpCheck -> IpCheckLive

    else -> when (icon.name) {
        "Rounded.Notifications" -> BellLive
        "Rounded.Favorite" -> HeartLive
        "AutoMirrored.Rounded.Send" -> SendLive
        "Rounded.Refresh" -> RefreshLive
        else -> null
    }
}

internal fun path(d: String): Path = PathParser().parsePathString(d).toPath()

internal fun ImageVector.subpaths(): List<Path> {
    fun first(g: VectorGroup): VectorPath? {
        for (n in g) {
            when (n) {
                is VectorPath -> return n
                is VectorGroup -> first(n)?.let { return it }
            }
        }
        return null
    }
    val parts = mutableListOf<MutableList<PathNode>>()
    for (n in first(root)!!.pathData) {
        if (n is PathNode.MoveTo || parts.isEmpty()) parts += mutableListOf<PathNode>()
        parts.last() += n
    }
    return parts.map { it.toPath() }
}

internal fun roundRect(l: Float, t: Float, r: Float, b: Float, radius: Float) = Path().apply { addRoundRect(RoundRect(l, t, r, b, CornerRadius(radius))) }

internal fun rect(l: Float, t: Float, r: Float, b: Float) = Path().apply { addRect(Rect(l, t, r, b)) }

internal val Black = Color.Black

internal val FillSpring = spring<Float>(dampingRatio = 1f, stiffness = 320f)

internal val FastSpatial = spring<Float>(dampingRatio = 0.6f, stiffness = 800f)

internal val MainSpatial = spring<Float>(dampingRatio = 0.62f, stiffness = 380f)

internal val Decel = Springs.EmphasizedDecelerate

internal val Accel = Springs.EmphasizedAccelerate

internal inline fun DrawScope.layer(block: DrawScope.() -> Unit) {
    drawContext.canvas.saveLayer(Rect(-8f, -8f, 32f, 32f), Paint())
    block()
    drawContext.canvas.restore()
}

private val outsides = HashMap<Path, Path>()

private val Everything by lazy { rect(-8f, -8f, 32f, 32f) }

internal fun DrawScope.keepInside(shape: Path) {
    val outside = outsides.getOrPut(shape) { Path.combine(PathOperation.Difference, Everything, shape) }
    drawPath(outside, Black, blendMode = BlendMode.DstOut)
}

internal fun DrawScope.ink(shape: Path, tint: Color, fill: Float, w: Float = 2f, r: Float = 6f) {
    val f = fill.coerceIn(0f, 1f)
    if (f >= 0.999f) {
        drawPath(shape, tint)
        return
    }
    layer {
        drawPath(shape, tint, style = Stroke(width = 2f * (w + f * (r - w + 0.5f)), join = StrokeJoin.Round))
        keepInside(shape)
    }
}

internal fun hump(t: Float): Float = if (t <= 0f || t >= 1f) 0f else sin(PI.toFloat() * t)

internal fun easeOut(t: Float): Float = 1f - (1f - t).let { it * it * it }

internal fun rad(deg: Float) = deg * PI.toFloat() / 180f

internal fun DrawScope.spark(tint: Color, from: Offset, dx: Float, dy: Float, t: Float, length: Float = 2.2f, travel: Float = 1.6f) {
    if (t <= 0f || t >= 1f) return
    val start = travel * easeOut(t)
    val len = length * hump(t)
    drawLine(
        tint,
        Offset(from.x + dx * start, from.y + dy * start),
        Offset(from.x + dx * (start + len), from.y + dy * (start + len)),
        strokeWidth = 1.5f,
        cap = StrokeCap.Round,
        alpha = (1f - t * t).coerceIn(0f, 1f)
    )
}

internal fun DrawScope.burst(tint: Color, center: Offset, t: Float, from: Float, to: Float, angles: FloatArray, length: Float = 1.6f) {
    if (t <= 0f || t >= 1f) return
    for (a in angles) spark(tint, Offset(center.x + cos(rad(a)) * from, center.y + sin(rad(a)) * from), cos(rad(a)), sin(rad(a)), t, length, to - from)
}

internal fun DrawScope.echo(tint: Color, center: Offset, t: Float, from: Float, to: Float, width: Float = 1.4f) {
    if (t <= 0f || t >= 1f) return
    drawCircle(tint, from + (to - from) * easeOut(t), center, alpha = 0.7f * (1f - t), style = Stroke(width * (1f - 0.5f * t)))
}

internal fun DrawScope.particles(tint: Color, center: Offset, t: Float, angles: FloatArray, distance: Float, radius: Float = 0.8f) {
    if (t <= 0f || t >= 1f) return
    val r = distance * easeOut(t)
    for ((i, a) in angles.withIndex()) {
        val k = if (i % 2 == 0) 1f else 0.72f
        drawCircle(tint, radius * (1f - t * 0.6f), Offset(center.x + cos(rad(a)) * r * k, center.y + sin(rad(a)) * r * k), alpha = 1f - t * t)
    }
}

internal fun DrawScope.flash(tint: Color, center: Offset, t: Float, radius: Float, strength: Float = 0.35f) {
    val g = hump(t)
    if (g <= 0f) return
    drawCircle(Brush.radialGradient(0f to tint.copy(alpha = strength * g * tint.alpha), 1f to Color.Transparent, center = center, radius = radius), radius, center)
}

internal suspend fun Channel.kick(v: Float, damping: Float = 0.3f, stiffness: Float = 400f) = animateTo(0f, spring(dampingRatio = damping, stiffness = stiffness), initialVelocity = v)

internal suspend fun Channel.play(ms: Int) {
    snapTo(0f)
    animateTo(1f, tween(ms, easing = LinearEasing))
}

internal suspend fun Channel.dip(to: Float, ms: Int = 90, damping: Float = 0.38f, stiffness: Float = 420f) {
    animateTo(to, tween(ms, easing = Decel))
    animateTo(0f, spring(dampingRatio = damping, stiffness = stiffness))
}
