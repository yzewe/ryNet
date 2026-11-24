@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.yzewe.rynet.ui

import android.content.Intent
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuGroup
import androidx.compose.material3.DropdownMenuPopup
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.SelectableDropdownMenuItem
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderState
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import androidx.graphics.shapes.CornerRounding
import androidx.graphics.shapes.RoundedPolygon
import dev.yzewe.rynet.core.Native
import dev.yzewe.rynet.data.Hwid
import dev.yzewe.rynet.data.PingMode
import dev.yzewe.rynet.data.RoutingMode
import dev.yzewe.rynet.data.ServerGrid
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.data.ServerView
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.SubClient
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

enum class SettingsPage(val icon: ImageVector, val shape: RoundedPolygon) {
    Network(Glyph.Route, MaterialShapes.Sunny),
    Servers(Glyph.Grid, MaterialShapes.Cookie9Sided),
    Ping(Glyph.Bolt, MaterialShapes.Cookie6Sided),
    Auto(Glyph.Auto, MaterialShapes.Cookie9Sided),
    Subscription(Glyph.Link, MaterialShapes.Clover4Leaf),
    App(Glyph.Palette, RoundedHexagon);

    val title: String
        get() = when (this) {
            Network -> tr("Сеть", "Network")
            Servers -> tr("Серверы", "Servers")
            Ping -> tr("Пинг", "Ping")
            Auto -> tr("Автовыбор", "Auto select")
            Subscription -> tr("Подписка", "Subscription")
            App -> tr("Приложение", "App")
        }

    val summary: String
        get() = when (this) {
            Network -> tr("Маршрутизация, DNS, обход блокировок", "Routing, DNS, anti-censorship")
            Servers -> tr("Вид списка и сортировка", "List view and sorting")
            Ping -> tr("Способ проверки, слежение за сервером", "Test method, server monitoring")
            Auto -> tr("Лучший сервер, белые списки", "Best server, whitelists")
            Subscription -> tr("Обновление, маскировка", "Updates, disguise")
            App -> tr("Оформление, работа в фоне, резервная копия", "Look, background, backup")
        }
}

private val RoundedHexagon: RoundedPolygon = RoundedPolygon(6, rounding = CornerRounding(0.32f)).normalized()

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SettingsScreen(
    version: String,
    onPage: (SettingsPage) -> Unit,
    onDiag: () -> Unit,
    onBack: () -> Unit
) {
    PageScaffold(tr("Настройки", "Settings"), onBack, bottom = 32.dp) {
        item {
            Column(Modifier.enter(0)) {
                Group(SettingsPage.entries.map { page -> Item(page.icon, page.shape, page.title, page.summary) { onPage(page) } }, circles = true)
            }
        }
        item {
            Column(Modifier.enter(1)) { Footer(version, onDiag) }
        }
    }
}

@Composable
fun SettingsTab(version: String, onPage: (SettingsPage) -> Unit, onDiag: () -> Unit, bottom: Dp, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).heightIn(min = maxHeight)) {
            Column(Modifier.statusBarsPadding().padding(horizontal = 16.dp)) {
                Text(
                    tr("Настройки", "Settings"),
                    style = MaterialTheme.typography.displaySmall,
                    maxLines = 1,
                    modifier = Modifier.padding(start = 8.dp, top = 24.dp, bottom = 20.dp)
                )
                Column(Modifier.enter(0)) {
                    Group(SettingsPage.entries.map { page -> Item(page.icon, page.shape, page.title, page.summary) { onPage(page) } }, circles = true)
                }
            }
            Spacer(Modifier.height(40.dp))
            Spacer(Modifier.weight(1f))
            Box(Modifier.enter(1)) { FooterBand(version, onDiag, bottom) }
        }
    }
}

@Composable
private fun FooterBand(version: String, onDiag: () -> Unit, bottom: Dp) {
    val c = MaterialTheme.colorScheme
    val haptics = LocalHapticFeedback.current
    val wave = 6.dp
    Column(
        Modifier
            .fillMaxWidth()
            .drawBehind {
                val a = wave.toPx()
                val n = (size.width / 80.dp.toPx()).roundToInt().coerceAtLeast(3)
                val len = size.width / n
                val path = Path().apply {
                    moveTo(0f, size.height)
                    lineTo(0f, a)
                    var x = 0f
                    while (x <= size.width) {
                        lineTo(x, a - a * cos(x / len * 6.2831855f))
                        x += 2f
                    }
                    lineTo(size.width, a - a * cos(size.width / len * 6.2831855f))
                    lineTo(size.width, size.height)
                    close()
                }
                drawPath(path, c.surfaceContainerLow)
            }
            .focusProperties { canFocus = false }
            .combinedClickable(
                interactionSource = null,
                indication = null,
                onClick = {},
                onLongClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    onDiag()
                }
            )
            .padding(top = wave * 2 + 24.dp, bottom = bottom + 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text("ryNet", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold, color = c.primary, maxLines = 1)
        Text(
            tr("Версия $version", "Version $version"),
            style = MaterialTheme.typography.bodyMedium,
            color = c.onSurfaceVariant,
            modifier = Modifier.padding(top = 4.dp)
        )
    }
}

@Composable
internal fun StylePicker(current: OrbStyle, onPick: (OrbStyle) -> Unit, onDismiss: () -> Unit) {
    var close by remember { mutableLongStateOf(0L) }
    AppSheet(title = tr("Анимация", "Animation"), onDismiss = onDismiss, dismissRequestId = close) {
        LazyVerticalGrid(
            columns = GridCells.Fixed(2),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(OrbStyle.entries, key = { it.key }) { st ->
                val src = remember { MutableInteractionSource() }
                val selected = st == current
                val bgc by animateColorAsState(if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh, label = "style")
                Column(
                    Modifier
                        .pressSquish(src, 0.95f)
                        .clip(MaterialTheme.shapes.largeIncreased)
                        .background(bgc)
                        .clickable(src, PressGlow) {
                            onPick(st)
                            close++
                        }
                        .padding(vertical = 14.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    LoadingLogo(MaterialShapes.Cookie6Sided, Modifier.size(96.dp), style = st)
                    Spacer(Modifier.height(8.dp))
                    Text(st.title, color = if (selected) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface, style = MaterialTheme.typography.titleSmall)
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SettingsPageScreen(
    page: SettingsPage,
    settings: Settings,
    defaultHwid: String,
    onChange: (Settings) -> Unit,
    onOpen: (Screen) -> Unit,
    onBack: () -> Unit
) {
    var hwidDialog by remember { mutableStateOf(false) }
    var sendDialog by remember { mutableStateOf(false) }
    val ctx = LocalContext.current
    var promotionAllowed by remember { mutableStateOf(true) }
    if (android.os.Build.VERSION.SDK_INT >= Settings.LIVE_UPDATE_SDK) {
        androidx.lifecycle.compose.LifecycleResumeEffect(Unit) {
            promotionAllowed = runCatching { ctx.getSystemService(android.app.NotificationManager::class.java).canPostPromotedNotifications() }.getOrDefault(true)
            onPauseOrDispose {}
        }
    }
    PageScaffold(page.title, onBack = onBack) {
        item {
            Column(Modifier.enter(0)) {
                when (page) {
                    SettingsPage.Servers -> ServersGroup(settings, onChange)

                    SettingsPage.Network -> {
                        RoutingGroup(settings, onChange, onOpen)
                        NetworkGroup(settings, onChange)
                    }

                    SettingsPage.Ping -> {
                        PingGroup(settings, onChange)
                        HealthGroup(settings, onChange)
                    }

                    SettingsPage.Auto -> AutoGroup(settings, onChange)

                    SettingsPage.Subscription -> {
                        Header(tr("Обновление", "Updates"))
                        Group(
                            listOf(
                                Item(
                                    Icons.Rounded.Refresh,
                                    MaterialShapes.Cookie9Sided,
                                    tr("Автообновление", "Auto-update"),
                                    tr("Подписка обновляется сама в фоне", "The subscription refreshes itself in the background"),
                                    settings.autoUpdate
                                ) { onChange(settings.copy(autoUpdate = !settings.autoUpdate)) },
                                Item(
                                    Icons.Rounded.Notifications,
                                    MaterialShapes.Cookie6Sided,
                                    tr("Сообщения об обновлении", "Update notices"),
                                    tr("Сколько серверов пришло после обновления", "How many servers came after an update"),
                                    settings.refreshNotices
                                ) { onChange(settings.copy(refreshNotices = !settings.refreshNotices)) }
                            )
                        )
                        SubClientBlock(settings.subClient, settings.subHeaders) { client, headers -> onChange(settings.copy(subClient = client, subHeaders = headers)) }
                        Group(
                            items = listOf(
                                Item(
                                    Icons.AutoMirrored.Rounded.Send,
                                    MaterialShapes.Cookie7Sided,
                                    tr("Отправлять HWID", "Send HWID"),
                                    tr("Провайдер подписки увидит устройство и учтёт в лимите", "The subscription provider sees the device and counts it in the limit"),
                                    settings.sendHwid
                                ) { onChange(settings.copy(sendHwid = !settings.sendHwid)) },
                                Item(
                                    Glyph.Smartphone,
                                    MaterialShapes.Square,
                                    "HWID",
                                    settings.hwid.ifBlank { tr("$defaultHwid · по\u00A0умолчанию", "$defaultHwid · default") }
                                ) { hwidDialog = true }
                            )
                        )
                    }

                    SettingsPage.App -> {
                        Group(
                            listOf(
                                Item(Glyph.Palette, MaterialShapes.Flower, tr("Оформление", "Appearance"), tr("Тема, цвета, иконка и анимация", "Theme, colors, icon and animation")) {
                                    ctx.startActivity(SettingsActivity.appearance(ctx))
                                }
                            )
                        )
                        Header(tr("Работа", "Behavior"))
                        Group(
                            top = {
                                DropdownChecks(
                                    Glyph.TimerOff,
                                    tr("Отключать VPN", "Turn off the VPN"),
                                    listOf(VPN_OFF_WIFI, VPN_OFF_LOCK),
                                    setOfNotNull(VPN_OFF_WIFI.takeIf { settings.wifiPause }, VPN_OFF_LOCK.takeIf { settings.lockOff }),
                                    { if (it == VPN_OFF_WIFI) "Wi-Fi" else tr("Экран заблокирован 30 минут", "Screen locked for 30 minutes") },
                                    none = tr("Никогда", "Never")
                                ) { o, on -> onChange(if (o == VPN_OFF_WIFI) settings.copy(wifiPause = on) else settings.copy(lockOff = on)) }
                            },
                            items = listOfNotNull(
                                Item(
                                    Glyph.Power,
                                    MaterialShapes.Gem,
                                    tr("Запуск при включении телефона", "Start on boot"),
                                    tr("Подключаться сразу после перезагрузки", "Connect right after a reboot"),
                                    settings.autostart
                                ) { onChange(settings.copy(autostart = !settings.autostart)) },
                                Item(
                                    Glyph.Battery,
                                    MaterialShapes.Cookie6Sided,
                                    tr("Улучшенная работа в\u00A0фоне", "Improved background mode"),
                                    tr("Не засыпает с выключенным экраном, ест больше батареи", "Stays awake with the screen off, uses more battery"),
                                    settings.background
                                ) { onChange(settings.copy(background = !settings.background)) },
                                Item(
                                    Icons.Rounded.Notifications,
                                    MaterialShapes.Cookie7Sided,
                                    tr("Подробное уведомление", "Detailed notification"),
                                    tr("Скорость и трафик прямо в уведомлении", "Speed and traffic right in the notification"),
                                    settings.notifyDetails
                                ) { onChange(settings.copy(notifyDetails = !settings.notifyDetails)) },
                                Item(
                                    Glyph.Timer,
                                    MaterialShapes.Cookie9Sided,
                                    tr("Live Update", "Live Update"),
                                    tr("Уведомление с таймером подключения в строке состояния", "A notification with the connection timer in the status bar"),
                                    settings.liveUpdate
                                ) { onChange(settings.copy(liveUpdate = !settings.liveUpdate)) }.takeIf { android.os.Build.VERSION.SDK_INT >= Settings.LIVE_UPDATE_SDK },
                                Item(
                                    Glyph.TimerOff,
                                    MaterialShapes.Cookie9Sided,
                                    tr("Live Update выключен в системе", "Live Update is off in system settings"),
                                    tr("Нажмите, чтобы разрешить его для ryNet", "Tap to allow it for ryNet"),
                                    action = true
                                ) {
                                    runCatching {
                                        ctx.startActivity(
                                            Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS).putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, ctx.packageName)
                                        )
                                    }
                                }.takeIf { android.os.Build.VERSION.SDK_INT >= Settings.LIVE_UPDATE_SDK && settings.liveUpdate && !promotionAllowed },
                                Item(
                                    Glyph.Link,
                                    MaterialShapes.Burst,
                                    tr("Внешнее управление", "External control"),
                                    tr("Tasker и MacroDroid смогут включать и выключать VPN", "Tasker and MacroDroid can turn the VPN on and off"),
                                    settings.remoteControl
                                ) { onChange(settings.copy(remoteControl = !settings.remoteControl)) },
                                Item(
                                    Glyph.Upload,
                                    MaterialShapes.Gem,
                                    tr("Отправлять логи разработчику", "Send logs to the developer"),
                                    tr("Отчёты о сбоях помогут быстрее их исправить", "Crash reports help get them fixed sooner"),
                                    settings.sendLogs
                                ) { if (settings.sendLogs) onChange(settings.sendingLogs(false)) else sendDialog = true }
                            )
                        )
                        BackupSection(onImported = onChange)
                        UpdateSection()
                    }
                }
            }
        }
    }
    if (sendDialog) SendLogsDialog(onConfirm = { onChange(settings.sendingLogs(true)) }, onDismiss = { sendDialog = false })
    if (hwidDialog) {
        HwidDialog(settings.hwid, defaultHwid, onDismiss = { hwidDialog = false }) {
            onChange(settings.copy(hwid = it))
            hwidDialog = false
        }
    }
}

private const val VPN_OFF_WIFI = 1

private const val VPN_OFF_LOCK = 2

@Composable
internal fun RoutingGroup(settings: Settings, onChange: (Settings) -> Unit, onOpen: (Screen) -> Unit) {
    val count = settings.rules.size + settings.sets.size + settings.userRules.lines().count { it.isNotBlank() && !it.trim().startsWith("#") }
    ChoiceBlock(
        tr("Маршрутизация", "Routing"),
        RoutingMode.selectable,
        settings.routing,
        { it.title },
        { it.summary }
    ) { onChange(settings.copy(routing = it)) }
    Group(
        listOf(
            Item(
                Icons.Rounded.Edit,
                MaterialShapes.Pill,
                tr("Правила", "Rules"),
                if (count == 0) tr("Какие сайты и приложения идут мимо VPN", "Which sites and apps skip the VPN") else plural(count, "правило", "правила", "правил", "rule")
            ) { onOpen(Screen.Rules) }
        )
    )
}

@Composable
private fun ServersGroup(settings: Settings, onChange: (Settings) -> Unit) {
    ChoiceBlock(tr("Отображение", "View"), ServerView.entries, settings.view, { it.title }) { onChange(settings.copy(view = it)) }
    Group(
        items = listOf(
            Item(
                Glyph.Flag,
                MaterialShapes.Pill,
                tr("Страна по IP выхода", "Country by exit IP"),
                tr("По стороннему сервису, бывает точнее названия", "From a third-party service, often more accurate than the name"),
                settings.exitFlags
            ) { onChange(settings.copy(exitFlags = !settings.exitFlags)) }
        ),
        top = {
            IconToggleRow(
                Glyph.Grid,
                tr("Сетка", "Grid"),
                listOf(ServerGrid.One, ServerGrid.Two),
                settings.grid,
                { if (it == ServerGrid.Two) tr("Две плитки в ряд", "Two tiles per row") else tr("Одна плитка в ряд", "One tile per row") },
                { if (it == ServerGrid.Two) Glyph.TilesTwo else Glyph.TilesOne }
            ) { onChange(settings.copy(grid = it)) }
        }
    )
    ChoiceBlock(tr("Сортировка", "Sorting"), ServerSort.entries, settings.sort, { it.title }) { onChange(settings.copy(sort = it)) }
    ChoiceBlock(tr("Подпись", "Caption"), ServerInfo.entries, settings.serverInfo, { it.title }) { onChange(settings.copy(serverInfo = it)) }
}

@Composable
private fun SubClientBlock(client: SubClient, headers: String, onChange: (SubClient, String) -> Unit) {
    var text by rememberSaveable { mutableStateOf(headers) }
    val latest by rememberUpdatedState(onChange)
    LaunchedEffect(text) {
        if (text == headers) return@LaunchedEffect
        delay(400)
        latest(SubClient.Custom, text.trim())
    }
    ChoiceBlock(
        tr("Клиент подписки", "Subscription client"),
        SubClient.entries,
        client,
        { it.title },
        extra = { m ->
            Reveal(m == SubClient.Custom) {
                TextField(
                    value = text,
                    onValueChange = { text = it },
                    placeholder = { Text("User-Agent: Happ/4.3.0\nX-Client: ryNet", fontFamily = FontFamily.Monospace) },
                    minLines = 2,
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    shape = RoundedCornerShape(Segments.Outer),
                    colors = TextFieldDefaults.colors(
                        focusedIndicatorColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent,
                        focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                        unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh
                    ),
                    modifier = Modifier.fillMaxWidth().padding(top = 12.dp).remoteField(RoundedCornerShape(Segments.Outer))
                )
            }
        }
    ) { latest(it, text.trim()) }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Footer(version: String, onDiag: () -> Unit) {
    val c = MaterialTheme.colorScheme
    val haptics = LocalHapticFeedback.current
    Column(
        Modifier
            .fillMaxWidth()
            .padding(top = 40.dp, bottom = 8.dp)
            .focusProperties { canFocus = false }
            .combinedClickable(
                interactionSource = null,
                indication = null,
                onClick = {},
                onLongClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    onDiag()
                }
            ),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Canvas(Modifier.fillMaxWidth(0.5f).height(12.dp)) {
            val a = size.height * 0.3f
            val len = size.width / 6f
            val mid = size.height / 2
            val path = Path().apply {
                moveTo(0f, mid)
                var x = 0f
                while (x <= size.width) {
                    lineTo(x, mid + a * sin(x / len * 6.2831855f))
                    x += 1.5f
                }
            }
            drawPath(path, c.outline, style = Stroke(1.5.dp.toPx(), cap = StrokeCap.Round))
        }
        Spacer(Modifier.height(10.dp))
        Text(
            "ryNet $version",
            style = MaterialTheme.typography.labelMedium,
            color = c.onSurfaceVariant
        )
    }
}
