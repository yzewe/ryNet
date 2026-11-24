@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import dev.yzewe.rynet.tr

@Composable
fun PageScaffold(
    title: String,
    onBack: () -> Unit,
    floatingActionButton: @Composable () -> Unit = {},
    bottom: Dp = 64.dp,
    actions: @Composable RowScope.() -> Unit = {},
    list: LazyListState = rememberLazyListState(),
    overlay: @Composable BoxScope.() -> Unit = {},
    content: LazyListScope.() -> Unit
) {
    val contentFocus = remember { FocusRequester() }
    val bg = MaterialTheme.colorScheme.surface
    var titleHeight by remember { mutableIntStateOf(0) }
    val collapsed by remember {
        derivedStateOf {
            when {
                list.firstVisibleItemIndex > 0 -> 1f
                titleHeight <= 0 -> 0f
                else -> (list.firstVisibleItemScrollOffset / titleHeight.toFloat()).coerceIn(0f, 1f)
            }
        }
    }
    TitleSnap(list) { titleHeight }
    Box(Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = bg,
            topBar = {
                PageTopBar(
                    title,
                    false,
                    onBack,
                    actions,
                    backModifier = Modifier.focusProperties { down = contentFocus },
                    titleAlpha = { ((collapsed - 0.35f) / 0.4f).coerceIn(0f, 1f) }
                )
            },
            floatingActionButton = floatingActionButton
        ) { pad ->
            LazyColumn(
                state = list,
                modifier = Modifier.imePadding().focusRequester(contentFocus).focusGroup(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = pad.calculateTopPadding(), bottom = pad.calculateBottomPadding() + bottom)
            ) {
                item(key = "page-title") {
                    Text(
                        title,
                        color = MaterialTheme.colorScheme.onSurface,
                        style = PageType.title,
                        modifier = Modifier
                            .onSizeChanged { titleHeight = it.height }
                            .padding(top = 34.dp, bottom = 11.dp)
                            .graphicsLayer { alpha = 1f - ((collapsed - 0.35f) / 0.4f).coerceIn(0f, 1f) }
                    )
                }
                content()
            }
        }
        overlay()
    }
}

@Composable
private fun TitleSnap(list: LazyListState, height: () -> Int) {
    LaunchedEffect(list) {
        snapshotFlow { Triple(list.isScrollInProgress, list.firstVisibleItemIndex, list.firstVisibleItemScrollOffset) }
            .collect { (scrolling, index, offset) ->
                val h = height()
                if (scrolling || index != 0 || h <= 0 || offset <= 0 || offset >= h) return@collect
                val target = if (offset >= h / 2f) h else 0
                runCatching { list.animateScrollBy((target - offset).toFloat(), tween(TITLE_SNAP_MS, easing = FastOutSlowInEasing)) }
            }
    }
}

private const val TITLE_SNAP_MS = 140

@Composable
fun PageTopBar(
    title: String,
    showTitle: Boolean,
    onBack: () -> Unit,
    actions: @Composable RowScope.() -> Unit = {},
    titleModifier: Modifier = Modifier,
    backModifier: Modifier = Modifier,
    titleAlpha: (() -> Float)? = null
) {
    val c = MaterialTheme.colorScheme
    val shown by animateFloatAsState(if (showTitle) 1f else 0f, label = "barTitle")
    Row(
        Modifier
            .fillMaxWidth()
            .background(c.surface)
            .statusBarsPadding()
            .padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        CircleBack(onBack, backModifier)
        Text(
            title,
            color = c.onSurface,
            style = PageType.bar,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 14.dp).weight(1f).then(titleModifier).graphicsLayer { alpha = titleAlpha?.invoke() ?: shown }
        )
        actions()
    }
}

@Composable
fun CircleBack(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = MaterialTheme.colorScheme
    val view = LocalView.current
    val src = remember { MutableInteractionSource() }
    Box(
        modifier
            .minimumInteractiveComponentSize()
            .size(40.dp)
            .remoteFocus(src, CircleShape)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(interactionSource = src, indication = ripple()) {
                view.haptic(Haptic.Tap)
                onBack()
            },
        contentAlignment = Alignment.Center
    ) {
        Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = tr("Назад", "Back"), tint = c.onSurface, modifier = Modifier.size(24.dp))
    }
}
