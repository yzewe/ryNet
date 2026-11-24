package dev.yzewe.rynet.ui

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cbrt
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sign
import kotlin.math.sin
import kotlin.math.sqrt

object Seed {
    fun fromHue(hue: Double): Int = Hct.toArgb(hue, 48.0, 50.0)

    fun hue(argb: Int): Double = Cam16.fromArgb(argb).hue

    fun swatch(argb: Int, dark: Boolean): Triple<Color, Color, Color> {
        val h = hue(argb)
        return Triple(
            Color(Hct.toArgb(h, 36.0, if (dark) 80.0 else 40.0)),
            Color(Hct.toArgb(h, 16.0, if (dark) 30.0 else 90.0)),
            Color(Hct.toArgb(h + 60.0, 24.0, if (dark) 30.0 else 90.0))
        )
    }

    fun accent(argb: Int): Int = Hct.toArgb(hue(argb), 36.0, 40.0)

    fun scheme(argb: Int, dark: Boolean, contrast: Int = 0): ColorScheme {
        val h = hue(argb)
        val k = contrast.coerceIn(0, 2) + if (dark) 3 else 0
        val p = Palette(h, 36.0)
        val s = Palette(h, 16.0)
        val t = Palette(h + 60.0, 24.0)
        val n = Palette(h, 6.0)
        val nv = Palette(h, 8.0)
        val brand = when (contrast) {
            2 -> if (dark) DarkHighContrast else LightHighContrast
            1 -> if (dark) DarkMediumContrast else LightMediumContrast
            else -> if (dark) Dark else Light
        }
        fun T.of(pal: Palette) = pal.tone(tones[k])
        val surface = n.tone(if (dark) 6.0 else 98.0)
        val onSurface = ON_SURFACE.of(n)
        val args = Roles(
            primary = PRIMARY.of(p), onPrimary = ON_ACCENT.of(p), primaryContainer = CONTAINER.of(p), onPrimaryContainer = ON_CONTAINER.of(p),
            inversePrimary = INVERSE_PRIMARY.of(p),
            secondary = PRIMARY.of(s), onSecondary = ON_ACCENT.of(s), secondaryContainer = CONTAINER.of(s), onSecondaryContainer = ON_CONTAINER.of(s),
            tertiary = PRIMARY.of(t), onTertiary = ON_ACCENT.of(t), tertiaryContainer = CONTAINER.of(t), onTertiaryContainer = ON_CONTAINER.of(t),
            surface = surface, onSurface = onSurface,
            surfaceVariant = nv.tone(if (dark) 30.0 else 90.0), onSurfaceVariant = ON_SURFACE_VARIANT.of(nv),
            surfaceTint = p.tone(if (dark) 80.0 else 40.0),
            inverseSurface = n.tone(if (dark) 90.0 else 20.0), inverseOnSurface = INVERSE_ON_SURFACE.of(n),
            error = brand.error, onError = brand.onError, errorContainer = brand.errorContainer, onErrorContainer = brand.onErrorContainer,
            outline = OUTLINE.of(nv), outlineVariant = OUTLINE_VARIANT.of(nv),
            surfaceBright = SURFACE_BRIGHT.of(n), surfaceDim = SURFACE_DIM.of(n),
            surfaceContainerLowest = LOWEST.of(n), surfaceContainerLow = LOW.of(n), surfaceContainer = MID.of(n),
            surfaceContainerHigh = HIGH.of(n), surfaceContainerHighest = HIGHEST.of(n),
            primaryFixed = FIXED.of(p), primaryFixedDim = FIXED_DIM.of(p), onPrimaryFixed = ON_FIXED.of(p), onPrimaryFixedVariant = ON_FIXED_VARIANT.of(p),
            secondaryFixed = FIXED.of(s), secondaryFixedDim = FIXED_DIM.of(s), onSecondaryFixed = ON_FIXED.of(s), onSecondaryFixedVariant = ON_FIXED_VARIANT.of(s),
            tertiaryFixed = FIXED.of(t), tertiaryFixedDim = FIXED_DIM.of(t), onTertiaryFixed = ON_FIXED.of(t), onTertiaryFixedVariant = ON_FIXED_VARIANT.of(t)
        )
        return args.build(dark)
    }

    private class T(vararg val tones: Double)

    private val PRIMARY = T(40.0, 22.6, 18.0, 80.0, 87.8, 94.7)
    private val ON_ACCENT = T(100.0, 100.0, 100.0, 20.0, 15.0, 0.0)
    private val CONTAINER = T(90.0, 46.0, 31.0, 30.0, 60.0, 78.6)
    private val ON_CONTAINER = T(30.0, 100.0, 100.0, 90.0, 0.0, 3.0)
    private val INVERSE_PRIMARY = T(80.0, 80.0, 80.0, 40.0, 30.5, 30.5)
    private val ON_SURFACE = T(10.0, 5.0, 0.0, 90.0, 100.0, 100.0)
    private val ON_SURFACE_VARIANT = T(30.0, 22.6, 0.0, 80.0, 87.9, 100.0)
    private val INVERSE_ON_SURFACE = T(95.0, 95.0, 100.0, 20.0, 16.9, 0.0)
    private val OUTLINE = T(50.0, 35.2, 18.0, 60.0, 72.4, 94.6)
    private val OUTLINE_VARIANT = T(80.0, 46.0, 31.0, 30.0, 60.0, 78.6)
    private val SURFACE_BRIGHT = T(98.0, 98.0, 98.0, 24.0, 29.0, 34.0)
    private val SURFACE_DIM = T(87.0, 80.0, 75.0, 6.0, 6.0, 6.0)
    private val LOWEST = T(100.0, 100.0, 100.0, 4.0, 2.0, 0.0)
    private val LOW = T(96.0, 96.0, 95.0, 10.0, 11.0, 12.0)
    private val MID = T(94.0, 92.0, 90.0, 12.0, 16.0, 20.0)
    private val HIGH = T(92.0, 88.0, 85.0, 17.0, 21.0, 25.0)
    private val HIGHEST = T(90.0, 84.0, 80.0, 22.0, 26.0, 30.0)
    private val FIXED = T(90.0, 46.0, 31.0, 90.0, 90.0, 90.0)
    private val FIXED_DIM = T(80.0, 36.2, 21.0, 80.0, 80.0, 80.0)
    private val ON_FIXED = T(10.0, 100.0, 100.0, 10.0, 5.2, 0.0)
    private val ON_FIXED_VARIANT = T(30.0, 100.0, 100.0, 30.0, 22.6, 5.2)

    private class Palette(hue: Double, private val chroma: Double) {
        private val hue = ((hue % 360.0) + 360.0) % 360.0
        private val cache = HashMap<Double, Color>()
        fun tone(t: Double): Color = cache.getOrPut(t) { Color(Hct.toArgb(hue, chroma, t)) }
    }

    private class Roles(
        val primary: Color,
        val onPrimary: Color,
        val primaryContainer: Color,
        val onPrimaryContainer: Color,
        val inversePrimary: Color,
        val secondary: Color,
        val onSecondary: Color,
        val secondaryContainer: Color,
        val onSecondaryContainer: Color,
        val tertiary: Color,
        val onTertiary: Color,
        val tertiaryContainer: Color,
        val onTertiaryContainer: Color,
        val surface: Color,
        val onSurface: Color,
        val surfaceVariant: Color,
        val onSurfaceVariant: Color,
        val surfaceTint: Color,
        val inverseSurface: Color,
        val inverseOnSurface: Color,
        val error: Color,
        val onError: Color,
        val errorContainer: Color,
        val onErrorContainer: Color,
        val outline: Color,
        val outlineVariant: Color,
        val surfaceBright: Color,
        val surfaceDim: Color,
        val surfaceContainerLowest: Color,
        val surfaceContainerLow: Color,
        val surfaceContainer: Color,
        val surfaceContainerHigh: Color,
        val surfaceContainerHighest: Color,
        val primaryFixed: Color,
        val primaryFixedDim: Color,
        val onPrimaryFixed: Color,
        val onPrimaryFixedVariant: Color,
        val secondaryFixed: Color,
        val secondaryFixedDim: Color,
        val onSecondaryFixed: Color,
        val onSecondaryFixedVariant: Color,
        val tertiaryFixed: Color,
        val tertiaryFixedDim: Color,
        val onTertiaryFixed: Color,
        val onTertiaryFixedVariant: Color
    ) {
        fun build(dark: Boolean): ColorScheme = if (dark) {
            darkColorScheme(
                primary = primary, onPrimary = onPrimary, primaryContainer = primaryContainer, onPrimaryContainer = onPrimaryContainer,
                inversePrimary = inversePrimary, secondary = secondary, onSecondary = onSecondary, secondaryContainer = secondaryContainer,
                onSecondaryContainer = onSecondaryContainer, tertiary = tertiary, onTertiary = onTertiary, tertiaryContainer = tertiaryContainer,
                onTertiaryContainer = onTertiaryContainer, background = surface, onBackground = onSurface, surface = surface, onSurface = onSurface,
                surfaceVariant = surfaceVariant, onSurfaceVariant = onSurfaceVariant, surfaceTint = surfaceTint, inverseSurface = inverseSurface,
                inverseOnSurface = inverseOnSurface, error = error, onError = onError, errorContainer = errorContainer, onErrorContainer = onErrorContainer,
                outline = outline, outlineVariant = outlineVariant, scrim = Color.Black, surfaceBright = surfaceBright, surfaceContainer = surfaceContainer,
                surfaceContainerHigh = surfaceContainerHigh, surfaceContainerHighest = surfaceContainerHighest, surfaceContainerLow = surfaceContainerLow,
                surfaceContainerLowest = surfaceContainerLowest, surfaceDim = surfaceDim,
                primaryFixed = primaryFixed, primaryFixedDim = primaryFixedDim, onPrimaryFixed = onPrimaryFixed, onPrimaryFixedVariant = onPrimaryFixedVariant,
                secondaryFixed = secondaryFixed, secondaryFixedDim = secondaryFixedDim, onSecondaryFixed = onSecondaryFixed, onSecondaryFixedVariant = onSecondaryFixedVariant,
                tertiaryFixed = tertiaryFixed, tertiaryFixedDim = tertiaryFixedDim, onTertiaryFixed = onTertiaryFixed, onTertiaryFixedVariant = onTertiaryFixedVariant
            )
        } else {
            lightColorScheme(
                primary = primary, onPrimary = onPrimary, primaryContainer = primaryContainer, onPrimaryContainer = onPrimaryContainer,
                inversePrimary = inversePrimary, secondary = secondary, onSecondary = onSecondary, secondaryContainer = secondaryContainer,
                onSecondaryContainer = onSecondaryContainer, tertiary = tertiary, onTertiary = onTertiary, tertiaryContainer = tertiaryContainer,
                onTertiaryContainer = onTertiaryContainer, background = surface, onBackground = onSurface, surface = surface, onSurface = onSurface,
                surfaceVariant = surfaceVariant, onSurfaceVariant = onSurfaceVariant, surfaceTint = surfaceTint, inverseSurface = inverseSurface,
                inverseOnSurface = inverseOnSurface, error = error, onError = onError, errorContainer = errorContainer, onErrorContainer = onErrorContainer,
                outline = outline, outlineVariant = outlineVariant, scrim = Color.Black, surfaceBright = surfaceBright, surfaceContainer = surfaceContainer,
                surfaceContainerHigh = surfaceContainerHigh, surfaceContainerHighest = surfaceContainerHighest, surfaceContainerLow = surfaceContainerLow,
                surfaceContainerLowest = surfaceContainerLowest, surfaceDim = surfaceDim,
                primaryFixed = primaryFixed, primaryFixedDim = primaryFixedDim, onPrimaryFixed = onPrimaryFixed, onPrimaryFixedVariant = onPrimaryFixedVariant,
                secondaryFixed = secondaryFixed, secondaryFixedDim = secondaryFixedDim, onSecondaryFixed = onSecondaryFixed, onSecondaryFixedVariant = onSecondaryFixedVariant,
                tertiaryFixed = tertiaryFixed, tertiaryFixedDim = tertiaryFixedDim, onTertiaryFixed = onTertiaryFixed, onTertiaryFixedVariant = onTertiaryFixedVariant
            )
        }
    }
}

internal object Hct {
    fun toArgb(hue: Double, chroma: Double, tone: Double): Int {
        if (chroma < 1.0 || tone.roundToInt() <= 0 || tone.roundToInt() >= 100) return argbFromLstar(tone)
        val h = ((hue % 360.0) + 360.0) % 360.0
        var high = chroma
        var mid = chroma
        var low = 0.0
        var first = true
        var answer: Cam16? = null
        while (abs(low - high) >= 0.4) {
            val possible = findByJ(h, mid, tone)
            if (first) {
                if (possible != null) return possible.toArgb()
                first = false
                mid = low + (high - low) / 2
                continue
            }
            if (possible == null) {
                high = mid
            } else {
                answer = possible
                low = mid
            }
            mid = low + (high - low) / 2
        }
        return answer?.toArgb() ?: argbFromLstar(tone)
    }

    private fun findByJ(hue: Double, chroma: Double, tone: Double): Cam16? {
        var low = 0.0
        var high = 100.0
        var bestDl = 1000.0
        var bestDe = 1000.0
        var best: Cam16? = null
        while (abs(low - high) > 0.01) {
            val mid = low + (high - low) / 2
            val clipped = Cam16.fromJch(mid, chroma, hue).toArgb()
            val l = lstarFromArgb(clipped)
            val dl = abs(tone - l)
            if (dl < 0.2) {
                val cam = Cam16.fromArgb(clipped)
                val de = cam.distance(Cam16.fromJch(cam.j, cam.chroma, hue))
                if (de <= 1.0 && de <= bestDe) {
                    bestDl = dl
                    bestDe = de
                    best = cam
                }
            }
            if (bestDl == 0.0 && bestDe == 0.0) break
            if (l < tone) low = mid else high = mid
        }
        return best
    }

    fun lstarFromArgb(argb: Int): Double {
        val y = 0.2126 * lin(argb shr 16 and 255) + 0.7152 * lin(argb shr 8 and 255) + 0.0722 * lin(argb and 255)
        return lstarFromY(y * 100)
    }

    fun lstarFromY(y: Double): Double {
        val t = y / 100
        return if (t > 216.0 / 24389) 116 * cbrt(t) - 16 else 24389.0 / 27 * t
    }

    fun yFromLstar(l: Double): Double {
        val ft = (l + 16) / 116
        val ft3 = ft * ft * ft
        return 100 * (if (ft3 > 216.0 / 24389) ft3 else (116 * ft - 16) / (24389.0 / 27))
    }

    fun argbFromLstar(l: Double): Int {
        val c = delin(yFromLstar(l) / 100)
        return (0xFF shl 24) or (c shl 16) or (c shl 8) or c
    }

    fun lin(c: Int): Double {
        val v = c / 255.0
        return if (v <= 0.040449936) v / 12.92 else ((v + 0.055) / 1.055).pow(2.4)
    }

    fun delin(v: Double): Int {
        val n = if (v <= 0.0031308) v * 12.92 else 1.055 * v.pow(1 / 2.4) - 0.055
        return (n * 255).roundToInt().coerceIn(0, 255)
    }
}

internal class Cam16(val hue: Double, val chroma: Double, val j: Double, private val jstar: Double, private val astar: Double, private val bstar: Double) {
    fun distance(o: Cam16): Double {
        val dj = jstar - o.jstar
        val da = astar - o.astar
        val db = bstar - o.bstar
        return 1.41 * sqrt(dj * dj + da * da + db * db).pow(0.63)
    }

    fun toArgb(): Int {
        val alpha = if (chroma == 0.0 || j == 0.0) 0.0 else chroma / sqrt(j / 100)
        val t = (alpha / (1.64 - 0.29.pow(V.n)).pow(0.73)).pow(1 / 0.9)
        val hRad = Math.toRadians(hue)
        val eHue = 0.25 * (cos(hRad + 2) + 3.8)
        val ac = V.aw * (j / 100).pow(1 / V.c / V.z)
        val p1 = eHue * (50000.0 / 13) * V.nc * V.ncb
        val p2 = ac / V.nbb
        val hSin = sin(hRad)
        val hCos = cos(hRad)
        val gamma = 23 * (p2 + 0.305) * t / (23 * p1 + 11 * t * hCos + 108 * t * hSin)
        val a = gamma * hCos
        val b = gamma * hSin
        val rA = (460 * p2 + 451 * a + 288 * b) / 1403
        val gA = (460 * p2 - 891 * a - 261 * b) / 1403
        val bA = (460 * p2 - 220 * a - 6300 * b) / 1403
        fun un(x: Double, d: Double): Double {
            val base = maxOf(0.0, 27.13 * abs(x) / (400 - abs(x)))
            return sign(x) * (100 / V.fl) * base.pow(1 / 0.42) / d
        }
        val rF = un(rA, V.rgbD[0])
        val gF = un(gA, V.rgbD[1])
        val bF = un(bA, V.rgbD[2])
        val x = 1.86206786 * rF - 1.01125463 * gF + 0.14918677 * bF
        val y = 0.38752654 * rF + 0.62144744 * gF - 0.00897398 * bF
        val z = -0.01584150 * rF - 0.03412294 * gF + 1.04996444 * bF
        val r = 3.2413774792388685 * x - 1.5376652402851851 * y - 0.49885366846268053 * z
        val g = -0.9691452513005321 * x + 1.8758853451067872 * y + 0.04156585616912061 * z
        val bl = 0.05562093689691305 * x - 0.20395524564742123 * y + 1.0571799111220335 * z
        return (0xFF shl 24) or (Hct.delin(r / 100) shl 16) or (Hct.delin(g / 100) shl 8) or Hct.delin(bl / 100)
    }

    companion object {
        fun fromArgb(argb: Int): Cam16 {
            val r = Hct.lin(argb shr 16 and 255) * 100
            val g = Hct.lin(argb shr 8 and 255) * 100
            val b = Hct.lin(argb and 255) * 100
            val x = 0.41233895 * r + 0.35762064 * g + 0.18051042 * b
            val y = 0.2126 * r + 0.7152 * g + 0.0722 * b
            val z = 0.01932141 * r + 0.11916382 * g + 0.95034478 * b
            val rC = 0.401288 * x + 0.650173 * y - 0.051461 * z
            val gC = -0.250268 * x + 1.204414 * y + 0.045854 * z
            val bC = -0.002079 * x + 0.048952 * y + 0.953127 * z
            fun adapt(c: Double, d: Double): Double {
                val v = d * c
                val f = (V.fl * abs(v) / 100).pow(0.42)
                return sign(v) * 400 * f / (f + 27.13)
            }
            val rA = adapt(rC, V.rgbD[0])
            val gA = adapt(gC, V.rgbD[1])
            val bA = adapt(bC, V.rgbD[2])
            val a = (11 * rA - 12 * gA + bA) / 11
            val bb = (rA + gA - 2 * bA) / 9
            val u = (20 * rA + 20 * gA + 21 * bA) / 20
            val p2 = (40 * rA + 20 * gA + bA) / 20
            val deg = Math.toDegrees(atan2(bb, a))
            val hue = if (deg < 0) {
                deg + 360
            } else if (deg >= 360) {
                deg - 360
            } else {
                deg
            }
            val ac = p2 * V.nbb
            val j = 100 * (ac / V.aw).pow(V.c * V.z)
            val huePrime = if (hue < 20.14) hue + 360 else hue
            val eHue = 0.25 * (cos(Math.toRadians(huePrime) + 2) + 3.8)
            val p1 = 50000.0 / 13 * eHue * V.nc * V.ncb
            val t = p1 * hypot(a, bb) / (u + 0.305)
            val alpha = (1.64 - 0.29.pow(V.n)).pow(0.73) * t.pow(0.9)
            val c = alpha * sqrt(j / 100)
            return of(j, c, hue)
        }

        fun fromJch(j: Double, c: Double, h: Double): Cam16 = of(j, c, h)

        private fun of(j: Double, c: Double, h: Double): Cam16 {
            val m = c * V.fLRoot
            val jstar = (1 + 100 * 0.007) * j / (1 + 0.007 * j)
            val mstar = 1 / 0.0228 * ln(1 + 0.0228 * m)
            val hRad = Math.toRadians(h)
            return Cam16(h, c, j, jstar, mstar * cos(hRad), mstar * sin(hRad))
        }
    }

    private object V {
        private val white = doubleArrayOf(95.047, 100.0, 108.883)
        private val la = 200.0 / PI * Hct.yFromLstar(50.0) / 100
        const val f = 1.0
        val c = 0.69
        val nc = f
        val rgbD: DoubleArray
        val fl: Double
        val fLRoot: Double
        val n = Hct.yFromLstar(50.0) / white[1]
        val z = 1.48 + sqrt(n)
        val nbb = 0.725 / n.pow(0.2)
        val ncb = nbb
        val aw: Double

        init {
            val rW = white[0] * 0.401288 + white[1] * 0.650173 + white[2] * -0.051461
            val gW = white[0] * -0.250268 + white[1] * 1.204414 + white[2] * 0.045854
            val bW = white[0] * -0.002079 + white[1] * 0.048952 + white[2] * 0.953127
            val d = (f * (1 - (1 / 3.6) * exp((-la - 42) / 92))).coerceIn(0.0, 1.0)
            rgbD = doubleArrayOf(d * (100 / rW) + 1 - d, d * (100 / gW) + 1 - d, d * (100 / bW) + 1 - d)
            val k = 1 / (5 * la + 1)
            val k4 = k * k * k * k
            val k4F = 1 - k4
            fl = k4 * la + 0.1 * k4F * k4F * cbrt(5 * la)
            fLRoot = fl.pow(0.25)
            fun a(w: Double, dd: Double): Double {
                val x = (fl * dd * w / 100).pow(0.42)
                return 400 * x / (x + 27.13)
            }
            aw = (2 * a(rW, rgbD[0]) + a(gW, rgbD[1]) + 0.05 * a(bW, rgbD[2])) * nbb
        }
    }
}
