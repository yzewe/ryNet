package dev.yzewe.rynet.ui

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import kotlin.math.abs
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SeedTest {
    private val brand = 0xFF0B6B57.toInt()

    private fun roles(s: ColorScheme): Map<String, Color> = mapOf(
        "primary" to s.primary, "onPrimary" to s.onPrimary, "primaryContainer" to s.primaryContainer, "onPrimaryContainer" to s.onPrimaryContainer,
        "inversePrimary" to s.inversePrimary, "secondary" to s.secondary, "onSecondary" to s.onSecondary, "secondaryContainer" to s.secondaryContainer,
        "onSecondaryContainer" to s.onSecondaryContainer, "tertiary" to s.tertiary, "onTertiary" to s.onTertiary, "tertiaryContainer" to s.tertiaryContainer,
        "onTertiaryContainer" to s.onTertiaryContainer, "surface" to s.surface, "onSurface" to s.onSurface, "surfaceVariant" to s.surfaceVariant,
        "onSurfaceVariant" to s.onSurfaceVariant, "inverseSurface" to s.inverseSurface, "inverseOnSurface" to s.inverseOnSurface,
        "error" to s.error, "onError" to s.onError, "errorContainer" to s.errorContainer, "onErrorContainer" to s.onErrorContainer,
        "outline" to s.outline, "outlineVariant" to s.outlineVariant, "surfaceBright" to s.surfaceBright, "surfaceDim" to s.surfaceDim,
        "surfaceContainerLowest" to s.surfaceContainerLowest, "surfaceContainerLow" to s.surfaceContainerLow, "surfaceContainer" to s.surfaceContainer,
        "surfaceContainerHigh" to s.surfaceContainerHigh, "surfaceContainerHighest" to s.surfaceContainerHighest,
        "primaryFixed" to s.primaryFixed, "primaryFixedDim" to s.primaryFixedDim, "onPrimaryFixed" to s.onPrimaryFixed, "onPrimaryFixedVariant" to s.onPrimaryFixedVariant,
        "tertiaryFixed" to s.tertiaryFixed, "onTertiaryFixedVariant" to s.onTertiaryFixedVariant
    )

    private fun hex(c: Color) = "#%06X".format(c.toArgb() and 0xFFFFFF)

    private fun close(a: Color, b: Color, tol: Int): Boolean {
        val x = a.toArgb()
        val y = b.toArgb()
        return (0..2).all { i -> abs((x shr (8 * i) and 255) - (y shr (8 * i) and 255)) <= tol }
    }

    private fun check(expected: ColorScheme, actual: ColorScheme, tol: Int) {
        val e = roles(expected)
        val a = roles(actual)
        val bad = e.keys.filter { !close(e.getValue(it), a.getValue(it), tol) }.map { "$it: ${hex(e.getValue(it))} vs ${hex(a.getValue(it))}" }
        assertTrue(bad.joinToString("\n"), bad.isEmpty())
    }

    @Test fun brandLight() = check(Light, Seed.scheme(brand, dark = false), 3)

    @Test fun brandDark() = check(Dark, Seed.scheme(brand, dark = true), 3)

    @Test fun brandContrast() {
        check(LightMediumContrast, Seed.scheme(brand, dark = false, contrast = 1), 3)
        check(DarkMediumContrast, Seed.scheme(brand, dark = true, contrast = 1), 3)
        check(LightHighContrast, Seed.scheme(brand, dark = false, contrast = 2), 3)
        check(DarkHighContrast, Seed.scheme(brand, dark = true, contrast = 2), 3)
    }

    @Test fun hueRoundTrip() {
        for (h in listOf(0.0, 25.0, 90.0, 145.0, 210.0, 280.0, 330.0)) {
            val back = Seed.hue(Seed.fromHue(h))
            val d = abs(((back - h + 540) % 360) - 180)
            assertTrue("hue $h → $back", d < 2.0)
        }
    }

    @Test fun toneIsLstar() {
        for (t in listOf(10.0, 40.0, 90.0)) assertEquals(t, Hct.lstarFromArgb(Hct.toArgb(250.0, 36.0, t)), 0.5)
    }
}
