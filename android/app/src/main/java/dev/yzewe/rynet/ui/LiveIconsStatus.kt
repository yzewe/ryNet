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

internal val PowerLive = Live(
    toggle = { on, p ->
        if (on) {
            p.a.animateTo(-1.8f, tween(90, easing = Decel))
            p.a.animateTo(3f, tween(70, easing = Accel))
            launch { p.a.animateTo(0f, spring(dampingRatio = 0.42f, stiffness = 600f)) }
            launch {
                p.b.animateTo(-0.16f, tween(60, easing = Decel))
                p.b.animateTo(0f, spring(dampingRatio = 0.38f, stiffness = 380f))
            }
            launch {
                p.d.animateTo(-74f, tween(70, easing = Decel))
                p.d.animateTo(0f, spring(dampingRatio = 0.45f, stiffness = 380f))
            }
            launch {
                delay(40)
                p.e.kick(420f, damping = 0.4f, stiffness = 320f)
            }
            delay(40)
            p.c.play(380)
        } else {
            launch { p.b.dip(-0.08f, ms = 90, damping = 0.55f, stiffness = 600f) }
            launch { p.d.dip(56f, ms = 100, damping = 0.55f, stiffness = 500f) }
            p.a.dip(-2.4f, ms = 90, damping = 0.55f, stiffness = 600f)
        }
    },
    draw = { p, tint ->
        echo(tint, Offset(12f, 12f), p.c.value, 8f, 11.2f)
        val gap = (80f + p.d.value).coerceAtLeast(2f)
        val k = 1f + p.b.value - 0.08f * p.press.value
        rotate(p.e.value, Offset(12f, 12f)) {
            scale(k, k, Offset(12f, 12f)) {
                drawArc(tint, -90f + gap / 2f, 360f - gap, false, Offset(4f, 4f), Size(16f, 16f), style = Stroke(2f))
            }
        }
        val y = p.a.value + 2.2f * p.press.value
        drawLine(tint, Offset(12f, 3f + y), Offset(12f, 13f + y), strokeWidth = 2f)
    }
)

private object BatteryPaths {
    val body by lazy { roundRect(7f, 4f, 17f, 22f, 1.33f) }
}

internal val BatteryLive = Live(
    rest = { f -> floatArrayOf(f, f, f, 0f) },
    toggle = { on, p ->
        val cells = listOf(p.a, p.b, p.c)
        if (on) {
            for ((i, cell) in cells.withIndex()) {
                launch {
                    delay(i * 110L)
                    cell.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = 500f))
                }
            }
            delay(330)
            launch { p.fill.animateTo(1f, tween(160, easing = Decel)) }
            launch { p.e.play(360) }
            p.d.dip(-1.5f, ms = 80, damping = 0.35f, stiffness = 520f)
        } else {
            p.fill.animateTo(0f, tween(120, easing = Accel))
            for ((i, cell) in cells.reversed().withIndex()) {
                launch {
                    delay(i * 80L)
                    cell.animateTo(0f, tween(130, easing = Accel))
                }
            }
        }
    },
    tap = { p -> p.d.dip(-1.2f, ms = 80, damping = 0.35f, stiffness = 520f) },
    draw = { p, tint ->
        scale(1f, 1f - 0.1f * p.press.value, Offset(12f, 22f)) {
            drawRect(tint, Offset(10f, 2f + p.d.value), Size(4f, 2.4f))
            val f = p.fill.value.coerceIn(0f, 1f)
            ink(BatteryPaths.body, tint, f, w = 2f, r = 3f)
            if (f < 1f) {
                for ((i, cell) in listOf(p.a, p.b, p.c).withIndex()) {
                    val k = cell.value
                    if (k <= 0.01f) continue
                    val cy = 17.5f - i * 4.6f
                    val w = 4.8f * k
                    val h = 3.6f * k
                    drawRect(tint, Offset(12f - w / 2f, cy - h / 2f), Size(w, h))
                }
            }
        }
        val t = p.e.value
        spark(tint, Offset(9.2f, 2.6f), -0.94f, -0.34f, t, length = 1.6f)
        spark(tint, Offset(14.8f, 2.6f), 0.94f, -0.34f, t, length = 1.6f)
    }
)

private object BellPaths {
    private val parts by lazy { Icons.Rounded.Notifications.subpaths() }
    val clapper get() = parts[0]
    val body get() = parts[1]
    val pivot = Offset(12f, 3f)
}

internal val BellLive = Live(
    toggle = { on, p ->
        if (on) {
            launch { p.fill.animateTo(1f, FillSpring) }
            launch { p.b.animateTo(-5f, tween(120)) }
            p.a.animateTo(-9f, tween(90, easing = Decel))
            launch {
                delay(120)
                p.c.play(560)
            }
            launch { p.a.kick(470f, damping = 0.24f, stiffness = 300f) }
            delay(60)
            p.b.kick(520f, damping = 0.22f, stiffness = 260f)
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.d.dip(-0.1f, ms = 80, damping = 0.4f, stiffness = 500f) }
            launch { p.a.kick(-190f, damping = 0.42f, stiffness = 700f) }
            delay(40)
            p.b.kick(-210f, damping = 0.42f, stiffness = 600f)
        }
    },
    tap = { p ->
        launch { p.c.play(520) }
        launch { p.a.kick(220f, damping = 0.24f, stiffness = 300f) }
        delay(60)
        p.b.kick(250f, damping = 0.24f, stiffness = 280f)
    },
    draw = { p, tint ->
        val c = p.c.value
        for (k in 0..1) {
            val t = c * 1.4f - 0.4f * k
            if (t > 0f && t < 1f) {
                val r = 8.4f + 2.2f * easeOut(t)
                val tl = Offset(12f - r, 11f - r)
                val sz = Size(2 * r, 2 * r)
                val style = Stroke(1.7f, cap = StrokeCap.Round)
                drawArc(tint, 204f, 36f, false, tl, sz, alpha = 1f - t, style = style)
                drawArc(tint, -60f, 36f, false, tl, sz, alpha = 1f - t, style = style)
            }
        }
        val lean = -14f * p.press.value
        scale(1f - 0.5f * p.d.value, 1f + p.d.value, Offset(12f, 20f)) {
            rotate(p.b.value * 1.15f + lean * 1.4f, BellPaths.pivot) { drawPath(BellPaths.clapper, tint) }
            rotate(p.a.value + lean, BellPaths.pivot) { ink(BellPaths.body, tint, p.fill.value, w = 2f, r = 4.2f) }
        }
    }
)

private object HeartPaths {
    val heart by lazy { Icons.Rounded.Favorite.subpaths()[0] }
}

internal val HeartLive = Live(
    toggle = { on, p ->
        if (on) {
            p.a.animateTo(-0.12f, tween(90, easing = Decel))
            launch { p.fill.animateTo(1f, spring(dampingRatio = 1f, stiffness = 380f)) }
            launch {
                delay(60)
                p.c.play(560)
            }
            p.a.animateTo(0f, spring(dampingRatio = 0.3f, stiffness = 420f), initialVelocity = 4.2f)
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.d.dip(1f, ms = 130, damping = 0.5f, stiffness = 360f) }
            p.a.dip(-0.18f, ms = 130, damping = 0.45f, stiffness = 360f)
        }
    },
    tap = { p ->
        launch { p.c.play(480) }
        p.a.animateTo(0f, spring(dampingRatio = 0.35f, stiffness = 460f), initialVelocity = 3f)
    },
    draw = { p, tint ->
        val t = p.c.value
        if (t > 0f && t < 1f) {
            val r = 1f + 0.32f * easeOut(t)
            scale(r, r, Offset(12f, 12f)) { drawPath(HeartPaths.heart, tint, alpha = 0.5f * (1f - t) * (1f - t), style = Stroke(1.4f / r)) }
        }
        val k = 1f + p.a.value - 0.16f * p.press.value
        translate(0f, p.d.value) {
            scale(k, k, Offset(12f, 12f)) {
                val f = p.fill.value.coerceIn(0f, 1f)
                if (f >= 0.999f) {
                    drawPath(HeartPaths.heart, tint)
                } else {
                    ink(HeartPaths.heart, tint, 0f, w = 2f, r = 5.2f)
                    if (f > 0.001f) scale(f, f, Offset(12f, 12.5f)) { drawPath(HeartPaths.heart, tint) }
                }
            }
        }
    }
)

private object BoltPaths {
    val bolt by lazy { Glyph.Bolt.subpaths()[0] }
}

internal val BoltLive = Live(
    toggle = { on, p ->
        if (on) {
            p.d.snapTo(0f)
            launch {
                p.e.animateTo(-0.12f, tween(80, easing = Decel))
                p.e.animateTo(0.08f, tween(160))
                p.e.animateTo(-0.14f, tween(50))
                p.e.animateTo(0f, spring(dampingRatio = 0.35f, stiffness = 600f))
            }
            delay(80)
            p.a.animateTo(-1.6f, tween(80, easing = Decel))
            p.a.animateTo(0f, tween(80, easing = Accel))
            launch { p.fill.animateTo(1f, tween(260, easing = Decel)) }
            launch { p.c.play(400) }
            p.b.kick(36f, damping = 0.14f, stiffness = 1500f)
        } else {
            p.d.snapTo(1f)
            launch { p.fill.animateTo(0f, tween(320, easing = Accel)) }
            launch { p.e.dip(-0.1f, ms = 110, damping = 0.45f, stiffness = 500f) }
            p.a.dip(0.9f, ms = 110, damping = 0.5f, stiffness = 500f)
        }
    },
    tap = { p ->
        launch { p.c.play(400) }
        launch { p.e.dip(-0.12f, ms = 70, damping = 0.35f, stiffness = 600f) }
        p.b.kick(32f, damping = 0.14f, stiffness = 1500f)
    },
    draw = { p, tint ->
        flash(tint, Offset(12f, 12f), p.c.value * 1.6f, 11f, 0.45f)
        translate(p.b.value, p.a.value) {
            scale(1f - 0.4f * p.e.value, 1f + p.e.value - 0.15f * p.press.value, Offset(12f, 21f)) {
                val f = p.fill.value.coerceIn(0f, 1f)
                if (f >= 0.999f) {
                    drawPath(BoltPaths.bolt, tint)
                } else {
                    layer {
                        drawPath(BoltPaths.bolt, tint, style = Stroke(3.2f, join = StrokeJoin.Round))
                        if (f > 0f) {
                            if (p.d.value < 0.5f) {
                                drawRect(tint, Offset(0f, 0f), Size(24f, 3f + 18.4f * f))
                            } else {
                                drawRect(tint, Offset(0f, 21.4f - 18.4f * f), Size(24f, 24f))
                            }
                        }
                        keepInside(BoltPaths.bolt)
                    }
                }
            }
        }
        spark(tint, Offset(6.6f, 15.8f), -0.9f, 0.45f, p.c.value)
        spark(tint, Offset(17.4f, 8.2f), 0.9f, -0.45f, p.c.value)
    }
)

private object LinkPaths {
    private val parts by lazy { Glyph.Link.subpaths() }
    val left get() = parts[0]
    val bar get() = parts[1]
    val right get() = parts[2]
    val rays = floatArrayOf(-62f, -90f, -118f, 62f, 90f, 118f)
}

internal val LinkLive = Live(
    toggle = { on, p ->
        if (on) {
            p.fill.animateTo(-1.4f, tween(120, easing = Decel))
            launch {
                delay(150)
                launch { p.a.kick(-160f, damping = 0.35f, stiffness = 420f) }
                p.c.play(360)
            }
            p.fill.animateTo(1f, spring(dampingRatio = 0.42f, stiffness = 360f))
        } else {
            launch { p.a.kick(-90f, damping = 0.35f, stiffness = 380f) }
            p.fill.animateTo(0f, spring(dampingRatio = 0.55f, stiffness = 320f))
        }
    },
    tap = { p ->
        launch { p.a.kick(-120f, damping = 0.35f, stiffness = 420f) }
        p.c.play(360)
    },
    draw = { p, tint ->
        val f = p.fill.value
        burst(tint, Offset(12f, 12f), p.c.value, 5.2f, 6.8f, LinkPaths.rays, length = 1.4f)
        val apart = (1f - f) * 1.4f - 1.1f * p.press.value
        val fade = (1f + f / 1.4f).coerceIn(0f, 1f).let { if (f < 0f) 0.35f + 0.65f * it else 1f }
        translate(-apart, 0f) { rotate(-p.a.value * 0.1f, Offset(7f, 12f)) { drawPath(LinkPaths.left, tint, alpha = fade) } }
        translate(apart, 0f) { rotate(p.a.value * 0.1f, Offset(17f, 12f)) { drawPath(LinkPaths.right, tint, alpha = fade) } }
        val bar = f.coerceIn(0f, 1.15f)
        if (bar > 0.08f) scale(bar, 1f, Offset(12f, 12f)) { drawPath(LinkPaths.bar, tint) }
    }
)

internal val CutLive = Live(
    toggle = { on, p ->
        if (on) {
            snip(p)
        } else {
            launch { p.e.dip(-1.2f, ms = 90, damping = 0.55f, stiffness = 600f) }
            p.a.dip(12f, ms = 100, damping = 0.55f, stiffness = 600f)
        }
    },
    tap = { p -> snip(p) },
    draw = { p, tint ->
        val strip = p.d.value
        if (strip > 0f) drawLine(tint, Offset(17.6f, 12f), Offset(23.6f, 12f), strokeWidth = 1.6f, alpha = strip)
        val t = p.c.value
        if (t > 0f && t < 1f) {
            val x = 21f + 1.4f * easeOut(t)
            val y = 12f + 11f * t * t
            rotate(260f * t, Offset(x, y)) {
                drawLine(tint, Offset(x - 2f, y), Offset(x + 2f, y), strokeWidth = 1.6f, alpha = (1f - t * t).coerceIn(0f, 1f))
            }
        }
        val open = p.a.value + 22f * p.press.value
        val pivot = Offset(12f, 12f)
        val ring = Stroke(2f)
        translate(p.e.value, 0f) {
            layer {
                rotate(open / 2f, pivot) {
                    drawCircle(tint, 3f, Offset(6f, 6f), style = ring)
                    drawLine(tint, Offset(8.1f, 8.1f), Offset(20.6f, 20.6f), strokeWidth = 2.1f)
                }
                rotate(-open / 2f, pivot) {
                    drawCircle(tint, 3f, Offset(6f, 18f), style = ring)
                    drawLine(tint, Offset(8.1f, 15.9f), Offset(10.7f, 13.3f), strokeWidth = 2.1f)
                    drawLine(tint, Offset(13.5f, 10.5f), Offset(20.6f, 3.4f), strokeWidth = 2.1f)
                }
                drawCircle(Black, 0.55f, pivot, blendMode = BlendMode.DstOut)
            }
        }
    }
)

private suspend fun snip(p: Parts) = coroutineScope {
    launch { p.d.animateTo(1f, tween(110)) }
    launch { p.e.animateTo(2.2f, spring(dampingRatio = 0.7f, stiffness = 500f)) }
    p.a.animateTo(34f, tween(130, easing = Decel))
    p.a.animateTo(-46f, tween(70, easing = Accel))
    p.d.snapTo(0f)
    launch { p.c.play(520) }
    launch {
        delay(60)
        p.e.animateTo(0f, MainSpatial)
    }
    p.a.animateTo(0f, spring(dampingRatio = 0.38f, stiffness = 520f))
}

private object LockPaths {
    val body by lazy { roundRect(4f, 8f, 20f, 22f, 2f) }
}

internal val LockLive = Live(
    rest = { f -> floatArrayOf(0f, 1f - f, 0f, 0f) },
    toggle = { on, p ->
        if (on) {
            launch { p.b.animateTo(0f, tween(180, easing = Accel)) }
            p.a.animateTo(0.9f, tween(90, easing = Decel))
            p.a.animateTo(-0.8f, tween(90, easing = Accel))
            launch { p.c.dip(-0.14f, ms = 50, damping = 0.32f, stiffness = 520f) }
            launch { p.d.play(360) }
            launch { p.fill.animateTo(1f, FillSpring) }
            p.a.animateTo(0f, spring(dampingRatio = 0.4f, stiffness = 600f))
        } else {
            launch { p.fill.animateTo(0f, FillSpring) }
            launch { p.b.animateTo(1f, tween(160, easing = Decel)) }
            launch { p.c.dip(0.06f, ms = 100, damping = 0.4f, stiffness = 420f) }
            p.a.animateTo(1.2f, tween(110, easing = Decel))
            p.a.animateTo(0f, spring(dampingRatio = 0.38f, stiffness = 420f))
        }
    },
    draw = { p, tint ->
        val lift = p.a.value - 0.9f * p.press.value
        val leg = 8.6f - 2.6f * p.b.value.coerceIn(0f, 1f)
        translate(0f, -lift) {
            clipRect(0f, -4f, 24f, 9f + lift) {
                drawPath(
                    Path().apply {
                        moveTo(7.95f, leg)
                        lineTo(7.95f, 6f)
                        arcTo(Rect(7.95f, 1.95f, 16.05f, 10.05f), 180f, 180f, false)
                        lineTo(16.05f, 9f)
                    },
                    tint,
                    style = Stroke(1.9f)
                )
            }
        }
        spark(tint, Offset(5.6f, 5.4f), -0.8f, -0.6f, p.d.value, length = 1.8f, travel = 1.2f)
        spark(tint, Offset(18.4f, 5.4f), 0.8f, -0.6f, p.d.value, length = 1.8f, travel = 1.2f)
        scale(1f - 0.4f * p.c.value, 1f + p.c.value - 0.08f * p.press.value, Offset(12f, 22f)) {
            val f = p.fill.value.coerceIn(0f, 1f)
            layer {
                drawPath(LockPaths.body, tint, style = Stroke(2f * (2f + f * 5.5f), join = StrokeJoin.Round))
                keepInside(LockPaths.body)
                drawCircle(tint, 2f, Offset(12f, 15f))
                if (f > 0f) drawCircle(Black, 2f * f, Offset(12f, 15f), blendMode = BlendMode.DstOut)
            }
        }
    }
)
