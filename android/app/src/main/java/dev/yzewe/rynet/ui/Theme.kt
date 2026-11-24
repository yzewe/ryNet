package dev.yzewe.rynet.ui

import android.app.UiModeManager
import android.content.Context
import android.os.Build
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import kotlin.math.cbrt
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sin

@Immutable
class StatusColors(val warning: Color, val onWarning: Color, val warningContainer: Color, val onWarningContainer: Color)

private val LocalStatusColors = staticCompositionLocalOf { LightStatus }

val ColorScheme.warning: Color
    @Composable @ReadOnlyComposable
    get() = LocalStatusColors.current.warning

val ColorScheme.warningContainer: Color
    @Composable @ReadOnlyComposable
    get() = LocalStatusColors.current.warningContainer

val ColorScheme.onWarningContainer: Color
    @Composable @ReadOnlyComposable
    get() = LocalStatusColors.current.onWarningContainer

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun RynetTheme(dynamic: Boolean = true, dark: Boolean = isSystemInDarkTheme(), amoled: Boolean = false, seed: Int = 0, content: @Composable () -> Unit) {
    val ctx = LocalContext.current
    val target = remember(dynamic, seed, dark, amoled) {
        val base = when {
            dynamic && Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
            seed != 0 -> Seed.scheme(seed, dark, contrastLevel(contrast(ctx)))
            else -> staticScheme(dark, contrast(ctx))
        }
        if (amoled && dark) amoled(base) else base
    }
    val scheme = target.crossfaded()
    val status = remember(target, dark) { (if (dark) DarkStatus else LightStatus).harmonizedTo(target.primary) }
    CompositionLocalProvider(LocalStatusColors provides status) {
        MaterialExpressiveTheme(colorScheme = scheme, motionScheme = MotionScheme.expressive(), content = content)
    }
}

private fun contrastLevel(contrast: Float): Int = when {
    contrast >= 0.75f -> 2
    contrast >= 0.25f -> 1
    else -> 0
}

@Composable
private fun ColorScheme.crossfaded(): ColorScheme {
    val spec = remember { tween<Color>(DURATION_COLORS, easing = FastOutSlowInEasing) }

    @Composable
    fun a(c: Color): Color = animateColorAsState(c, spec, label = "scheme").value
    return copy(
        primary = a(primary), onPrimary = a(onPrimary), primaryContainer = a(primaryContainer), onPrimaryContainer = a(onPrimaryContainer),
        inversePrimary = a(inversePrimary), secondary = a(secondary), onSecondary = a(onSecondary), secondaryContainer = a(secondaryContainer),
        onSecondaryContainer = a(onSecondaryContainer), tertiary = a(tertiary), onTertiary = a(onTertiary), tertiaryContainer = a(tertiaryContainer),
        onTertiaryContainer = a(onTertiaryContainer), background = a(background), onBackground = a(onBackground), surface = a(surface),
        onSurface = a(onSurface), surfaceVariant = a(surfaceVariant), onSurfaceVariant = a(onSurfaceVariant), surfaceTint = a(surfaceTint),
        inverseSurface = a(inverseSurface), inverseOnSurface = a(inverseOnSurface), error = a(error), onError = a(onError),
        errorContainer = a(errorContainer), onErrorContainer = a(onErrorContainer), outline = a(outline), outlineVariant = a(outlineVariant),
        surfaceBright = a(surfaceBright), surfaceDim = a(surfaceDim), surfaceContainer = a(surfaceContainer),
        surfaceContainerHigh = a(surfaceContainerHigh), surfaceContainerHighest = a(surfaceContainerHighest), surfaceContainerLow = a(surfaceContainerLow),
        surfaceContainerLowest = a(surfaceContainerLowest),
        primaryFixed = a(primaryFixed), primaryFixedDim = a(primaryFixedDim), onPrimaryFixed = a(onPrimaryFixed), onPrimaryFixedVariant = a(onPrimaryFixedVariant),
        secondaryFixed = a(secondaryFixed), secondaryFixedDim = a(secondaryFixedDim), onSecondaryFixed = a(onSecondaryFixed), onSecondaryFixedVariant = a(onSecondaryFixedVariant),
        tertiaryFixed = a(tertiaryFixed), tertiaryFixedDim = a(tertiaryFixedDim), onTertiaryFixed = a(onTertiaryFixed), onTertiaryFixedVariant = a(onTertiaryFixedVariant)
    )
}

private const val DURATION_COLORS = 450

private fun staticScheme(dark: Boolean, contrast: Float): ColorScheme = when {
    contrast >= 0.75f -> if (dark) DarkHighContrast else LightHighContrast
    contrast >= 0.25f -> if (dark) DarkMediumContrast else LightMediumContrast
    else -> if (dark) Dark else Light
}

private fun contrast(ctx: Context): Float = if (Build.VERSION.SDK_INT >= 34) ctx.getSystemService(UiModeManager::class.java)?.contrast ?: 0f else 0f

private fun amoled(s: ColorScheme): ColorScheme {
    fun dim(c: Color, k: Float) = lerp(Color.Black, c, k)
    return s.copy(
        background = Color.Black,
        surface = Color.Black,
        surfaceDim = Color.Black,
        surfaceContainerLowest = Color.Black,
        surfaceContainerLow = Color.Black,
        surfaceBright = dim(s.surfaceBright, 0.8f),
        surfaceContainer = dim(s.surfaceContainer, 0.7f),
        surfaceContainerHigh = dim(s.surfaceContainerHigh, 0.78f),
        surfaceContainerHighest = dim(s.surfaceContainerHighest, 0.85f)
    )
}

private fun StatusColors.harmonizedTo(primary: Color): StatusColors {
    fun h(c: Color) = harmonize(c, primary)
    return StatusColors(h(warning), h(onWarning), h(warningContainer), h(onWarningContainer))
}

private fun harmonize(c: Color, toward: Color): Color {
    val (l, ch, h) = lch(c)
    val target = lch(toward).third
    val diff = ((target - h + 540) % 360) - 180
    val rotation = minOf(kotlin.math.abs(diff) * 0.5, 15.0) * kotlin.math.sign(diff)
    return fromLch(l, ch, h + rotation).copy(alpha = c.alpha)
}

private fun lch(c: Color): Triple<Double, Double, Double> {
    fun lin(v: Float) = if (v <= 0.04045f) v / 12.92 else ((v + 0.055) / 1.055).pow(2.4)
    val r = lin(c.red)
    val g = lin(c.green)
    val b = lin(c.blue)
    val x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047
    val y = 0.2126 * r + 0.7152 * g + 0.0722 * b
    val z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883
    fun f(t: Double) = if (t > 216.0 / 24389) cbrt(t) else (24389.0 / 27 * t + 16) / 116
    val l = 116 * f(y) - 16
    val a = 500 * (f(x) - f(y))
    val bb = 200 * (f(y) - f(z))
    return Triple(l, kotlin.math.hypot(a, bb), Math.toDegrees(kotlin.math.atan2(bb, a)))
}

private fun fromLch(l: Double, chroma: Double, hueDeg: Double): Color {
    var c = chroma
    while (true) {
        val rgb = lchToRgb(l, c, hueDeg)
        if (rgb != null || c <= 0.0) return rgb ?: lchToRgb(l, 0.0, hueDeg)!!
        c -= 1.0
    }
}

private fun lchToRgb(l: Double, c: Double, hueDeg: Double): Color? {
    val h = Math.toRadians(hueDeg)
    val a = c * cos(h)
    val b = c * sin(h)
    val fy = (l + 16) / 116
    val fx = fy + a / 500
    val fz = fy - b / 200
    fun inv(t: Double) = if (t * t * t > 216.0 / 24389) t * t * t else (116 * t - 16) / (24389.0 / 27)
    val x = inv(fx) * 0.95047
    val y = if (l > 8) fy * fy * fy else l / (24389.0 / 27)
    val z = inv(fz) * 1.08883
    val r = 3.2406 * x - 1.5372 * y - 0.4986 * z
    val g = -0.9689 * x + 1.8758 * y + 0.0415 * z
    val bl = 0.0557 * x - 0.2040 * y + 1.0570 * z
    val e = 1e-4
    if (r < -e || g < -e || bl < -e || r > 1 + e || g > 1 + e || bl > 1 + e) return null
    fun gam(v: Double): Float {
        val u = v.coerceIn(0.0, 1.0)
        return (if (u <= 0.0031308) 12.92 * u else 1.055 * u.pow(1 / 2.4) - 0.055).toFloat()
    }
    return Color(gam(r), gam(g), gam(bl))
}
