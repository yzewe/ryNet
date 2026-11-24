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

internal fun wave(time: Float, delay: Float, length: Float, depth: Float): Float {
    if (time <= 0f || time >= 1f) return 1f
    val t = ((time - delay) / length).coerceIn(0f, 1f)
    return 1f + depth * sin(2f * PI.toFloat() * t) * (1f - t)
}

private object GridPaths {
    val squares by lazy { listOf(rect(3f, 3f, 11f, 11f), rect(13f, 3f, 21f, 11f), rect(3f, 13f, 11f, 21f), rect(13f, 13f, 21f, 21f)) }
    val centers = listOf(Offset(7f, 7f), Offset(17f, 7f), Offset(7f, 17f), Offset(17f, 17f))
}

internal val GridLive = Live(
    glyph = 0f,
    rest = { f -> floatArrayOf(f, f, f, f) },
    toggle = { on, p ->
        if (on) launch { p.e.dip(0.6f, ms = 90, damping = 0.35f, stiffness = 420f) }
        delay(if (on) 70L else 0L)
        for ((i, ch) in listOf(p.a, p.b, p.d, p.c).withIndex()) {
            launch {
                delay(i * 60L)
                ch.animateTo(if (on) 1f else 0f, spring(dampingRatio = 1f, stiffness = 380f))
            }
        }
    },
    tap = { p ->
        p.fill.play(600)
        p.fill.snapTo(0f)
    },
    draw = { p, tint ->
        val fills = floatArrayOf(p.a.value, p.b.value, p.c.value, p.d.value)
        val order = intArrayOf(0, 1, 3, 2)
        val pull = 1.1f * (p.press.value + p.e.value)
        for (i in 0..3) {
            val c = GridPaths.centers[i]
            val dx = if (c.x < 12f) pull else -pull
            val dy = if (c.y < 12f) pull else -pull
            val k = (1f + 0.16f * hump(fills[i])) * wave(p.fill.value, order[i] * 0.12f, 0.5f, -0.25f)
            translate(dx, dy) {
                scale(k, k, c) { ink(GridPaths.squares[i], tint, fills[i], w = 2f, r = 4f) }
            }
        }
    }
)

private object SwapPaths {
    private val parts by lazy { Glyph.Swap.subpaths() }
    val left get() = parts[0]
    val right get() = parts[1]
    val center = Offset(12f, 12f)
}

internal val SwapLive = Live(
    toggle = { on, p ->
        if (on) {
            swapPlaces(p)
        } else {
            launch { p.c.dip(1.8f, ms = 80, damping = 0.5f, stiffness = 600f) }
        }
    },
    tap = { p -> swapPlaces(p) },
    draw = { p, tint ->
        val back = 2f * p.press.value + p.c.value
        rotate(180f * p.a.value, SwapPaths.center) { translate(-back, 0f) { drawPath(SwapPaths.right, tint) } }
        rotate(180f * p.b.value, SwapPaths.center) { translate(back, 0f) { drawPath(SwapPaths.left, tint) } }
    }
)

private suspend fun swapPlaces(p: Parts) = coroutineScope {
    p.c.animateTo(2f, tween(90, easing = Decel))
    launch { p.c.animateTo(0f, spring(dampingRatio = 0.6f, stiffness = 500f)) }
    val top = launch { p.a.animateTo(1f, spring(dampingRatio = 0.55f, stiffness = 260f)) }
    delay(60)
    p.b.animateTo(1f, spring(dampingRatio = 0.55f, stiffness = 260f))
    top.join()
    p.a.snapTo(0f)
    p.b.snapTo(0f)
}

private object RoutePaths {
    val leftHead by lazy { path("M10,4H4v6z") }
    val rightHead by lazy { path("M14,4h6v6z") }
    val joint = Offset(12f, 12f)
}

internal val RouteLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.d.animateTo(-30f, tween(120, easing = Decel)) }
        p.b.animateTo(1f, tween(120, easing = Accel))
        launch { p.d.animateTo(0f, spring(dampingRatio = 0.32f, stiffness = 260f)) }
        launch {
            delay(90)
            p.a.kick(70f, damping = 0.4f, stiffness = 380f)
        }
        p.b.animateTo(0f, spring(dampingRatio = 0.55f, stiffness = 420f))
    },
    draw = { p, tint ->
        val grow = 1f - (p.b.value + 0.35f * p.press.value).coerceIn(0f, 1f)
        val ext = p.a.value * 0.7071f
        val j = RoutePaths.joint
        drawLine(tint, j, Offset(12f, 20f), strokeWidth = 2f)
        rotate(p.d.value - 12f * p.press.value, j) {
            drawCircle(tint, 1f, j)
            if (grow > 0.01f) {
                val lEnd = Offset(j.x + (6.6f - ext - j.x) * grow, j.y + (6.6f - ext - j.y) * grow)
                drawLine(tint, j, lEnd, strokeWidth = 2f)
                val rIn = Offset(j.x + (14.12f - j.x) * grow, j.y + (9.88f - j.y) * grow)
                val rEnd = Offset(j.x + (17.4f + ext - j.x) * grow, j.y + (6.6f - ext - j.y) * grow)
                drawLine(tint, rIn, rEnd, strokeWidth = 2f)
                val k = grow.coerceAtMost(1.2f)
                translate(-ext * grow, -ext * grow) { scale(k, k, j) { drawPath(RoutePaths.leftHead, tint) } }
                translate(ext * grow, -ext * grow) { scale(k, k, j) { drawPath(RoutePaths.rightHead, tint) } }
            }
        }
    }
)

private object PalettePaths {
    val body by lazy { Glyph.Palette.subpaths()[0] }
    val dots = listOf(Offset(6.5f, 10.5f), Offset(9.5f, 6.5f), Offset(14.5f, 6.5f), Offset(17.5f, 10.5f))
}

internal val PaletteLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.e.kick(-260f, damping = 0.4f, stiffness = 380f) }
        for ((i, ch) in listOf(p.a, p.b, p.c, p.d).withIndex()) {
            launch {
                delay(i * 60L)
                ch.play(380)
            }
        }
    },
    draw = { p, tint ->
        val glow = floatArrayOf(p.a.value, p.b.value, p.c.value, p.d.value)
        rotate(-24f * p.press.value + p.e.value, Offset(12f, 13f)) {
            layer {
                drawPath(PalettePaths.body, tint)
                for (i in 0..3) {
                    val g = hump(glow[i])
                    val r = 1.5f * (1f + 0.6f * g - 0.35f * p.press.value)
                    val center = PalettePaths.dots[i]
                    drawCircle(Black, (r + 0.9f * g).coerceAtLeast(0.3f), center, blendMode = BlendMode.DstOut)
                    if (g > 0f) drawCircle(tint, r * g, center, alpha = g)
                }
            }
        }
    }
)

internal val SortLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        val bars = listOf(p.a to -12f, p.b to -5f, p.c to 12f)
        for ((i, bar) in bars.withIndex()) {
            launch {
                delay(i * 50L)
                bar.first.animateTo(bar.second, spring(dampingRatio = 0.55f, stiffness = 420f))
            }
        }
        delay(330)
        for ((i, bar) in bars.reversed().withIndex()) {
            launch {
                delay(i * 50L)
                bar.first.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 380f))
            }
        }
    },
    draw = { p, tint ->
        val squeeze = 1f - 0.3f * p.press.value
        fun bar(y: Float, length: Float) = drawRect(tint, Offset(3f, y - 1f), Size((length * squeeze).coerceIn(0.5f, 20.5f), 2f))
        bar(7f, 18f + p.a.value)
        bar(12f, 12f + p.b.value)
        bar(17f, 6f + p.c.value)
    }
)

private object GlobePaths {
    private val glyph by lazy { Glyph.Globe.subpaths().reduce { a, b -> a.apply { addPath(b) } } }
    val disc by lazy { Path().apply { addOval(Rect(Offset(12f, 12f), 8f)) } }
    val ring by lazy { Path.combine(PathOperation.Difference, glyph, disc) }
    val land by lazy { Path.combine(PathOperation.Intersect, glyph, disc) }
}

internal val GlobeLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        p.a.snapTo(0f)
        p.a.animateTo(1f, spring(dampingRatio = 0.7f, stiffness = 90f))
        p.a.snapTo(0f)
    },
    draw = { p, tint ->
        val k = 1f - 0.12f * p.press.value
        scale(k, k, Offset(12f, 12f)) {
            drawPath(GlobePaths.ring, tint)
            val dx = (16f * p.a.value).mod(16f)
            layer {
                translate(dx, 0f) { drawPath(GlobePaths.land, tint) }
                if (dx > 0.01f) translate(dx - 16f, 0f) { drawPath(GlobePaths.land, tint) }
                keepInside(GlobePaths.disc)
            }
        }
    }
)

private object RulerPaths {
    val body by lazy { roundRect(1f, 6f, 23f, 18f, 2f) }
}

internal val RulerLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.a.play(560) }
        p.b.animateTo(0.07f, tween(110, easing = Decel))
        p.b.animateTo(0f, spring(dampingRatio = 0.4f, stiffness = 420f))
    },
    draw = { p, tint ->
        val squeeze = 0.08f * p.press.value
        val w = p.a.value
        scale(1f + p.b.value - squeeze, 1f - squeeze, Offset(12f, 12f)) {
            layer {
                drawPath(RulerPaths.body, tint)
                val hole = Path().apply {
                    moveTo(21f, 16f)
                    lineTo(3f, 16f)
                    lineTo(3f, 8f)
                    for (i in 0..3) {
                        val x = 5f + 4f * i
                        val len = 4f + if (w > 0f && w < 1f) 2.4f * hump(((w - i * 0.15f) / 0.55f).coerceIn(0f, 1f)) else 0f
                        lineTo(x, 8f)
                        lineTo(x, 8f + len)
                        lineTo(x + 2f, 8f + len)
                        lineTo(x + 2f, 8f)
                    }
                    lineTo(21f, 8f)
                    close()
                }
                drawPath(hole, Black, blendMode = BlendMode.DstOut)
            }
        }
    }
)

private object FlagPaths {
    val pole by lazy { rect(5f, 4f, 7f, 21f) }
    val outline = listOf(
        Offset(7f, 4f),
        Offset(14f, 4f),
        Offset(14.4f, 6f),
        Offset(20f, 6f),
        Offset(20f, 16f),
        Offset(13f, 16f),
        Offset(12.6f, 14f),
        Offset(7f, 14f)
    )
}

private fun cloth(amp: Float, phase: Float, lift: Float): Path {
    fun at(o: Offset): Offset {
        val k = (o.x - 7f) / 13f
        return Offset(o.x, o.y + lift * (1f - 0.3f * k) + amp * k * sin(phase - (o.x - 7f) * 0.55f))
    }
    val pts = FlagPaths.outline
    return Path().apply {
        val s = at(pts[0])
        moveTo(s.x, s.y)
        for (i in 1..pts.size) {
            val a = pts[i - 1]
            val b = pts[i % pts.size]
            val n = if (abs(b.x - a.x) > 1f) 6 else 1
            for (j in 1..n) at(a + (b - a) * (j / n.toFloat())).let { lineTo(it.x, it.y) }
        }
        close()
    }
}

internal val FlagLive = Live(
    toggle = { on, p ->
        p.b.snapTo(0f)
        launch { p.b.animateTo(if (on) 4f * PI.toFloat() else 2f * PI.toFloat(), tween(if (on) 760 else 480, easing = LinearEasing)) }
        launch {
            p.a.snapTo(0f)
            p.a.animateTo(if (on) 1.5f else 0.8f, tween(120, easing = Decel))
            p.a.animateTo(0f, tween(if (on) 640 else 360, easing = FastOutSlowInEasing))
        }
        if (on) {
            p.c.animateTo(2.4f, tween(100, easing = Decel))
            p.c.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 380f))
        } else {
            p.c.dip(1.4f, ms = 140, damping = 0.55f, stiffness = 320f)
        }
    },
    tap = { p ->
        p.b.snapTo(0f)
        launch { p.b.animateTo(2f * PI.toFloat(), tween(520, easing = LinearEasing)) }
        p.a.animateTo(1f, tween(110, easing = Decel))
        p.a.animateTo(0f, tween(420, easing = FastOutSlowInEasing))
    },
    draw = { p, tint ->
        drawPath(FlagPaths.pole, tint)
        val still = p.a.value == 0f && p.c.value == 0f && p.press.value == 0f
        drawPath(if (still) FlagStill else cloth(p.a.value, p.b.value, p.c.value + 1.2f * p.press.value), tint)
    }
)

private val FlagStill by lazy { cloth(0f, 0f, 0f) }

private object WifiPaths {
    private val parts by lazy { Glyph.Wifi.subpaths() }
    val big get() = parts[0]
    val dot get() = parts[1]
    val mid get() = parts[2]
    val pivot = Offset(12f, 20f)
}

internal val WifiLive = Live(
    toggle = { on, p ->
        val parts = listOf(p.a, p.b, p.c)
        if (on) {
            coroutineScope { for (ch in parts) launch { ch.animateTo(0.75f, tween(70, easing = Accel)) } }
            for ((i, ch) in parts.withIndex()) {
                launch {
                    delay(i * 80L)
                    ch.animateTo(0f, spring(dampingRatio = 0.5f, stiffness = 480f))
                }
            }
        } else {
            for ((i, ch) in parts.reversed().withIndex()) {
                launch {
                    delay(i * 70L)
                    ch.dip(0.6f, ms = 110, damping = 0.6f, stiffness = 420f)
                }
            }
        }
    },
    tap = { p ->
        for ((i, ch) in listOf(p.a, p.b, p.c).withIndex()) {
            launch {
                delay(i * 60L)
                ch.dip(0.35f, ms = 90, damping = 0.45f, stiffness = 480f)
            }
        }
    },
    draw = { p, tint ->
        val k = 1f - 0.1f * p.press.value
        scale(k, k, WifiPaths.pivot) {
            fun part(path: Path, hidden: Float) {
                val s = 1f - 0.4f * hidden
                scale(s, s, WifiPaths.pivot) { drawPath(path, tint, alpha = (1f - hidden).coerceIn(0f, 1f)) }
            }
            part(WifiPaths.dot, p.a.value)
            part(WifiPaths.mid, p.b.value)
            part(WifiPaths.big, p.c.value)
        }
    }
)

private object TimerPaths {
    val center = Offset(12f, 13f)
    val disc by lazy { Path().apply { addOval(Rect(center, 9f)) } }
    val hand by lazy { rect(11f, 8f, 13f, 14f) }

    val slashFrom = Offset(2.11f, 3.52f)
    val slashTo = Offset(20.49f, 21.89f)
}

private fun DrawScope.stopwatch(tint: Color, button: Float) {
    drawCircle(tint, 8f, TimerPaths.center, style = Stroke(2f))
    drawRect(tint, Offset(9f, 1f + button), Size(6f, 2f))
    drawLine(tint, Offset(18.33f, 6.69f), Offset(19.75f, 5.27f), strokeWidth = 2f)
}

internal val TimerLive = Live(
    glyph = 0f,
    toggle = { on, p ->
        if (on) {
            p.b.animateTo(1.3f, tween(80, easing = Decel))
            launch { p.b.animateTo(0f, spring(dampingRatio = 0.4f, stiffness = 520f)) }
            launch { p.fill.animateTo(1f, tween(360, easing = Decel)) }
            launch {
                delay(120)
                p.c.play(420)
            }
            p.a.animateTo(360f, spring(dampingRatio = 0.55f, stiffness = 170f))
            p.a.snapTo(0f)
        } else {
            launch { p.fill.animateTo(0f, tween(260, easing = Accel)) }
            launch { p.b.dip(0.9f, ms = 80, damping = 0.5f, stiffness = 520f) }
            p.a.animateTo(-90f, tween(140, easing = Accel))
            p.a.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 320f))
        }
    },
    tap = { p ->
        p.b.dip(1.1f, ms = 80, damping = 0.4f, stiffness = 520f)
    },
    draw = { p, tint ->
        val c = TimerPaths.center
        echo(tint, c, p.c.value, 9f, 11.4f)
        val k = 1f - 0.08f * p.press.value
        scale(k, k, c) {
            val f = p.fill.value.coerceIn(0f, 1f)
            layer {
                if (f > 0f) {
                    if (f >= 0.999f) {
                        drawPath(TimerPaths.disc, tint)
                    } else {
                        layer {
                            drawCircle(tint, 9.5f * f, c)
                            keepInside(TimerPaths.disc)
                        }
                    }
                }
                rotate(p.a.value + 25f * p.press.value, c) { drawPath(TimerPaths.hand, tint, blendMode = BlendMode.Xor) }
            }
            stopwatch(tint, p.b.value + 1.1f * p.press.value)
        }
    }
)

internal val TimerOffLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.c.play(460) }
        delay(60)
        p.a.kick(260f, damping = 0.3f, stiffness = 520f)
    },
    draw = { p, tint ->
        val c = TimerPaths.center
        val t = p.c.value
        val s = if (t > 0f && t < 1f) abs(1f - 2f * t) else 1f
        val end = TimerPaths.slashFrom + (TimerPaths.slashTo - TimerPaths.slashFrom) * s
        val k = 1f - 0.08f * p.press.value
        scale(k, k, c) {
            layer {
                rotate(p.a.value, c) {
                    stopwatch(tint, 0f)
                    drawPath(TimerPaths.hand, tint)
                }
                if (s > 0.01f) drawLine(Black, TimerPaths.slashFrom, end, strokeWidth = 5f, blendMode = BlendMode.DstOut)
            }
            if (s > 0.01f) drawLine(tint, TimerPaths.slashFrom, end, strokeWidth = 2f)
        }
    }
)

private fun DrawScope.pingArcs(tint: Color, r: Float, width: Float = 2f, alpha: Float = 1f) {
    val tl = Offset(12f - r, 12f - r)
    val sz = Size(2f * r, 2f * r)
    drawArc(tint, -45f, 90f, false, tl, sz, alpha = alpha, style = Stroke(width))
    drawArc(tint, 135f, 90f, false, tl, sz, alpha = alpha, style = Stroke(width))
}

internal val PingLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.a.dip(-0.35f, ms = 70, damping = 0.4f, stiffness = 520f) }
        for ((i, ch) in listOf(p.b, p.d).withIndex()) {
            launch {
                delay(50L + i * 70L)
                ch.dip(1.3f, ms = 90, damping = 0.42f, stiffness = 460f)
            }
        }
        delay(150)
        p.c.play(460)
    },
    draw = { p, tint ->
        val t = p.c.value
        if (t > 0f && t < 1f) pingArcs(tint, 9.2f + 2.4f * easeOut(t), width = 1.6f * (1f - 0.5f * t), alpha = 0.7f * (1f - t))
        val pull = 1f - 0.14f * p.press.value
        drawCircle(tint, 2f * (1f + p.a.value), Offset(12f, 12f))
        pingArcs(tint, 5f * pull + p.b.value)
        pingArcs(tint, 9f * pull + p.d.value)
    }
)

private object IpCheckPaths {
    val globe by lazy { Glyph.IpCheck.subpaths()[2] }
    val lens = Offset(15.52f, 14.48f)
}

internal val IpCheckLive = Live(
    toggle = { _, _ -> },
    tap = { p ->
        launch { p.c.play(620) }
        delay(480)
        p.b.dip(0.14f, ms = 90, damping = 0.4f, stiffness = 460f)
    },
    draw = { p, tint ->
        val t = p.c.value
        val r = 1.7f * hump(t)
        val angle = 2f * PI.toFloat() * t - PI.toFloat() / 2f
        val press = 1.2f * p.press.value
        val at = IpCheckPaths.lens + Offset(r * cos(angle) - press, r * sin(angle) - press)
        val k = 1f + p.b.value
        layer {
            drawPath(IpCheckPaths.globe, tint)
            drawCircle(Black, 5.2f * k, at, blendMode = BlendMode.DstOut)
        }
        scale(k, k, at) {
            drawCircle(tint, 3.5f, at, style = Stroke(2f))
            drawLine(tint, at + Offset(3.09f, 3.12f), at + Offset(6.28f, 6.32f), strokeWidth = 2f)
        }
    }
)
