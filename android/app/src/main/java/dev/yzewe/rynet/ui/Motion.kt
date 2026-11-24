package dev.yzewe.rynet.ui

import android.os.SystemClock
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector2D
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.isSpecified
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.round
import kotlin.math.PI
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

object Springs {
    fun <T> spatial(): SpringSpec<T> = spring(dampingRatio = 0.78f, stiffness = 420f)

    fun <T> bouncy(): SpringSpec<T> = spring(dampingRatio = 0.58f, stiffness = 520f)

    fun <T> effects(): SpringSpec<T> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = 1400f)

    fun <T> pressIn(): SpringSpec<T> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = 1800f)

    fun <T> pressOut(): SpringSpec<T> = spring(dampingRatio = 0.42f, stiffness = 650f)

    val EmphasizedDecelerate = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)
    val EmphasizedAccelerate = CubicBezierEasing(0.3f, 0f, 0.8f, 0.15f)

    internal const val MIN_VISIBLE_PRESS_MS = 90L
}

object PressIndication : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = PressNode(interactionSource, sink = true)
    override fun equals(other: Any?): Boolean = other === this
    override fun hashCode(): Int = javaClass.hashCode()
}

private const val FOCUS_ALPHA = 0.22f

object PressGlow : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = PressNode(interactionSource, sink = false)
    override fun equals(other: Any?): Boolean = other === this
    override fun hashCode(): Int = javaClass.hashCode()
}

private class PressNode(private val source: InteractionSource, private val sink: Boolean) :
    Modifier.Node(),
    DrawModifierNode,
    CompositionLocalConsumerModifierNode {
    private val focus = Animatable(0f)
    private val depth = Animatable(0f)
    private val glow = Animatable(0f)
    private val spread = Animatable(0f)
    private var origin = Offset.Unspecified

    override fun onAttach() {
        coroutineScope.launch {
            var pressedAt = 0L
            var release: Job? = null
            source.interactions.collect { i ->
                when (i) {
                    is FocusInteraction.Focus -> launch { focus.animateTo(1f, Springs.effects()) { invalidateDraw() } }

                    is FocusInteraction.Unfocus -> launch { focus.animateTo(0f, Springs.effects()) { invalidateDraw() } }

                    is PressInteraction.Press -> {
                        release?.cancel()
                        pressedAt = System.currentTimeMillis()
                        origin = i.pressPosition
                        launch {
                            spread.snapTo(0f)
                            spread.animateTo(1f, Springs.spatial()) { invalidateDraw() }
                        }
                        launch { depth.animateTo(1f, Springs.pressIn()) { invalidateDraw() } }
                        launch { glow.animateTo(1f, Springs.effects()) { invalidateDraw() } }
                    }

                    is PressInteraction.Release, is PressInteraction.Cancel -> {
                        val held = System.currentTimeMillis() - pressedAt
                        release = launch {
                            if (held < Springs.MIN_VISIBLE_PRESS_MS) delay(Springs.MIN_VISIBLE_PRESS_MS - held)
                            launch { depth.animateTo(0f, Springs.pressOut()) { invalidateDraw() } }
                            launch { spread.animateTo(1.35f, Springs.spatial()) { invalidateDraw() } }
                            glow.animateTo(0f, tween(420, easing = Springs.EmphasizedDecelerate)) { invalidateDraw() }
                        }
                    }
                }
            }
        }
    }

    override fun ContentDrawScope.draw() {
        val d = depth.value
        val g = glow.value
        val f = focus.value
        if (d == 0f && g == 0f) {
            drawContent()
            if (f > 0f) drawRect(currentValueOf(LocalContentColor).copy(alpha = FOCUS_ALPHA * f))
            return
        }
        if (sink) {
            val shrink = min(0.06f, 3.dp.toPx() * 2f / max(1f, max(size.width, size.height)))
            val s = 1f - shrink * d
            scale(s, s) { this@draw.drawContent() }
        } else {
            drawContent()
        }
        if (f > 0f) drawRect(currentValueOf(LocalContentColor).copy(alpha = FOCUS_ALPHA * f))
        if (g > 0f) {
            val center = if (origin.isSpecified) origin else Offset(size.width / 2f, size.height / 2f)
            val reach = max(size.width, size.height) * (0.35f + 0.65f * spread.value)
            drawRect(
                Brush.radialGradient(
                    0f to Color.White.copy(alpha = 0.16f * g),
                    0.55f to Color.White.copy(alpha = 0.07f * g),
                    1f to Color.Transparent,
                    center = center,
                    radius = max(1f, reach)
                )
            )
        }
    }
}

fun Modifier.pressSquish(source: InteractionSource, pressedScale: Float = 0.965f): Modifier = composed {
    val scale = remember { Animatable(1f) }
    LaunchedEffect(source) {
        var pressedAt = 0L
        var release: Job? = null
        source.interactions.collect { i ->
            when (i) {
                is PressInteraction.Press -> {
                    release?.cancel()
                    pressedAt = System.currentTimeMillis()
                    launch { scale.animateTo(pressedScale, Springs.pressIn()) }
                }

                is PressInteraction.Release, is PressInteraction.Cancel -> release = launch {
                    val held = System.currentTimeMillis() - pressedAt
                    if (held < Springs.MIN_VISIBLE_PRESS_MS) delay(Springs.MIN_VISIBLE_PRESS_MS - held)
                    scale.animateTo(1f, Springs.pressOut())
                }
            }
        }
    }
    graphicsLayer {
        scaleX = scale.value
        scaleY = scale.value
    }
}

private const val STAGGER = 14

private const val ENTER_WINDOW_MS = 400L

@Composable
fun rememberEnterStart(): Long = remember { SystemClock.uptimeMillis() }

@Composable
fun Modifier.enter(index: Int, since: Long = -1L): Modifier {
    val done = rememberSaveable {
        booleanArrayOf(if (since < 0) index >= STAGGER else SystemClock.uptimeMillis() - since > ENTER_WINDOW_MS)
    }
    val a = remember { Animatable(if (done[0]) 1f else 0f) }
    val lift = with(LocalDensity.current) { 28.dp.toPx() }
    LaunchedEffect(Unit) {
        if (done[0]) return@LaunchedEffect
        delay(index.coerceAtMost(STAGGER) * 28L)
        a.animateTo(1f, spring(dampingRatio = 0.72f, stiffness = 320f))
        done[0] = true
    }
    return graphicsLayer {
        val v = a.value
        alpha = v.coerceIn(0f, 1f)
        translationY = (1f - v) * lift
        val s = 0.94f + 0.06f * v
        scaleX = s
        scaleY = s
    }
}

val LocalBarCover = staticCompositionLocalOf { 0.dp }

val LocalEdgeBelow = staticCompositionLocalOf { true }

fun Modifier.scrollEdge(state: LazyGridState, key: Any): Modifier = composed {
    val edge = with(LocalDensity.current) { 96.dp.toPx() }
    val cover = with(LocalDensity.current) { LocalBarCover.current.toPx() }
    val fadeBelow = LocalEdgeBelow.current
    graphicsLayer {
        val info = state.layoutInfo
        val item = info.visibleItemsInfo.firstOrNull { it.key == key } ?: return@graphicsLayer
        val h = item.size.height.toFloat()
        val top = item.offset.y.toFloat()
        val bottom = top + h
        val below = if (!fadeBelow) 1f else ((info.viewportEndOffset - cover - top) / (edge + h)).coerceIn(0f, 1f)
        val scrolled = if (state.firstVisibleItemIndex > 0) 1f else (state.firstVisibleItemScrollOffset / edge).coerceIn(0f, 1f)
        val above = 1f - scrolled * (1f - ((bottom - info.viewportStartOffset) / (edge + h)).coerceIn(0f, 1f))
        val k = FastOutSlowInEasing.transform(minOf(below, above))
        val sc = 0.86f + 0.14f * k
        scaleX = sc
        scaleY = sc
        alpha = 0.35f + 0.65f * k
    }
}

fun Modifier.remoteFocus(source: InteractionSource, shape: Shape, inward: Boolean = false): Modifier = composed {
    val focused by source.collectIsFocusedAsState()
    val alpha by animateFloatAsState(if (focused) 1f else 0f, Springs.effects(), label = "remoteFocus")
    val ring = MaterialTheme.colorScheme.secondary
    drawWithContent {
        drawContent()
        if (alpha == 0f) return@drawWithContent
        val w = 3.dp.toPx()
        val d = if (inward) -w / 2 else 2.dp.toPx() + w / 2
        val outline = shape.createOutline(Size(size.width + 2 * d, size.height + 2 * d), layoutDirection, this)
        translate(-d, -d) { drawOutline(outline, ring.copy(alpha = alpha), style = Stroke(w)) }
    }
}

enum class IconMotion {
    Turn,

    Drop,
    Lift,

    Shake,

    Pop,

    Flip
}

fun motionFor(icon: ImageVector): IconMotion? = when (icon) {
    Glyph.Bolt, Glyph.Auto, Glyph.Power, Glyph.Heart, Glyph.Star, Glyph.Battery, Glyph.Shield, Glyph.Pin -> IconMotion.Pop

    Glyph.Down, Glyph.Download -> IconMotion.Drop

    Glyph.Up, Glyph.Upload -> IconMotion.Lift

    Glyph.Swap, Glyph.Timer, Glyph.Sort, Glyph.Palette -> IconMotion.Turn

    Glyph.Globe, Glyph.Link, Glyph.Route, Glyph.Smartphone, Glyph.IpCheck -> IconMotion.Flip

    Glyph.Megaphone, Glyph.Block, Glyph.TimerOff, Glyph.Ping -> IconMotion.Shake

    else -> when (icon.name) {
        "Rounded.Notifications", "Rounded.NotificationsActive" -> IconMotion.Shake
        "Rounded.Refresh", "Rounded.Settings", "Rounded.Sync" -> IconMotion.Turn
        "Rounded.Favorite", "Rounded.Info" -> IconMotion.Pop
        "AutoMirrored.Rounded.Send" -> IconMotion.Lift
        else -> null
    }
}

fun Modifier.iconMotion(source: InteractionSource, motion: IconMotion?): Modifier = composed {
    val press = remember { Animatable(0f) }
    val turn = remember { Animatable(0f) }
    val shift = remember { Animatable(0f) }
    val squash = remember { Animatable(1f) }
    LaunchedEffect(source, motion) {
        var release: Job? = null
        source.interactions.collect { i ->
            when (i) {
                is PressInteraction.Press -> {
                    release?.cancel()
                    launch { press.animateTo(1f, Springs.pressIn()) }
                }

                is PressInteraction.Cancel -> release = launch { press.animateTo(0f, Springs.pressOut()) }

                is PressInteraction.Release -> release = launch {
                    launch { press.animateTo(0f, Springs.pressOut()) }
                    when (motion) {
                        null -> Unit

                        IconMotion.Turn -> turn.animateTo(turn.targetValue + 90f, spring(dampingRatio = 0.45f, stiffness = 300f))

                        IconMotion.Drop, IconMotion.Lift -> {
                            val d = if (motion == IconMotion.Drop) 3.5f else -3.5f
                            shift.animateTo(d, tween(120, easing = Springs.EmphasizedAccelerate))
                            shift.animateTo(0f, spring(dampingRatio = 0.3f, stiffness = 520f))
                        }

                        IconMotion.Pop -> {
                            squash.animateTo(0.7f, tween(90))
                            squash.animateTo(1f, spring(dampingRatio = 0.35f, stiffness = 480f))
                        }

                        IconMotion.Shake -> {
                            turn.snapTo(0f)
                            turn.animateTo(0f, spring(dampingRatio = 0.18f, stiffness = 700f), initialVelocity = 900f)
                        }

                        IconMotion.Flip -> {
                            turn.snapTo(0f)
                            turn.animateTo(360f, spring(dampingRatio = 0.6f, stiffness = 110f))
                            turn.snapTo(0f)
                        }
                    }
                }
            }
        }
    }
    graphicsLayer {
        val p = 1f - 0.14f * press.value
        scaleX = p
        scaleY = p * squash.value
        if (motion == IconMotion.Flip) {
            rotationY = turn.value
            cameraDistance = 10f * density
        } else {
            rotationZ = turn.value
        }
        translationY = shift.value * density
        if (motion == IconMotion.Pop) transformOrigin = TransformOrigin(0.5f, 1f)
    }
}

fun Modifier.popOnChange(on: Boolean?): Modifier = composed {
    val scale = remember { Animatable(1f) }
    val twist = remember { Animatable(0f) }
    var last by remember { mutableStateOf(on) }
    LaunchedEffect(on) {
        if (on == null || on == last) return@LaunchedEffect
        last = on
        if (on) {
            scale.snapTo(0.55f)
            twist.snapTo(-16f)
            launch { twist.animateTo(0f, spring(dampingRatio = 0.35f, stiffness = 420f)) }
            scale.animateTo(1f, spring(dampingRatio = 0.38f, stiffness = 520f))
        } else {
            scale.animateTo(0.8f, tween(90))
            scale.animateTo(1f, spring(dampingRatio = 0.5f, stiffness = 600f))
        }
    }
    graphicsLayer {
        scaleX = scale.value
        scaleY = scale.value
        rotationZ = twist.value
    }
}

fun Modifier.reflow(): Modifier = composed {
    val scope = rememberCoroutineScope()
    var target by remember { mutableStateOf<IntOffset?>(null) }
    var anim by remember { mutableStateOf<Animatable<IntOffset, AnimationVector2D>?>(null) }
    var from by remember { mutableStateOf(IntOffset.Zero) }
    onPlaced { target = it.positionInParent().round() }
        .offset {
            val t = target ?: return@offset IntOffset.Zero
            val a = anim ?: Animatable(t, IntOffset.VectorConverter).also { anim = it }
            when {
                a.targetValue == t -> Unit

                a.targetValue.y == t.y && !a.isRunning -> {
                    scope.launch { a.snapTo(t) }
                    return@offset IntOffset.Zero
                }

                else -> {
                    from = a.value - t
                    scope.launch { a.animateTo(t, spring(dampingRatio = 0.85f, stiffness = 700f)) }
                }
            }
            a.value - t
        }
        .graphicsLayer {
            val a = anim
            val t = target
            val d = hypot(from.x.toFloat(), from.y.toFloat())
            if (a != null && t != null && a.isRunning && d > 0f) {
                val left = (a.value - t).let { hypot(it.x.toFloat(), it.y.toFloat()) } / d
                alpha = 1f - 0.55f * sin(PI.toFloat() * (1f - left).coerceIn(0f, 1f))
            }
        }
}
