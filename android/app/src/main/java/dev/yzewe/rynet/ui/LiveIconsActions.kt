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

private object WallpaperPaths {
    private val parts by lazy { Glyph.Wallpaper.subpaths() }
    val topLeft get() = parts[0]
    val hills get() = parts[1]
    val sun get() = parts[2]
    val topRight get() = parts[3]
    val bottomRight get() = parts[4]
    val bottomLeft get() = parts[5]
    val sunCenter = Offset(15.5f, 8.5f)
    val sparkles = floatArrayOf(-160f, -115f, -70f, -25f, 15f)
}

internal val WallpaperLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.a.dip(1f, ms = 90, damping = 0.3f, stiffness = 380f) }
            launch { p.b.dip(0.5f, ms = 110, damping = 0.42f, stiffness = 300f) }
            p.c.animateTo(4.6f, tween(120, easing = Accel))
            launch {
                delay(170)
                launch { p.d.play(380) }
                p.e.dip(0.3f, ms = 90, damping = 0.4f, stiffness = 400f)
            }
            p.c.animateTo(0f, MainSpatial)
        } else {
            launch { p.b.dip(0.3f, ms = 120, damping = 0.5f, stiffness = 380f) }
            p.c.animateTo(3f, tween(170, easing = Accel))
            p.c.animateTo(0f, spring(dampingRatio = 0.55f, stiffness = 260f))
        }
    },
    draw = { p, tint ->
        val d = 2.2f * p.press.value + p.a.value
        translate(d, d) { drawPath(WallpaperPaths.topLeft, tint) }
        translate(-d, d) { drawPath(WallpaperPaths.topRight, tint) }
        translate(-d, -d) { drawPath(WallpaperPaths.bottomRight, tint) }
        translate(d, -d) { drawPath(WallpaperPaths.bottomLeft, tint) }
        particles(tint, WallpaperPaths.sunCenter, p.d.value, WallpaperPaths.sparkles, 5.6f, radius = 0.9f)
        clipRect(0f, 0f, 24f, 13.4f) {
            translate(0f, p.c.value) {
                val k = 1f + p.e.value
                scale(k, k, WallpaperPaths.sunCenter) { drawPath(WallpaperPaths.sun, tint) }
            }
        }
        scale(1f, 1f - p.b.value, Offset(12f, 18f)) { drawPath(WallpaperPaths.hills, tint) }
    }
)

private object PulsePoints {
    val x = floatArrayOf(2.75f, 9.5f, 13.5f, 21.3f)
    val y = floatArrayOf(17.74f, 10.98f, 14.98f, 6.2f)
}

internal val PulseLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.c.play(420) }
            launch { p.a.play(560) }
            p.b.animateTo(-0.35f, tween(80, easing = Decel))
            p.b.animateTo(0f, spring(dampingRatio = 0.35f, stiffness = 380f))
        } else {
            p.b.animateTo(-0.8f, tween(110, easing = Decel))
            p.b.animateTo(0f, spring(dampingRatio = 0.5f, stiffness = 500f))
        }
    },
    tap = { p ->
        launch { p.c.play(420) }
        p.a.play(560)
    },
    draw = { p, tint ->
        val k = 1f - 0.12f * p.press.value
        scale(k, k, Offset(12f, 12f)) { pulse(p, tint) }
    }
)

private fun DrawScope.pulse(p: Parts, tint: Color) {
    run {
        val base = 1f + p.b.value
        val xs = PulsePoints.x
        val w = p.a.value
        val ys = FloatArray(4) {
            val bump = if (w > 0f && w < 1f) 0.9f * hump(((w - it * 0.16f) / 0.5f).coerceIn(0f, 1f)) else 0f
            12f + (PulsePoints.y[it] - 12f) * (base + bump)
        }
        val line = Path().apply {
            moveTo(xs[0], ys[0])
            for (i in 1..3) lineTo(xs[i], ys[i])
        }
        drawPath(line, tint, style = Stroke(2.1f, join = StrokeJoin.Miter))
        val t = p.c.value
        if (t > 0f && t < 1f) {
            val lens = FloatArray(3) { hypot(xs[it + 1] - xs[it], ys[it + 1] - ys[it]) }
            var left = lens.sum() * easeOut(t)
            var i = 0
            while (i < 2 && left > lens[i]) left -= lens[i++]
            val k = (left / lens[i]).coerceIn(0f, 1f)
            val dot = Offset(xs[i] + (xs[i + 1] - xs[i]) * k, ys[i] + (ys[i + 1] - ys[i]) * k)
            drawCircle(tint, 2.2f * (0.6f + 0.4f * hump(t)), dot, alpha = (1.6f - 1.6f * t).coerceIn(0f, 1f))
        }
    }
}

private object UploadPaths {
    private val parts by lazy { Glyph.Upload.subpaths() }
    val tray get() = parts[0]
    val arrow get() = parts[1]
}

internal val UploadLive = Live(
    toggle = { on, p ->
        if (on) {
            launchArrow(p) { p.fill.snapTo(1f) }
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.e.dip(-0.15f, ms = 130, damping = 0.4f, stiffness = 420f) }
            p.a.dip(2.2f, ms = 130, damping = 0.45f, stiffness = 420f)
        }
    },
    tap = { p -> launchArrow(p) {} },
    draw = { p, tint ->
        translate(0f, p.b.value + 0.6f * p.press.value) { drawPath(UploadPaths.tray, tint) }
        clipRect(0f, 0f, 24f, 17.6f) {
            val y = p.a.value + 2f * p.press.value
            val t = p.c.value
            if (t > 0f && t < 1f) {
                val len = 4f * hump(t)
                for (x in floatArrayOf(6.6f, 17.4f)) drawLine(tint, Offset(x, 12f + y), Offset(x, 12f + y + len), strokeWidth = 1.4f, cap = StrokeCap.Round, alpha = 1f - t)
            }
            translate(0f, y) {
                scale(1f - 0.5f * p.e.value, 1f + p.e.value, Offset(12f, 16f)) { ink(UploadPaths.arrow, tint, p.fill.value, w = 2f, r = 3.2f) }
            }
        }
    }
)

private suspend fun launchArrow(p: Parts, swap: suspend () -> Unit) = coroutineScope {
    launch { p.a.animateTo(1.2f, tween(90, easing = Decel)) }
    p.e.animateTo(-0.14f, tween(90, easing = Decel))
    launch { p.e.animateTo(0.1f, tween(100)) }
    launch { p.b.kick(30f, damping = 0.38f, stiffness = 520f) }
    launch { p.c.play(300) }
    p.a.animateTo(-17f, tween(170, easing = Accel))
    swap()
    p.e.snapTo(0f)
    p.a.snapTo(12f)
    p.a.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 300f))
}

private object SendPaths {
    val plane by lazy { Icons.AutoMirrored.Rounded.Send.subpaths()[0] }
    val loop = Offset(12f, 7.6f)
    const val R = 4.4f
}

internal val SendLive = Live(
    toggle = { on, p ->
        if (on) {
            loopTheLoop(p) { p.fill.animateTo(1f, FillSpring) }
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.e.dip(22f, ms = 90, damping = 0.5f, stiffness = 600f) }
            p.b.dip(-2.2f, ms = 90, damping = 0.5f, stiffness = 600f)
        }
    },
    tap = { p -> loopTheLoop(p) {} },
    draw = { p, tint ->
        val t = p.a.value.coerceIn(0f, 1f)
        val press = p.press.value
        if (t > 0f && t < 0.85f) {
            for (k in 1..5) {
                val tk = t - k * 0.07f
                if (tk <= 0f) break
                val pos = loopPoint(tk)
                drawCircle(tint, 1f, pos, alpha = 0.75f * (1f - k / 6f))
            }
        }
        val pos = if (t > 0f && t < 1f) loopPoint(t) else Offset(12f, 12f)
        val k = 1f - 0.58f * hump(t)
        val heading = if (t > 0f && t < 1f) -360f * t else 0f
        translate(pos.x - 12f + p.b.value - 2.5f * press, pos.y - 12f) {
            rotate(heading + p.e.value, Offset(12f, 12f)) {
                scale(k * (1f - 0.12f * press), k, Offset(12f, 12f)) { ink(SendPaths.plane, tint, p.fill.value, w = 1.8f, r = 3.6f) }
            }
        }
    }
)

private fun loopPoint(t: Float): Offset {
    val th = rad(90f - 360f * t)
    return Offset(SendPaths.loop.x + SendPaths.R * cos(th), SendPaths.loop.y + SendPaths.R * sin(th) + (12f - SendPaths.loop.y - SendPaths.R))
}

private suspend fun loopTheLoop(p: Parts, fill: suspend () -> Unit) = coroutineScope {
    p.b.animateTo(-2.2f, tween(90, easing = Decel))
    launch { p.b.animateTo(0f, spring(dampingRatio = 0.6f, stiffness = 500f)) }
    launch {
        delay(260)
        fill()
    }
    p.a.snapTo(0f)
    p.a.animateTo(1f, tween(540, easing = FastOutSlowInEasing))
    p.a.snapTo(0f)
    launch { p.e.kick(-260f, damping = 0.35f, stiffness = 420f) }
    p.b.kick(26f, damping = 0.4f, stiffness = 500f)
}

private object RefreshPaths {
    val head by lazy { path("M18.29,5.7C18.92,5.07 20,5.52 20,6.41V10c0,0.55 -0.45,1 -1,1H15.41c-0.89,0 -1.33,-1.08 -0.7,-1.71z") }
}

internal val RefreshLive = Live(
    toggle = { on, p ->
        if (on) {
            p.a.animateTo(-30f, tween(100, easing = Decel))
            launch {
                p.b.animateTo(160f, tween(240, easing = Decel))
                p.b.animateTo(0f, spring(dampingRatio = 0.55f, stiffness = 220f))
            }
            p.a.animateTo(360f, spring(dampingRatio = 0.62f, stiffness = 150f))
            p.a.snapTo(0f)
        } else {
            launch { p.b.dip(60f, ms = 150, damping = 0.5f, stiffness = 300f) }
            p.a.dip(-60f, ms = 150, damping = 0.45f, stiffness = 260f)
        }
    },
    tap = { p ->
        launch { p.b.dip(120f, ms = 200, damping = 0.55f, stiffness = 220f) }
        p.a.snapTo(0f)
        p.a.animateTo(360f, spring(dampingRatio = 0.62f, stiffness = 150f))
        p.a.snapTo(0f)
    },
    draw = { p, tint ->
        val b = (p.b.value + 60f * p.press.value).coerceIn(0f, 240f)
        val angle = p.a.value - 30f * p.press.value
        val lag = (p.a.velocity * 0.035f).coerceIn(-60f, 60f)
        for ((k, alpha) in listOf(2f to 0.14f, 1f to 0.28f)) {
            if (abs(lag) > 4f) rotate(angle - lag * k, Offset(12f, 12f)) { drawPath(RefreshPaths.head, tint, alpha = alpha) }
        }
        rotate(angle, Offset(12f, 12f)) {
            drawArc(tint, 25f + b, 290f - b, false, Offset(5f, 5f), Size(14f, 14f), style = Stroke(2f, cap = StrokeCap.Round))
            drawPath(RefreshPaths.head, tint)
        }
    }
)

private object PinPaths {
    val head by lazy { path("M16,9V4h1c0.55,0 1,-0.45 1,-1s-0.45,-1 -1,-1H7C6.45,2 6,2.45 6,3s0.45,1 1,1h1v5c0,1.66 -1.34,3 -3,3v2h14v-2C17.34,12 16,10.66 16,9z") }
    val needle by lazy { path("M10.97,13.5h2v7.5l-1,1 -1,-1z") }
}

internal val PinLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.b.animateTo(-16f, tween(110, easing = Decel)) }
            p.a.animateTo(-2f, tween(110, easing = Decel))
            p.a.animateTo(0.9f, tween(100, easing = Accel))
            launch { p.b.animateTo(0f, spring(dampingRatio = 0.4f, stiffness = 420f)) }
            launch { p.c.dip(-0.2f, ms = 50, damping = 0.32f, stiffness = 520f) }
            launch { p.d.play(340) }
            launch { p.fill.animateTo(1f, tween(170, easing = Decel)) }
            p.a.animateTo(0f, FastSpatial)
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.c.dip(0.08f, ms = 120, damping = 0.4f, stiffness = 420f) }
            launch { p.b.dip(14f, ms = 150, damping = 0.35f, stiffness = 300f) }
            p.a.dip(-2.2f, ms = 150, damping = 0.42f, stiffness = 380f)
        }
    },
    draw = { p, tint ->
        val sink = p.a.value + 1.6f * p.press.value
        spark(tint, Offset(10f, 21.6f), -0.96f, -0.28f, p.d.value, length = 1.8f, travel = 1.4f)
        spark(tint, Offset(14f, 21.6f), 0.96f, -0.28f, p.d.value, length = 1.8f, travel = 1.4f)
        translate(0f, sink) {
            rotate(p.b.value, Offset(12f, 22f)) {
                clipRect(0f, -6f, 24f, 22.2f - sink) { drawPath(PinPaths.needle, tint) }
                scale(1f - 0.4f * p.c.value, 1f + p.c.value, Offset(12f, 14f)) { ink(PinPaths.head, tint, p.fill.value, w = 2f, r = 3.4f) }
            }
        }
    }
)

private object SpeedPaths {
    private val parts by lazy { Glyph.Speed.subpaths() }
    val gauge get() = parts[0]
    val needle get() = parts[1]
    val disc by lazy { path("M3.35,19a2,2 0,0 0,1.72 1h13.85a2,2 0,0 0,1.74 -1A10,10 0,1 0,3.35 19z") }
    val pivot = Offset(12f, 14f)
}

internal val SpeedLive = Live(
    glyph = 0f,
    toggle = { on, p ->
        if (on) {
            p.a.animateTo(-140f, tween(130, easing = Decel))
            launch { p.fill.animateTo(1f, tween(380, easing = Decel)) }
            launch {
                delay(260)
                p.b.dip(0.1f, ms = 80, damping = 0.4f, stiffness = 500f)
            }
            p.a.animateTo(0f, spring(dampingRatio = 0.34f, stiffness = 300f))
        } else {
            launch { p.fill.animateTo(0f, tween(260, easing = Accel)) }
            p.a.animateTo(-70f, tween(160, easing = Accel))
            p.a.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 320f))
        }
    },
    draw = { p, tint ->
        val f = p.fill.value.coerceIn(0f, 1f)
        val k = 1f + p.b.value
        scale(k, k, SpeedPaths.pivot) {
            layer {
                if (f > 0f) {
                    if (f >= 0.999f) {
                        drawPath(SpeedPaths.disc, tint)
                    } else {
                        layer {
                            drawCircle(tint, 11.5f * f, SpeedPaths.pivot)
                            keepInside(SpeedPaths.disc)
                        }
                    }
                }
                rotate(p.a.value - 30f * p.press.value, SpeedPaths.pivot) { drawPath(SpeedPaths.needle, tint, blendMode = BlendMode.Xor) }
            }
            drawPath(SpeedPaths.gauge, tint)
        }
    }
)

internal val DataLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.e.dip(-0.08f, ms = 110, damping = 0.35f, stiffness = 380f) }
            p.b.animateTo(100f, tween(110, easing = Accel))
            launch { p.b.animateTo(0f, MainSpatial) }
            delay(160)
            p.a.dip(2.2f, ms = 100, damping = 0.4f, stiffness = 420f)
        } else {
            p.a.dip(-0.9f, ms = 120, damping = 0.45f, stiffness = 380f)
        }
    },
    draw = { p, tint ->
        val k = 1f + p.e.value
        scale(k, k, Offset(12f, 12f)) {
            val ring = Stroke(3f)
            val tl = Offset(3.5f, 3.5f)
            val sz = Size(17f, 17f)
            drawArc(tint, 37.5f, 225.7f, false, tl, sz, style = ring)
            val out = p.a.value + 1.8f * p.press.value
            val mid = rad(-30.35f)
            translate(out * cos(mid), out * sin(mid)) {
                val grow = p.b.value.coerceIn(0f, 105.7f)
                drawArc(tint, -83.2f, 105.7f - grow, false, tl, sz, style = ring)
            }
        }
    }
)

internal val AppsLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.c.dip(1f, ms = 90, damping = 0.35f, stiffness = 420f) }
            delay(60)
            p.a.play(720)
            p.a.snapTo(0f)
        } else {
            p.b.play(520)
            p.b.snapTo(0f)
        }
    },
    tap = { p ->
        p.a.play(620)
        p.a.snapTo(0f)
    },
    draw = { p, tint ->
        val pull = 0.22f * (p.press.value + p.c.value)
        for (row in 0..2) {
            for (col in 0..2) {
                val cx = 6f + 6f * col
                val cy = 6f + 6f * row
                val on = wave(p.a.value, (row + col) * 0.1f, 0.5f, -0.5f)
                val off = wave(p.b.value, (4 - row - col) * 0.08f, 0.45f, -0.25f)
                val k = on * off
                val x = cx + (12f - cx) * pull
                val y = cy + (12f - cy) * pull
                drawRect(tint, Offset(x - 2f * k, y - 2f * k), Size(4f * k, 4f * k))
            }
        }
    }
)
