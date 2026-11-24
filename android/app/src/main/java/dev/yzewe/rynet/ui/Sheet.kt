package dev.yzewe.rynet.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import android.view.View
import android.view.Window
import android.view.WindowManager
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.LocalOverscrollFactory
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusTarget
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.findRootCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.graphics.ColorUtils
import androidx.core.view.WindowCompat
import dev.yzewe.rynet.data.UiDebug
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun AppSheet(
    title: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    topDivider: Boolean = false,
    bottomDivider: Boolean = false,
    maxHeight: Dp = 824.dp,
    fillMaxHeight: Boolean = false,
    dismissRequestId: Long = 0L,
    edgeToEdgeBottom: Boolean = false,
    animateHeight: Boolean = false,
    imePadding: Boolean = false,
    footer: @Composable () -> Unit = {},
    overlay: @Composable BoxScope.() -> Unit = {},
    content: @Composable () -> Unit
) {
    val latestOnDismiss by rememberUpdatedState(onDismiss)
    val scope = rememberCoroutineScope()
    var dismissRequested by remember { mutableStateOf(false) }
    var wasVisible by remember { mutableStateOf(false) }
    var scrimVisible by remember { mutableStateOf(false) }

    val topAlpha by animateFloatAsState(if (topDivider) 1f else 0f, tween(160), label = "top")
    val bottomAlpha by animateFloatAsState(if (bottomDivider) 1f else 0f, tween(160), label = "bottom")
    val scrim by animateFloatAsState(if (scrimVisible && !dismissRequested) 1f else 0f, tween(320), label = "scrim")
    val bgAlpha by animateFloatAsState(if (!dismissRequested) 1f else 0f, tween(220), label = "bg")
    val scrimColor = Color.Black.copy(alpha = 0.32f * scrim)

    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    val density = LocalDensity.current
    val windowH = with(density) { LocalWindowInfo.current.containerSize.height.toDp() }
    val statusTop = with(density) { WindowInsets.statusBars.getTop(this).toDp() }
    val cap = if (windowH > 0.dp) minOf(maxHeight, windowH - statusTop - SHEET_TOP_GAP) else maxHeight
    val ctx = LocalContext.current
    val loggedHeight = remember { intArrayOf(-1) }
    if (UiDebug.enabled) {
        LaunchedEffect(title, cap) {
            UiDebug.log(ctx, "sheet", "title" to title, "maxHeight" to maxHeight.value, "window" to windowH.value, "statusBar" to statusTop.value, "cap" to cap.value, "fill" to fillMaxHeight)
        }
        LaunchedEffect(state.currentValue) {
            UiDebug.log(ctx, "sheet-state", "title" to title, "value" to state.currentValue.name, "offsetPx" to runCatching { state.requireOffset() }.getOrNull()?.toDouble())
        }
    }

    val bounds = remember {
        object : NestedScrollConnection {
            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset = if (available.y < 0f) Offset(0f, available.y) else Offset.Zero

            override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity = if (available.y < 0f) Velocity(0f, available.y) else Velocity.Zero
        }
    }

    LaunchedEffect(Unit) { scrimVisible = true }

    fun requestDismiss() {
        if (dismissRequested) return
        dismissRequested = true
        scope.launch {
            state.hide()
            latestOnDismiss()
        }
    }
    LaunchedEffect(dismissRequestId) { if (dismissRequestId > 0L) requestDismiss() }
    LaunchedEffect(state.targetValue) { if (state.targetValue != SheetValue.Hidden) wasVisible = true }
    LaunchedEffect(state.currentValue) {
        if (state.currentValue == SheetValue.Hidden && wasVisible && !dismissRequested) {
            dismissRequested = true
            latestOnDismiss()
        }
    }

    val bg = MaterialTheme.colorScheme.surfaceContainerLow
    val sheetBg = bg.copy(alpha = bg.alpha * bgAlpha)
    val base = MaterialTheme.motionScheme
    val settled = state.currentValue != SheetValue.Hidden
    val motion = remember(base, settled) {
        if (settled) {
            base
        } else {
            object : MotionScheme by base {
                override fun <T> defaultSpatialSpec(): FiniteAnimationSpec<T> = tween(300, easing = FastOutSlowInEasing)
            }
        }
    }
    val body: @Composable () -> Unit = {
        Column(
            Modifier
                .fillMaxWidth()
                .then(if (animateHeight) Modifier.animateContentSize(MaterialTheme.motionScheme.defaultSpatialSpec()) else Modifier)
                .then(if (fillMaxHeight) Modifier.height(cap) else Modifier.heightIn(max = cap))
                .then(
                    if (UiDebug.enabled) {
                        Modifier.onGloballyPositioned { c ->
                            if (c.size.height == loggedHeight[0]) return@onGloballyPositioned
                            loggedHeight[0] = c.size.height
                            UiDebug.log(ctx, "sheet-layout", "title" to title, "heightDp" to with(density) { c.size.height.toDp().value }, "topPx" to c.positionInWindow().y.toDouble(), "windowPx" to c.findRootCoordinates().size.height)
                        }
                    } else {
                        Modifier
                    }
                )
                .then(if (edgeToEdgeBottom) Modifier else Modifier.navigationBarsPadding())
                .then(if (imePadding) Modifier.imePadding() else Modifier)
        ) {
            Column(
                Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Box(Modifier.width(32.dp).height(4.dp).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), CircleShape))
                if (title.isNotEmpty()) {
                    Spacer(Modifier.height(12.dp))
                    Text(
                        title,
                        color = MaterialTheme.colorScheme.onSurface,
                        style = MaterialTheme.typography.titleMediumEmphasized,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth()
                    )
                }
            }
            Divider(topAlpha)
            Box(
                Modifier
                    .fillMaxWidth()
                    .weight(1f, fill = fillMaxHeight)
                    .clipToBounds()
                    .nestedScroll(bounds)
            ) {
                CompositionLocalProvider(LocalOverscrollFactory provides null) { content() }
                overlay()
            }
            Divider(bottomAlpha)
            Box(
                Modifier.fillMaxWidth().background(sheetBg).pointerInput(Unit) {
                    detectVerticalDragGestures { change, _ -> change.consume() }
                }
            ) { footer() }
        }
    }
    if (LocalInspectionMode.current) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
            Surface(shape = MaterialTheme.shapes.extraLarge.copy(bottomStart = CornerSize(0), bottomEnd = CornerSize(0)), color = bg) { body() }
        }
        return
    }
    MaterialTheme(motionScheme = motion) {
        ModalBottomSheet(
            onDismissRequest = { requestDismiss() },
            sheetState = state,
            modifier = modifier,
            dragHandle = null,
            containerColor = sheetBg,
            contentColor = MaterialTheme.colorScheme.onSurface,
            tonalElevation = 0.dp,
            scrimColor = scrimColor,
            shape = MaterialTheme.shapes.extraLarge.copy(bottomStart = CornerSize(0), bottomEnd = CornerSize(0)),
            contentWindowInsets = { WindowInsets(0, 0, 0, 0) }
        ) {
            val sheetView = LocalView.current
            val window = (sheetView.parent as? DialogWindowProvider)?.window
            SideEffect {
                if (Build.VERSION.SDK_INT >= 30) {
                    sheetView.importantForContentCapture = android.view.View.IMPORTANT_FOR_CONTENT_CAPTURE_NO_EXCLUDE_DESCENDANTS
                }
                window?.let {
                    WindowCompat.setDecorFitsSystemWindows(it, false)
                    if (Build.VERSION.SDK_INT >= 29) it.isNavigationBarContrastEnforced = false
                }
            }
            LaunchedEffect(dismissRequested, window) {
                val w = window ?: return@LaunchedEffect
                if (dismissRequested) {
                    w.addFlags(WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE)
                } else {
                    w.clearFlags(WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE)
                }
            }
            SystemBarsScrim()

            body()
        }
    }
}

@Composable
internal fun Modifier.hideKeyboardOnScroll(typing: Boolean): Modifier {
    val target = remember { FocusRequester() }
    val now by rememberUpdatedState(typing)
    val connection = remember(target) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (now && source == NestedScrollSource.UserInput && available.y != 0f) target.requestFocus()
                return Offset.Zero
            }
        }
    }
    return focusRequester(target).focusTarget().nestedScroll(connection)
}

@Composable
private fun Divider(alpha: Float) {
    Box(Modifier.fillMaxWidth().height(1.dp).graphicsLayer { this.alpha = alpha }, contentAlignment = Alignment.Center) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(1.dp)
                .graphicsLayer {
                    scaleX = 0.94f + 0.06f * alpha
                    scaleY = 20f - 19f * alpha
                }
                .background(MaterialTheme.colorScheme.surfaceContainerHighest, RoundedCornerShape(20.dp * (1f - alpha)))
        )
    }
}

@Composable
private fun SystemBarsScrim() {
    val view = LocalView.current
    val dialog = remember(view) { view.dialogWindow() }
    SideEffect {
        dialog?.let { w ->
            w.addFlags(WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS)
            w.clearFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
            w.attributes = w.attributes.apply { dimAmount = 0f }
            WindowCompat.setDecorFitsSystemWindows(w, false)
            if (Build.VERSION.SDK_INT >= 29) {
                w.isStatusBarContrastEnforced = false
                w.isNavigationBarContrastEnforced = false
            }
            w.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT)
        }
    }
}

private fun View.dialogWindow(): Window? {
    var p = parent
    while (p != null) {
        (p as? DialogWindowProvider)?.window?.let { return it }
        p = (p as? View)?.parent
    }
    return null
}

private val SHEET_TOP_GAP = 56.dp
