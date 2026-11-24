@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import android.os.SystemClock
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asComposePath
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.graphics.shapes.Morph
import androidx.graphics.shapes.RoundedPolygon
import androidx.graphics.shapes.toPath
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.tr
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.sin
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

enum class OrbStyle(val key: String, private val ruTitle: String, private val enTitle: String) {
    Wave("wave", "Волна", "Wave"),
    Gulp("gulp", "Глоток", "Gulp"),
    Sketch("sketch", "Набросок", "Sketch"),
    Ribbon("ribbon", "Лента", "Ribbon"),
    Rope("rope", "Канат", "Rope"),
    Chase("chase", "Догонялки", "Chase"),
    Breathe("breathe", "Дыхание", "Breathe")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

val LocalOrbStyle = staticCompositionLocalOf { OrbStyle.Wave }

private const val TAU = 6.2831855f
private const val N = LogoPoints.N
private val ORB = 188.dp

private const val LOGO_H = 0.44f

private const val WAVE_A = 0.011f
private const val WAVE_L = 0.075f
private const val WAVE_V = 1.4f
private const val SEG = 0.18f
private const val LAP = 1.7f
private const val THIN = 0.42f
private const val MIN_LOAD = 1200L

private val Decelerate = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)

private const val FIELD = 260f / 188f

private fun shapeOf(s: Status): RoundedPolygon = when (s) {
    Status.Off -> MaterialShapes.Cookie9Sided
    Status.Connecting -> MaterialShapes.Cookie6Sided
    Status.On -> MaterialShapes.Circle
    Status.Error -> MaterialShapes.Clover4Leaf
}

@Composable
fun ConnectOrb(
    status: Status,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    style: OrbStyle = LocalOrbStyle.current,
    size: Dp = ORB,
    onShown: (Status) -> Unit = {},
    bare: Boolean = false
) {
    val c = MaterialTheme.colorScheme
    val motion = MaterialTheme.motionScheme
    val m = remember { Loader(status) }
    val shown = m.shown
    val container = animateColorAsState(if (bare) Color.Transparent else containerOf(shown, c), motion.slowEffectsSpec(), label = "container")
    val ink = animateColorAsState(
        when {
            shown == Status.Error -> c.onErrorContainer
            shown == Status.On && !bare -> c.onPrimaryContainer
            else -> c.primary
        },
        motion.defaultEffectsSpec(),
        label = "ink"
    )
    val accent = c.tertiary
    val tonal = c.secondaryContainer
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val focused by interaction.collectIsFocusedAsState()
    val press = animateFloatAsState(if (pressed) 0.92f else if (focused) 1.08f else 1f, motion.fastSpatialSpec(), label = "press")
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    LaunchedEffect(shown) { onShown(shown) }
    LaunchedEffect(shown) {
        val target = shapeOf(shown)
        if (target === m.to) return@LaunchedEffect
        m.from = if (m.morphT.value < 0.5f) m.from else m.to
        m.to = target
        m.morph = Morph(m.from, target)
        m.morphT.snapTo(0f)
        m.morphT.animateTo(1f, motion.defaultSpatialSpec())
    }
    val thick = animateFloatAsState(if (shown == Status.On) 1f else 0f, motion.slowEffectsSpec(), label = "thick")

    LaunchedEffect(status) {
        if (m.last == status) return@LaunchedEffect
        m.last = status
        m.job?.cancel()
        m.job = scope.launch {
            m.burst.snapTo(0f)
            m.bounce.snapTo(1f)
            m.shake.snapTo(0f)
            when (status) {
                Status.Connecting -> {
                    m.since = SystemClock.elapsedRealtime()
                    m.shown = status
                    launch {
                        m.bounce.snapTo(0.9f)
                        m.bounce.animateTo(1f, spring(0.45f, 400f))
                    }
                    m.amp.animateTo(1f, spring(1f, 160f, EPS))
                }

                Status.On -> {
                    if (m.amp.value > 0f) {
                        m.amp.animateTo(1f, spring(1f, 160f, EPS))
                        val still = (coroutineContext[MotionDurationScale]?.scaleFactor ?: 1f) == 0f
                        if (!still) delay(MIN_LOAD - (SystemClock.elapsedRealtime() - m.since))
                    }
                    m.shown = status
                    haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                    launch {
                        m.bounce.snapTo(0.86f)
                        m.bounce.animateTo(1f, spring(0.32f, 300f))
                    }
                    launch {
                        m.burst.snapTo(0f)
                        m.burst.animateTo(1f, tween(900, easing = Decelerate))
                    }
                    m.amp.animateTo(0f, tween(800, easing = Decelerate))
                }

                Status.Off, Status.Error -> {
                    m.shown = status
                    if (status == Status.Error) {
                        haptics.performHapticFeedback(HapticFeedbackType.Reject)
                        launch {
                            m.shake.snapTo(1f)
                            m.shake.animateTo(0f, tween(600))
                        }
                    }
                    m.amp.animateTo(0f, spring(1f, 160f, EPS))
                }
            }
        }
    }
    LaunchedEffect(Unit) {
        if ((coroutineContext[MotionDurationScale]?.scaleFactor ?: 1f) == 0f) {
            m.clock = 0f
            return@LaunchedEffect
        }
        while (true) {
            snapshotFlow { m.loading }.first { it }
            val start = m.clock
            val t0 = withFrameNanos { it }
            while (m.loading) withFrameNanos { m.clock = start + (it - t0) / 1e9f }
        }
    }

    val ring by animateFloatAsState(if (focused) 1f else 0f, motion.fastEffectsSpec(), label = "focusRing")
    val ringColor = c.primary
    Box(
        modifier.size(size * FIELD).drawBehind {
            if (ring > 0f) drawCircle(ringColor.copy(alpha = ring), radius = this.size.minDimension / 2f - 2.dp.toPx(), style = Stroke(3.dp.toPx()))
        },
        contentAlignment = Alignment.Center
    ) {
        Canvas(
            Modifier
                .fillMaxSize()
                .graphicsLayer {
                    val k = press.value * m.bounce.value
                    scaleX = k
                    scaleY = k
                    translationX = m.shake.value * 18f * sin(m.shake.value * 28f)
                }
                .clickable(interaction, indication = null, role = Role.Button, onClick = onClick)
                .semantics { contentDescription = if (status == Status.On) tr("Отключить", "Disconnect") else tr("Подключить", "Connect") }
        ) {
            val d = this.size.minDimension / FIELD
            m.fit(this.size, d)
            val amp = m.amp.value.coerceIn(0f, 1f)
            val t = m.clock
            val full = LogoPoints.stroke * LOGO_H * d
            val thin = full * THIN
            val width = thin + (full - thin) * thick.value
            m.breath = 1f + 0.035f * amp * sin(TAU * t / 1.6f)
            val paint = OrbFrame(ink.value, container.value, accent, tonal, amp, t, d, full, thin, width)
            with(m) { drawStyle(style, paint) }
            val b = m.burst.value
            if (b > 0f && b < 1f) {
                drawCircle(
                    ink.value.copy(alpha = 0.5f * (1f - b)),
                    radius = d / 2 * (1f + (FIELD - 1f) * b),
                    style = Stroke(full * (1f - b) + 1f)
                )
            }
        }
    }
}

private class OrbFrame(
    val ink: Color,
    val container: Color,
    val accent: Color,
    val tonal: Color,
    val amp: Float,
    val t: Float,
    val d: Float,
    val full: Float,
    val thin: Float,
    val width: Float
)

@Composable
fun LogoMark(color: Color, modifier: Modifier = Modifier) {
    val drawn = remember { Animatable(0f) }
    LaunchedEffect(Unit) { drawn.animateTo(1f, tween(1100, easing = Decelerate)) }
    val path = remember { Path() }
    val xs = remember { FloatArray(LogoPoints.N) }
    val ys = remember { FloatArray(LogoPoints.N) }
    Canvas(modifier) {
        val n = (LogoPoints.N * drawn.value).toInt()
        if (n < 2) return@Canvas
        val h = size.minDimension / (1 + LogoPoints.stroke)
        for (i in 0 until n) {
            xs[i] = center.x + LogoPoints.x[i] * h
            ys[i] = center.y + LogoPoints.y[i] * h
        }
        path.smoothOpen(xs, ys, n)
        drawPath(path, color, style = Stroke(LogoPoints.stroke * h, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

@Composable
fun LoadingLogo(shape: RoundedPolygon, modifier: Modifier = Modifier, style: OrbStyle = LocalOrbStyle.current) {
    val c = MaterialTheme.colorScheme
    val m = remember { Loader(Status.Connecting).also { it.polygon = shape } }
    LaunchedEffect(Unit) {
        m.amp.snapTo(1f)
        if ((coroutineContext[MotionDurationScale]?.scaleFactor ?: 1f) == 0f) return@LaunchedEffect
        val t0 = withFrameNanos { it }
        while (true) withFrameNanos { m.clock = (it - t0) / 1e9f }
    }
    Canvas(modifier) {
        val d = size.minDimension
        m.fit(size, d)
        val full = LogoPoints.stroke * LOGO_H * d
        val thin = full * THIN
        val frame = OrbFrame(c.onPrimaryContainer, c.primaryContainer, c.tertiary, c.secondaryContainer, 1f, m.clock, d, full, thin, thin)
        with(m) { drawStyle(style, frame) }
    }
}

private const val EPS = 1e-3f

private fun containerOf(s: Status, c: ColorScheme) = when (s) {
    Status.On -> c.primaryContainer
    Status.Error -> c.errorContainer
    Status.Off, Status.Connecting -> c.surfaceContainerHigh
}

private fun smooth(v: Float): Float {
    val u = v.coerceIn(0f, 1f)
    return u * u * (3 - 2 * u)
}

private fun ease(v: Float): Float {
    val u = v.coerceIn(0f, 1f)
    return if (u < 0.5f) 4 * u * u * u else 1 - (-2 * u + 2).let { it * it * it } / 2
}

private fun frac(v: Float) = v - floor(v)

private val round = { w: Float -> Stroke(w, cap = StrokeCap.Round, join = StrokeJoin.Round) }

private class Loader(start: Status) {
    val amp = Animatable(if (start == Status.Connecting) 1f else 0f, EPS)
    val bounce = Animatable(1f)
    val burst = Animatable(0f)
    val shake = Animatable(0f)
    var shown by mutableStateOf(start)
    var since = 0L
    var job: Job? = null
    var clock by mutableFloatStateOf(0f)
    var last = start

    val line = Path()
    val active = Path()
    private val shape = android.graphics.Path()
    private val matrix = android.graphics.Matrix()
    private var fitted = Size.Zero
    private var h = 0f
    private var d = 0f
    private var cx = 0f
    private var cy = 0f
    private val px = FloatArray(LogoPoints.N)
    private val py = FloatArray(LogoPoints.N)
    private val wx = FloatArray(LogoPoints.LOOP)
    private val wy = FloatArray(LogoPoints.LOOP)
    private val ex = FloatArray(LogoPoints.N)
    private val ey = FloatArray(LogoPoints.N)

    val loading get() = amp.value > 0f

    var polygon: RoundedPolygon? = null
    var from: RoundedPolygon = shapeOf(start)
    var to: RoundedPolygon = from
    var morph: Morph? = null
    val morphT = Animatable(1f)
    var breath = 1f
    private val poly = android.graphics.Path()
    private val polyMatrix = android.graphics.Matrix()

    private fun DrawScope.container(color: Color) {
        poly.rewind()
        val fixed = polygon
        val mo = morph
        when {
            fixed != null -> fixed.toPath(poly)
            mo != null && morphT.value < 1f -> mo.toPath(morphT.value, poly)
            else -> to.toPath(poly)
        }
        val s = d * (if (fixed == null) breath else 1f)
        polyMatrix.reset()
        polyMatrix.setScale(s, s)
        polyMatrix.postTranslate(cx - s / 2, cy - s / 2)
        poly.transform(polyMatrix)
        drawPath(poly.asComposePath(), color)
    }

    fun fit(size: Size, orb: Float) {
        if (size == fitted) return
        fitted = size
        d = orb
        h = LOGO_H * d
        cx = size.width / 2
        cy = size.height / 2
        for (i in 0 until LogoPoints.N) {
            px[i] = cx + LogoPoints.x[i] * h
            py[i] = cy + LogoPoints.y[i] * h
        }
        line.smoothOpen(px, py, LogoPoints.N)
    }

    private val c get() = Offset(cx, cy)

    fun DrawScope.drawStyle(style: OrbStyle, p: OrbFrame) {
        val a = p.amp
        val r = d / 2
        if (a <= 0f) {
            container(p.container)
            drawPath(line, p.ink, style = round(p.width))
            return
        }
        val t = p.t
        when (style) {
            OrbStyle.Wave -> {
                container(p.container)
                drawPath(line, p.ink.copy(alpha = 1f - 0.78f * a), style = round(p.width))
                segment(t)
                drawPath(active, p.ink.copy(alpha = a), style = round(p.thin))
            }

            OrbStyle.Gulp -> {
                container(p.container)
                val head = (frac(t * 0.42f) * 1.3f - 0.15f) * N
                val sigma = N * 0.075f
                var i = 0
                while (i < N - 1) {
                    val g = exp(-((i - head) / sigma).let { it * it })
                    span(px, py, i, i + 4, p.ink, p.width + a * (p.full * 1.35f * g - (p.width - p.thin) * 0.6f))
                    i += 3
                }
            }

            OrbStyle.Sketch -> {
                container(p.container)
                drawPath(line, p.ink.copy(alpha = 1f - 0.8f * a), style = round(p.width))
                val head = frac(t * 0.5f) * 1.3f * N
                val tail = 84
                val ink = p.ink.copy(alpha = a)
                var k = tail
                while (k > 0) {
                    val i = (head - k).toInt()
                    val f = 1f - k / tail.toFloat()
                    if (i + 4 in 1 until N + 4) span(px, py, i, i + 4, ink, p.thin * 0.4f + (p.full - p.thin * 0.4f) * f * f)
                    k -= 3
                }
            }

            OrbStyle.Ribbon -> {
                container(p.container)
                val back = lerp(p.ink, p.accent, a)
                var i = 0
                while (i < N - 1) {
                    val ph = cos(i / N.toFloat() * TAU * 1.5f - t * 3.2f)
                    val w = p.width + a * (p.full * (0.22f + 0.95f * abs(ph)) - p.width)
                    span(px, py, i, i + 4, if (ph > 0f) p.ink else back, w)
                    i += 3
                }
            }

            OrbStyle.Rope -> {
                container(p.container)
                val amp = d * 0.026f * a
                for (i in 0 until N) {
                    val off = amp * sin(i * 0.13f - t * 4f)
                    wx[i] = px[i] + LogoPoints.nx[i] * off
                    wy[i] = py[i] + LogoPoints.ny[i] * off
                    ex[i] = px[i] - LogoPoints.nx[i] * off
                    ey[i] = py[i] - LogoPoints.ny[i] * off
                }
                val w = p.width + a * (p.full * 0.46f - p.width)
                val second = lerp(p.ink, p.accent, a)
                var i = 0
                while (i < N - 1) {
                    val front = cos(i * 0.13f - t * 4f) > 0f
                    if (front) {
                        span(ex, ey, i, i + 4, second, w)
                        span(wx, wy, i, i + 4, p.ink, w)
                    } else {
                        span(wx, wy, i, i + 4, p.ink, w)
                        span(ex, ey, i, i + 4, second, w)
                    }
                    i += 3
                }
            }

            OrbStyle.Chase -> {
                container(p.container)
                drawPath(line, p.ink.copy(alpha = 1f - 0.8f * a), style = round(p.width))
                val base = frac(t * 0.36f) * 1.3f * N
                val gap = 26f + 22f * cos(TAU * t / 1.9f)
                val w = p.thin + (p.full - p.thin) * 0.9f
                span(px, py, (base - 36).toInt(), base.toInt(), p.ink.copy(alpha = a), w)
                span(px, py, (base - gap - 60).toInt(), (base - gap - 36).toInt(), p.ink.copy(alpha = a), w)
            }

            OrbStyle.Breathe -> {
                val b = 0.5f - 0.5f * cos(TAU * t / 1.6f)
                val k = ease(b)
                val sc = 1f + 0.05f * a * k
                val amp = d * 0.012f * a
                for (i in 0 until N) {
                    val off = amp * sin(i * 0.1f - t * 4f)
                    wx[i] = px[i] + LogoPoints.nx[i] * off
                    wy[i] = py[i] + LogoPoints.ny[i] * off
                }
                container(p.container)
                withTransform({ scale(sc, sc, c) }) {
                    active.reset()
                    active.addSmooth(wx, wy, N)
                    drawPath(active, p.ink, style = round(p.width + a * (p.full * (0.55f + 0.85f * k) - p.width)))
                }
            }
        }
    }

    private fun DrawScope.span(xs: FloatArray, ys: FloatArray, from: Int, to: Int, color: Color, width: Float, cap: StrokeCap = StrokeCap.Round) {
        val a = from.coerceIn(0, N - 1)
        val b = to.coerceIn(0, N - 1)
        if (b - a < 1) return
        active.reset()
        active.moveTo(xs[a], ys[a])
        for (i in a + 1..b) active.lineTo(xs[i], ys[i])
        drawPath(active, color, style = Stroke(width, cap = cap, join = StrokeJoin.Round))
    }

    private fun segment(clock: Float) {
        val loop = LogoPoints.LOOP
        val last = LogoPoints.N - 1
        val n = (loop * SEG).toInt().coerceAtLeast(2)
        val head = (clock / LAP * loop) % loop
        val a = WAVE_A * d
        val phase = WAVE_V * clock
        val fade = n * 0.25f
        val edge = LogoPoints.N * 0.04f
        active.reset()
        var run = 0
        for (k in 0 until n) {
            val p = (head - (n - 1 - k) + loop) % loop
            if (p > last) {
                if (run >= 2) active.addSmooth(wx, wy, run)
                run = 0
                continue
            }
            val i0 = p.toInt()
            val i1 = minOf(i0 + 1, last)
            val f = p - i0
            val x = LogoPoints.lx[i0] + (LogoPoints.lx[i1] - LogoPoints.lx[i0]) * f
            val y = LogoPoints.ly[i0] + (LogoPoints.ly[i1] - LogoPoints.ly[i0]) * f
            val taper = smooth(minOf(k, n - 1 - k) / fade) * smooth(minOf(p, last - p) / edge)
            val off = a * taper * sin(TAU * (k * LogoPoints.step * h / (WAVE_L * d) - phase))
            wx[run] = cx + x * h + LogoPoints.lnx[i0] * off
            wy[run] = cy + y * h + LogoPoints.lny[i0] * off
            run++
        }
        if (run >= 2) active.addSmooth(wx, wy, run)
    }
}

private fun Path.addSmooth(x: FloatArray, y: FloatArray, n: Int) {
    moveTo(x[0], y[0])
    for (i in 1 until n - 1) quadraticTo(x[i], y[i], (x[i] + x[i + 1]) / 2, (y[i] + y[i + 1]) / 2)
    lineTo(x[n - 1], y[n - 1])
}

private val LOGO = floatArrayOf(
    330f, 540f,
    400f, 560f, 452f, 560f, 446f, 620f,
    440f, 676f, 360f, 700f, 330f, 760f,
    300f, 820f, 330f, 856f, 420f, 862f,
    520f, 870f, 640f, 864f, 712f, 822f,
    782f, 780f, 772f, 700f, 700f, 668f,
    640f, 640f, 610f, 600f, 622f, 548f,
    634f, 496f, 730f, 470f, 724f, 380f,
    718f, 280f, 620f, 214f, 512f, 214f,
    400f, 214f, 330f, 260f, 312f, 330f,
    302f, 372f, 298f, 400f, 300f, 430f
)

private object LogoPoints {
    const val N = 360
    val x = FloatArray(N)
    val y = FloatArray(N)
    val s = FloatArray(N)
    val nx = FloatArray(N)
    val ny = FloatArray(N)
    val stroke: Float

    val LOOP: Int
    val lx: FloatArray
    val ly: FloatArray
    val lnx: FloatArray
    val lny: FloatArray
    val step: Float

    init {
        val steps = 48
        val segs = (LOGO.size - 2) / 6
        val m = segs * steps + 1
        val dx = FloatArray(m).also { it[0] = LOGO[0] }
        val dy = FloatArray(m).also { it[0] = LOGO[1] }
        for (sg in 0 until segs) {
            val o = 2 + sg * 6
            for (j in 1..steps) {
                val t = j / steps.toFloat()
                val u = 1 - t
                val a = u * u * u
                val b = 3 * u * u * t
                val c = 3 * u * t * t
                val d = t * t * t
                dx[sg * steps + j] = a * LOGO[o - 2] + b * LOGO[o] + c * LOGO[o + 2] + d * LOGO[o + 4]
                dy[sg * steps + j] = a * LOGO[o - 1] + b * LOGO[o + 1] + c * LOGO[o + 3] + d * LOGO[o + 5]
            }
        }
        val len = FloatArray(m)
        for (k in 1 until m) len[k] = len[k - 1] + hypot(dx[k] - dx[k - 1], dy[k] - dy[k - 1])
        val h = dy.max() - dy.min()
        var k = 0
        for (i in 0 until N) {
            val at = len[m - 1] * i / (N - 1)
            while (k < m - 2 && len[k + 1] < at) k++
            val f = ((at - len[k]) / (len[k + 1] - len[k])).coerceIn(0f, 1f)
            x[i] = (dx[k] + (dx[k + 1] - dx[k]) * f - 512f) / h
            y[i] = (dy[k] + (dy[k + 1] - dy[k]) * f - 538f) / h
            s[i] = at / h
        }
        for (i in 0 until N) {
            val tx = x[minOf(i + 1, N - 1)] - x[maxOf(i - 1, 0)]
            val ty = y[minOf(i + 1, N - 1)] - y[maxOf(i - 1, 0)]
            val l = hypot(tx, ty)
            nx[i] = -ty / l
            ny[i] = tx / l
        }
        stroke = 68f / h
        val ds = s[1] - s[0]
        val ex = x[N - 1]
        val ey = y[N - 1]
        val sx = x[0]
        val sy = y[0]
        val gap = hypot(sx - ex, sy - ey)
        val te = norm(x[N - 1] - x[N - 2], y[N - 1] - y[N - 2])
        val ts = norm(x[1] - x[0], y[1] - y[0])
        val k1 = gap * 0.6f
        val c1x = ex + te.first * k1
        val c1y = ey + te.second * k1
        val c2x = sx - ts.first * k1
        val c2y = sy - ts.second * k1
        val fine = 200
        val bx = FloatArray(fine + 1)
        val by = FloatArray(fine + 1)
        val bl = FloatArray(fine + 1)
        for (j in 0..fine) {
            val t = j / fine.toFloat()
            val u = 1 - t
            bx[j] = u * u * u * ex + 3 * u * u * t * c1x + 3 * u * t * t * c2x + t * t * t * sx
            by[j] = u * u * u * ey + 3 * u * u * t * c1y + 3 * u * t * t * c2y + t * t * t * sy
            if (j > 0) bl[j] = bl[j - 1] + hypot(bx[j] - bx[j - 1], by[j] - by[j - 1])
        }
        val bridge = (bl[fine] / ds).toInt().coerceAtLeast(1)
        LOOP = N + bridge - 1
        lx = FloatArray(LOOP)
        ly = FloatArray(LOOP)
        x.copyInto(lx)
        y.copyInto(ly)
        var q = 0
        for (b in 1 until bridge) {
            val at = bl[fine] * b / bridge
            while (q < fine - 1 && bl[q + 1] < at) q++
            val f = ((at - bl[q]) / (bl[q + 1] - bl[q])).coerceIn(0f, 1f)
            lx[N - 1 + b] = bx[q] + (bx[q + 1] - bx[q]) * f
            ly[N - 1 + b] = by[q] + (by[q + 1] - by[q]) * f
        }
        lnx = FloatArray(LOOP)
        lny = FloatArray(LOOP)
        for (i in 0 until LOOP) {
            val (tx, ty) = norm(lx[(i + 1) % LOOP] - lx[(i + LOOP - 1) % LOOP], ly[(i + 1) % LOOP] - ly[(i + LOOP - 1) % LOOP])
            lnx[i] = -ty
            lny[i] = tx
        }
        step = ds
    }

    private fun norm(x: Float, y: Float): Pair<Float, Float> {
        val l = hypot(x, y).coerceAtLeast(1e-6f)
        return x / l to y / l
    }
}

private fun Path.smoothOpen(x: FloatArray, y: FloatArray, n: Int) {
    reset()
    if (n < 2) return
    moveTo(x[0], y[0])
    for (i in 1 until n - 1) quadraticTo(x[i], y[i], (x[i] + x[i + 1]) / 2, (y[i] + y[i + 1]) / 2)
    lineTo(x[n - 1], y[n - 1])
}
