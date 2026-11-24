package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.data.Endpoints
import dev.yzewe.rynet.data.Fragment
import dev.yzewe.rynet.data.ServerDns
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Sub
import dev.yzewe.rynet.tr
import kotlinx.coroutines.delay

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun NetworkGroup(settings: Settings, onChange: (Settings) -> Unit) {
    Header("DNS")
    Group(
        top = {
            DnsCard(settings.dnsServers) { onChange(settings.copy(dnsServers = it)) }
        },
        more = listOf {
            ServerDnsCard(settings.serverDns, settings.serverDnsCustom) { mode, list -> onChange(settings.copy(serverDns = mode, serverDnsCustom = list)) }
        },
        items = listOf(
            Item(
                Glyph.Bolt,
                MaterialShapes.Cookie4Sided,
                "FakeDNS",
                tr("Сайты открываются быстрее, без ожидания DNS", "Sites open faster, without waiting for DNS"),
                settings.fakeDns
            ) { onChange(settings.copy(fakeDns = !settings.fakeDns)) }
        )
    )
    Header(tr("Обход блокировок", "Anti-censorship"))
    Group(
        listOf(
            Item(
                Glyph.Link,
                MaterialShapes.Cookie9Sided,
                tr("Цепочка", "Chain"),
                tr("Трафик идёт через два сервера: вход и выход", "Traffic goes through two servers: entry and exit"),
                settings.chain
            ) { onChange(settings.copy(chain = !settings.chain)) },
            Item(
                Glyph.Cut,
                MaterialShapes.Cookie12Sided,
                tr("Фрагментация TLS", "TLS fragmentation"),
                tr("Помогает обойти блокировки у части провайдеров", "Helps get past blocking at some providers"),
                settings.fragment
            ) { onChange(settings.copy(fragment = !settings.fragment)) }
        )
    )
    Reveal(settings.fragment) { FragmentBlock(settings, onChange) }
    TrafficBlock(settings, onChange)
    Reveal(!settings.proxyMode) {
        Group(
            items = listOf(
                Item(
                    Glyph.Lock,
                    MaterialShapes.Cookie7Sided,
                    tr("Защита от утечки трафика", "Kill switch"),
                    tr("Если VPN оборвётся, интернет не пойдёт напрямую", "If the VPN drops, traffic won't go direct"),
                    settings.killSwitch
                ) { onChange(settings.copy(killSwitch = !settings.killSwitch)) }
            ),
            top = { MtuCard(settings.mtu) { onChange(settings.copy(mtu = it)) } }
        )
    }
}

@Composable
private fun TrafficBlock(settings: Settings, onChange: (Settings) -> Unit) {
    var port by rememberSaveable { mutableStateOf(settings.proxyPort.toString()) }
    val okPort = port.toIntOrNull()?.let { it in 1024..65535 } == true
    val latest by rememberUpdatedState(settings)
    val save by rememberUpdatedState(onChange)
    LaunchedEffect(port) {
        if (!okPort || port.toInt() == latest.proxyPort) return@LaunchedEffect
        delay(600)
        save(latest.copy(proxyPort = port.toInt()))
    }
    ChoiceBlock(
        tr("Трафик", "Traffic"),
        listOf(false, true),
        settings.proxyMode,
        { if (it) tr("Прокси", "Proxy") else tr("Весь трафик", "All traffic") },
        { if (it) tr("SOCKS5 и HTTP на 127.0.0.1:${settings.proxyPort}", "SOCKS5 and HTTP on 127.0.0.1:${settings.proxyPort}") else tr("Через VPN идут все приложения", "All apps go through the VPN") },
        extra = { proxy ->
            Reveal(proxy) {
                RangeField(port, tr("Порт", "Port"), okPort, Modifier.padding(top = 12.dp).fillMaxWidth()) { v -> port = v.filter(Char::isDigit).take(5) }
            }
        }
    ) { onChange(settings.copy(proxyMode = it)) }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun FragmentBlock(settings: Settings, onChange: (Settings) -> Unit) {
    var custom by rememberSaveable { mutableStateOf(Fragment.of(settings.fragmentLength, settings.fragmentInterval) == null) }
    var length by rememberSaveable { mutableStateOf(settings.fragmentLength) }
    var interval by rememberSaveable { mutableStateOf(settings.fragmentInterval) }
    val okLength = Fragment.validRange(length, 16384)
    val okInterval = Fragment.validRange(interval, 1000)
    val latest by rememberUpdatedState(settings)
    val save by rememberUpdatedState(onChange)
    LaunchedEffect(length, interval) {
        if (!custom || !okLength || !okInterval) return@LaunchedEffect
        if (length.trim() == latest.fragmentLength && interval.trim() == latest.fragmentInterval) return@LaunchedEffect
        delay(400)
        save(latest.copy(fragmentLength = length.trim(), fragmentInterval = interval.trim()))
    }
    val selected: Fragment? = if (custom) null else Fragment.of(settings.fragmentLength, settings.fragmentInterval)
    ChoiceBlock(
        tr("Сила фрагментации", "Fragmentation strength"),
        Fragment.entries + listOf(null),
        selected,
        { it?.title ?: tr("Своё", "Custom") },
        { it?.let { f -> tr("${f.summary} · ${f.length} байт, ${f.interval} мс", "${f.summary} · ${f.length} bytes, ${f.interval} ms") } ?: tr("Длина кусков и пауза между ними — «от-до»", "Chunk size and pause between chunks, as min-max") },
        extra = { f ->
            Reveal(f == null) {
                Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    RangeField(length, tr("Куски, байт", "Chunks, bytes"), okLength, Modifier.weight(1f)) { length = it }
                    RangeField(interval, tr("Пауза, мс", "Pause, ms"), okInterval, Modifier.weight(1f)) { interval = it }
                }
            }
        }
    ) { f ->
        custom = f == null
        if (f != null) {
            length = f.length
            interval = f.interval
            onChange(settings.copy(fragmentLength = f.length, fragmentInterval = f.interval))
        }
    }
}

@Composable
private fun RangeField(value: String, label: String, valid: Boolean, modifier: Modifier, onValue: (String) -> Unit) {
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
        isError = !valid,
        textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
        shape = RoundedCornerShape(Segments.Outer),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        colors = fieldColors(MaterialTheme.colorScheme.surfaceContainerHigh),
        modifier = modifier.remoteField(RoundedCornerShape(Segments.Outer))
    )
}

@Composable
private fun fieldColors(container: Color = MaterialTheme.colorScheme.surfaceContainerHighest) = TextFieldDefaults.colors(
    focusedIndicatorColor = Color.Transparent,
    unfocusedIndicatorColor = Color.Transparent,
    errorIndicatorColor = Color.Transparent,
    focusedContainerColor = container,
    unfocusedContainerColor = container
)

private const val MTU_CUSTOM = -1
private val MTUS = listOf(0, 1500, 1400, 1280, MTU_CUSTOM)

internal val DNS_PRESETS = Endpoints.DNS
private const val DNS_SUB = "sub"
private const val DNS_CUSTOM = "custom"

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun DnsCard(value: String, onChange: (String) -> Unit) {
    val preset = DNS_PRESETS.firstOrNull { it.second == value.trim() }?.first
    var custom by rememberSaveable { mutableStateOf(value.isNotBlank() && preset == null) }
    var text by rememberSaveable { mutableStateOf(if (preset == null) value else "") }
    val valid = text.isNotBlank() && text.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }.all { DNS_ADDRESS.matches(it) }
    val save by rememberUpdatedState(onChange)
    LaunchedEffect(text) {
        if (!custom || !valid || text.trim() == value.trim()) return@LaunchedEffect
        delay(600)
        save(text.trim())
    }
    val selected = when {
        custom -> DNS_CUSTOM
        preset != null -> preset
        else -> DNS_SUB
    }
    Column {
        DropdownRow(
            Glyph.Globe,
            "DNS",
            listOf(DNS_SUB) + DNS_PRESETS.map { it.first } + DNS_CUSTOM,
            selected,
            {
                when (it) {
                    DNS_SUB -> tr("Авто", "Auto")
                    DNS_CUSTOM -> tr("Своё", "Custom")
                    else -> it
                }
            }
        ) { d ->
            custom = d == DNS_CUSTOM
            when (d) {
                DNS_SUB -> onChange("")
                DNS_CUSTOM -> if (valid) onChange(text.trim())
                else -> onChange(DNS_PRESETS.first { it.first == d }.second)
            }
        }
        Fold(custom) {
            GroupCard {
                TextField(
                    value = text,
                    onValueChange = { text = it },
                    placeholder = { Text("https://dns.example/dns-query") },
                    isError = text.isNotBlank() && !valid,
                    maxLines = 4,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    colors = fieldColors(),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp).remoteField(MaterialTheme.shapes.large)
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ServerDnsCard(mode: ServerDns, custom: String, onChange: (ServerDns, String) -> Unit) {
    var text by rememberSaveable { mutableStateOf(custom) }
    val valid = text.isNotBlank() && text.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }.all { SERVER_DNS_ADDRESS.matches(it) }
    val latest by rememberUpdatedState(custom)
    val save by rememberUpdatedState(onChange)
    LaunchedEffect(text) {
        if (mode != ServerDns.Custom || !valid || text.trim() == latest.trim()) return@LaunchedEffect
        delay(600)
        save(ServerDns.Custom, text.trim())
    }
    Column {
        DropdownRow(
            Glyph.Link,
            tr("DNS серверов", "Server DNS"),
            ServerDns.entries,
            mode,
            { it.title }
        ) { save(it, if (it == ServerDns.Custom && valid) text.trim() else custom) }
        Fold(mode == ServerDns.Custom) {
            GroupCard {
                TextField(
                    value = text,
                    onValueChange = { text = it },
                    placeholder = { Text("https://dns.example/dns-query") },
                    isError = text.isNotBlank() && !valid,
                    maxLines = 4,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    colors = fieldColors(),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp).remoteField(MaterialTheme.shapes.large)
                )
            }
        }
    }
}

private val SERVER_DNS_ADDRESS = Regex("""(?i)^([0-9a-f.:\[\]]+|(udp|tls|https)://[^\s/]+(/\S*)?)$""")

internal val DNS_ADDRESS = Regex("""(?i)^(localhost|fakedns|[0-9a-f.:\[\]]+|(udp|tcp|tls|https|quic)(\+local)?://[^\s/]+(/\S*)?)$""")

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun MtuCard(mtu: Int, onChange: (Int) -> Unit) {
    var custom by rememberSaveable { mutableStateOf(mtu !in MTUS) }
    var text by rememberSaveable { mutableStateOf(if (mtu > 0) mtu.toString() else "") }
    val value = text.trim().toIntOrNull()
    val valid = value != null && value in Settings.MTU_RANGE
    val save by rememberUpdatedState(onChange)
    LaunchedEffect(text) {
        if (!custom || !valid || value == mtu) return@LaunchedEffect
        delay(400)
        save(value!!)
    }
    Column {
        DropdownRow(
            Glyph.Ruler,
            "MTU",
            MTUS,
            if (custom) MTU_CUSTOM else mtu,
            {
                when (it) {
                    0 -> tr("Авто", "Auto")
                    MTU_CUSTOM -> tr("Своё", "Custom")
                    else -> "$it"
                }
            },
            {
                when (it) {
                    0 -> tr("Быстрее и экономнее", "Faster and more efficient")
                    1500 -> tr("Как у обычной сети", "Same as a regular network")
                    1400 -> tr("Если рвутся звонки или игры", "If calls or games drop")
                    1280 -> tr("Самый маленький: если что-то не открывается", "Smallest: if something won't load")
                    else -> tr("От ${Settings.MTU_RANGE.first} до ${Settings.MTU_RANGE.last}", "From ${Settings.MTU_RANGE.first} to ${Settings.MTU_RANGE.last}")
                }
            }
        ) { m ->
            custom = m == MTU_CUSTOM
            if (m != MTU_CUSTOM) onChange(m)
        }
        Fold(custom) {
            GroupCard {
                TextField(
                    value = text,
                    onValueChange = { v -> text = v.filter { it.isDigit() }.take(4) },
                    placeholder = { Text("1420") },
                    singleLine = true,
                    isError = text.isNotEmpty() && !valid,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    shape = MaterialTheme.shapes.large,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    colors = fieldColors(),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp).remoteField(MaterialTheme.shapes.large)
                )
            }
        }
    }
}

@Composable
internal fun SubChips(subs: List<Sub>, current: String?, onPick: (String) -> Unit) {
    if (subs.size < 2) return
    LazyRow(
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier.padding(bottom = 8.dp)
    ) {
        items(subs, key = { it.id }) { s ->
            SmallChip(s.title, s.id == current, { onPick(s.id) })
        }
    }
}
