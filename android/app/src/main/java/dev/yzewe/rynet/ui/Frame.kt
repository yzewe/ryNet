package dev.yzewe.rynet.ui

import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.material.ripple.RippleAlpha
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RippleConfiguration
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.ThemeMode
import dev.yzewe.rynet.data.UiDebug

@Composable
@OptIn(ExperimentalMaterial3Api::class)
fun ComponentActivity.AppFrame(settings: Settings, content: @Composable () -> Unit) {
    val dark = when (settings.theme) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Light -> false
        ThemeMode.Dark, ThemeMode.Amoled -> true
    }
    SideEffect {
        val bar = if (dark) {
            SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
        } else {
            SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT)
        }
        enableEdgeToEdge(bar, bar)
    }
    val root = LocalView.current
    SideEffect {
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            root.importantForContentCapture = android.view.View.IMPORTANT_FOR_CONTENT_CAPTURE_NO_EXCLUDE_DESCENDANTS
        }
    }
    UiDebug.enabled = settings.uiDebug
    if (settings.uiDebug) ScreenReport()
    RynetTheme(dynamic = settings.wallpaperColors, dark = dark, amoled = settings.theme == ThemeMode.Amoled, seed = settings.seedColor) {
        val snackbars = remember { Snackbars() }
        val tv = remember { isTv(this) }
        CompositionLocalProvider(
            LocalSnackbars provides snackbars,
            LocalOrbStyle provides (OrbStyle.of(settings.orbStyle) ?: OrbStyle.Wave),
            LocalSectionStyle provides SectionStyle.animated(settings),
            LocalContentColor provides MaterialTheme.colorScheme.onSurface,
            LocalRippleConfiguration provides if (tv) TV_RIPPLE else RippleConfiguration()
        ) {
            Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
                content()
                SnackbarHost(snackbars)
            }
        }
    }
}

@Composable
private fun ComponentActivity.ScreenReport() {
    val cfg = LocalConfiguration.current
    val density = LocalDensity.current
    val bars = WindowInsets.systemBars
    val cut = WindowInsets.displayCutout
    val ime = WindowInsets.ime
    val gestures = WindowInsets.systemGestures
    val info = with(density) {
        listOf(
            "bars" to "t=${bars.getTop(this).toDp()} b=${bars.getBottom(this).toDp()}",
            "cutout" to "t=${cut.getTop(this).toDp()} l=${cut.getLeft(this, androidx.compose.ui.unit.LayoutDirection.Ltr).toDp()}",
            "ime" to ime.getBottom(this).toDp().toString(),
            "gestures" to "b=${gestures.getBottom(this).toDp()}"
        )
    }
    LaunchedEffect(cfg, info) {
        kotlinx.coroutines.delay(500)
        val m = resources.displayMetrics
        val win = if (android.os.Build.VERSION.SDK_INT >= 30) windowManager.currentWindowMetrics.bounds.let { "${it.width()}x${it.height()}" } else null
        UiDebug.log(
            this@ScreenReport, "screen",
            *UiDebug.device(),
            "activity" to this@ScreenReport.javaClass.simpleName,
            "screenDp" to "${cfg.screenWidthDp}x${cfg.screenHeightDp}",
            "windowPx" to win,
            "displayPx" to "${m.widthPixels}x${m.heightPixels}",
            "density" to m.density,
            "densityDpi" to m.densityDpi,
            "stableDpi" to android.util.DisplayMetrics.DENSITY_DEVICE_STABLE,
            "fontScale" to cfg.fontScale,
            "orientation" to cfg.orientation,
            *info.toTypedArray()
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
private val TV_RIPPLE = RippleConfiguration(rippleAlpha = RippleAlpha(draggedAlpha = 0.16f, focusedAlpha = 0.24f, hoveredAlpha = 0.08f, pressedAlpha = 0.16f))
