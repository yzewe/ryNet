package dev.yzewe.rynet.data

import android.content.Context
import dev.yzewe.rynet.tr
import org.json.JSONArray
import org.json.JSONObject

enum class RoutingMode(
    val key: String,
    private val ruTitle: String,
    private val enTitle: String,
    private val ruSummary: String,
    private val enSummary: String
) {
    Smart("smart", "Умный", "Smart", "Российское и локальная сеть — напрямую", "Russian and local traffic goes direct"),
    Subscription("subscription", "Подписка", "Subscription", "Правила из подписки", "Rules from the subscription"),
    Global("global", "Всё", "All", "Весь трафик через VPN", "All traffic through VPN"),
    Custom("custom", "Своё", "Custom", "Только свои правила", "Only your own rules"),
    Direct("direct", "Напрямую", "Direct", "Весь трафик идёт напрямую, кроме правил", "All traffic goes direct, except your rules")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    val summary: String get() = tr(ruSummary, enSummary)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }

        val selectable = listOf(Smart, Subscription, Global, Direct)
    }
}

enum class PingMode(
    val code: Int,
    val key: String,
    private val ruTitle: String,
    private val enTitle: String,
    private val ruSummary: String,
    private val enSummary: String
) {
    Tcp(0, "tcp", "TCP", "TCP", "Соединение с сервером", "Connection to the server"),
    Icmp(2, "icmp", "ICMP", "ICMP", "Эхо-запрос к серверу", "Echo request to the server"),
    Proxy(1, "proxy", "Прокси", "Proxy", "Запрос через сервер", "Request through the server"),
    ProxyWarm(3, "proxy2", "Прокси ×2", "Proxy ×2", "Второй запрос — без рукопожатия", "Second request — no handshake")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    val summary: String get() = tr(ruSummary, enSummary)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class ThemeMode(val key: String, private val ruTitle: String, private val enTitle: String, val summary: String) {
    System("system", "Авто", "Auto", ""),
    Light("light", "Светлая", "Light", ""),
    Dark("dark", "Тёмная", "Dark", ""),
    Amoled("amoled", "AMOLED", "AMOLED", "")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class ServerSort(val key: String, private val ruTitle: String, private val enTitle: String) {
    Default("default", "Как есть", "Default"),
    Ping("ping", "Пинг", "Ping"),
    Name("name", "А–Я", "A–Z")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class SubClient(val key: String, private val ruTitle: String, private val enTitle: String) {
    RyNet("rynet", "ryNet", "ryNet"),
    Happ("happ", "Happ", "Happ"),
    Custom("custom", "Своё", "Custom")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class ServerView(val key: String, private val ruTitle: String, private val enTitle: String) {
    Groups("groups", "Группы", "Groups"),
    Single("single", "Единый", "Single list")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class ServerInfo(val key: String, private val ruTitle: String, private val enTitle: String) {
    Description("description", "Описание", "Description"),
    Protocol("protocol", "Протокол", "Protocol"),
    None("none", "Ничего", "None")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class ServerGrid(val key: String, val title: String, val columns: Int) {
    Two("2x1", "2×1", 2),
    One("1x1", "1×1", 1)
    ;

    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class SectionDivider(val key: String, private val ruTitle: String, private val enTitle: String) {
    None("none", "Без разделителя", "None"),
    Gap("gap", "Зазор", "Gap")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class Fragment(
    private val ruTitle: String,
    private val enTitle: String,
    val length: String,
    val interval: String,
    private val ruSummary: String,
    private val enSummary: String
) {
    Light("Лёгкая", "Light", "100-200", "10-20", "Крупные куски — почти без задержки", "Large chunks — almost no delay"),
    Medium("Средняя", "Medium", "30-60", "10-30", "Если с лёгкой всё ещё режут", "If light still gets blocked"),
    Strong("Сильная", "Strong", "5-20", "20-50", "Мелкие куски — дольше соединение", "Small chunks — slower to connect")
    ;

    val title: String get() = tr(ruTitle, enTitle)
    val summary: String get() = tr(ruSummary, enSummary)
    companion object {
        fun of(length: String, interval: String) = entries.firstOrNull { it.length == length && it.interval == interval }

        fun validRange(v: String, max: Int): Boolean {
            val parts = v.trim().split('-').map { it.trim().toIntOrNull() }
            if (parts.isEmpty() || parts.size > 2 || parts.any { it == null }) return false
            val a = parts.first()!!
            val b = parts.last()!!
            return a in 1..max && b in a..max
        }
    }
}

data class Settings(
    val routing: RoutingMode = RoutingMode.Subscription,
    val pingMode: PingMode = PingMode.Proxy,
    val pingOnStart: Boolean = false,
    val sort: ServerSort = ServerSort.Default,
    val view: ServerView = ServerView.Groups,
    val grid: ServerGrid = ServerGrid.Two,
    val sectionRadius: Int = 16,
    val sectionDivider: SectionDivider = SectionDivider.Gap,
    val serverInfo: ServerInfo = ServerInfo.Description,
    val exitFlags: Boolean = false,
    val pingParallel: Int = 16,
    val pingTimeout: Int = 5,
    val pingUrl: String = "",
    val ipUrl: String = "",
    val fakeDns: Boolean = false,
    val serverDns: ServerDns = ServerDns.Auto,
    val serverDnsCustom: String = "",
    val dnsServers: String = "",
    val pins: Map<String, List<String>> = emptyMap(),
    val perServerDns: Map<String, Map<String, String>> = emptyMap(),
    val hwid: String = "",
    val autoUpdate: Boolean = true,
    val sendHwid: Boolean = true,
    val subClient: SubClient = SubClient.Happ,
    val subHeaders: String = "",
    val rules: List<Rule> = emptyList(),
    val sets: List<RuleSet> = emptyList(),
    val rest: RuleAction = RuleAction.Proxy,
    val userRules: String = "",
    val autostart: Boolean = false,
    val wifiPause: Boolean = false,
    val lockOff: Boolean = false,
    val proxyMode: Boolean = false,
    val proxyPort: Int = 10808,
    val remoteControl: Boolean = false,
    val theme: ThemeMode = ThemeMode.System,
    val wallpaperColors: Boolean = true,
    val seedColor: Int = 0,
    val debugLog: Boolean = false,
    val sendLogs: Boolean = false,
    val uiDebug: Boolean = false,
    val healthCheck: Boolean = false,
    val healthSec: Int = 300,
    val healthNotify: Boolean = true,
    val autoSwitch: Boolean = false,
    val healthIdle: Boolean = false,
    val killSwitch: Boolean = false,
    val vpnAppsOnly: Boolean = false,
    val background: Boolean = false,
    val notifyDetails: Boolean = false,
    val liveUpdate: Boolean = true,
    val refreshNotices: Boolean = false,
    val orbStyle: String = "wave",
    val mtu: Int = 0,
    val fragment: Boolean = false,
    val fragmentLength: String = Fragment.Light.length,
    val fragmentInterval: String = Fragment.Light.interval,
    val chain: Boolean = false,
    val chainSub: String = "",
    val chainProfile: String = "",
    val autoSelect: Boolean = false,
    val autoFeature: Boolean = false,
    val autoSkipSubs: List<String> = emptyList(),
    val autoSkip: Map<String, List<String>> = emptyMap(),
    val autoWlSubs: List<String> = emptyList(),
    val autoWl: Map<String, List<String>> = emptyMap(),
    val autoPins: Boolean = true,
    val autoTransports: List<String> = emptyList(),
    val autoWhitelist: WhitelistMode = WhitelistMode.Auto,
    val autoHome: String = AUTO_HOME,
    val autoFree: String = AUTO_FREE,
    val autoInterval: Int = 15,
    val autoMargin: Int = 50,
    val reserveOnlyOnWhitelists: Boolean = false,
    val autoSpeed: Boolean = true,
    val autoSaveData: Boolean = false,
    val autoNotify: Boolean = true,
    val dismissedNotices: List<String> = emptyList()
) {
    fun chainEntry(): Selection? = if (chain && chainSub.isNotEmpty() && chainProfile.isNotEmpty()) Selection(chainSub, chainProfile) else null

    fun serverDnsArg(): String = when {
        serverDns != ServerDns.Custom -> serverDns.key
        serverDnsCustom.isBlank() -> ServerDns.Auto.key
        else -> "custom:" + serverDnsCustom.trim()
    }

    fun ownDns(): List<String> = dnsServers.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }

    fun dnsFor(sel: Selection?): List<String> = sel?.let { s -> perServerDns[s.sub]?.let { m -> m[s.profile] ?: Profile.parentOf(s.profile)?.let(m::get) } }
        ?.split(Regex("[\\s,]+"))?.filter { it.isNotEmpty() }?.takeIf { it.isNotEmpty() } ?: ownDns()

    fun requestsLiveUpdate(sdk: Int): Boolean = liveUpdate && sdk >= LIVE_UPDATE_SDK

    fun noticeHidden(key: String): Boolean = key in dismissedNotices

    fun dismissNotice(key: String): Settings = if (key in dismissedNotices) this else copy(dismissedNotices = (dismissedNotices + key).takeLast(NOTICES_KEPT))

    fun pinned(sub: String?): List<String> = sub?.let { pins[it] }.orEmpty()

    fun pin(sub: String, remarks: String, on: Boolean): Settings {
        val now = pinned(sub).filter { it != remarks } + if (on) listOf(remarks) else emptyList()
        return copy(pins = if (now.isEmpty()) pins - sub else pins + (sub to now))
    }

    fun serverDns(sub: String, remarks: String, dns: String): Settings {
        val now = perServerDns[sub].orEmpty().let { if (dns.isBlank()) it - remarks else it + (remarks to dns.trim()) }
        return copy(perServerDns = if (now.isEmpty()) perServerDns - sub else perServerDns + (sub to now))
    }

    fun tunMtu(auto: Int): Int = mtu.takeIf { it in MTU_RANGE } ?: auto

    fun toJson(): JSONObject = JSONObject()
        .put("routingMode", routing.key)
        .put("pingMode", pingMode.key)
        .put("pingOnStart", pingOnStart)
        .put("sort", sort.key)
        .put("view", view.key)
        .put("grid", grid.key)
        .put("sectionRadius", sectionRadius)
        .put("sectionDivider", sectionDivider.key)
        .put("serverInfo", serverInfo.key)
        .put("exitFlags", exitFlags)
        .put("pingParallel", pingParallel)
        .put("pingTimeout", pingTimeout)
        .put("pingUrl", pingUrl)
        .put("ipUrl", ipUrl)
        .put("fakeDns", fakeDns)
        .put("serverDns", serverDns.key)
        .put("serverDnsCustom", serverDnsCustom)
        .put("dnsServers", dnsServers)
        .put("pins", JSONObject().apply { pins.forEach { (k, v) -> put(k, JSONArray(v)) } })
        .put("perServerDns", JSONObject().apply { perServerDns.forEach { (k, v) -> put(k, JSONObject(v)) } })
        .put("hwid", hwid)
        .put("sendHwid", sendHwid)
        .put("autoUpdate", autoUpdate)
        .put("subHeaders", subHeaders)
        .put("subClient", subClient.key)
        .put("userRules", userRules)
        .put("rules", Rules.toJson(rules))
        .put("sets", RuleSets.toJson(sets))
        .put("rest", rest.key)
        .put("autostart", autostart)
        .put("wifiPause", wifiPause)
        .put("lockOff", lockOff)
        .put("proxyMode", proxyMode)
        .put("proxyPort", proxyPort)
        .put("remoteControl", remoteControl)
        .put("theme", theme.key)
        .put("wallpaperColors", wallpaperColors)
        .put("seedColor", seedHex(seedColor))
        .put("debugLog", debugLog)
        .put("sendLogs", sendLogs)
        .put("uiDebug", uiDebug)
        .put("orbStyle", orbStyle)
        .put("healthCheck", healthCheck)
        .put("healthSec", healthSec)
        .put("healthNotify", healthNotify)
        .put("autoSwitch", autoSwitch)
        .put("healthIdle", healthIdle)
        .put("killSwitch", killSwitch)
        .put("background", background)
        .put("notifyDetails", notifyDetails)
        .put("liveUpdate", liveUpdate)
        .put("refreshNotices", refreshNotices)
        .put("mtu", mtu)
        .put("vpnAppsOnly", vpnAppsOnly)
        .put("fragment", fragment)
        .put("fragmentLength", fragmentLength)
        .put("fragmentInterval", fragmentInterval)
        .put("chain", chain)
        .put("chainSub", chainSub)
        .put("chainProfile", chainProfile)
        .put("autoSelect", autoSelect)
        .put("autoFeature", autoFeature)
        .put("autoSkipSubs", JSONArray(autoSkipSubs))
        .put("autoSkip", JSONObject().apply { autoSkip.forEach { (k, v) -> put(k, JSONArray(v)) } })
        .put("autoWlSubs", JSONArray(autoWlSubs))
        .put("autoWl", JSONObject().apply { autoWl.forEach { (k, v) -> put(k, JSONArray(v)) } })
        .put("autoPins", autoPins)
        .put("autoTransports", JSONArray(autoTransports))
        .put("autoWhitelist", autoWhitelist.key)
        .put("autoHome", autoHome)
        .put("autoFree", autoFree)
        .put("autoInterval", autoInterval)
        .put("autoMargin", autoMargin)
        .put("reserveOnlyOnWhitelists", reserveOnlyOnWhitelists)
        .put("autoSpeed", autoSpeed)
        .put("autoSaveData", autoSaveData)
        .put("autoNotify", autoNotify)
        .put("dismissedNotices", JSONArray(dismissedNotices))

    fun tunnelDiffers(other: Settings): Boolean = routing != other.routing || effectiveRules() != other.effectiveRules() ||
        Rules.servers(rules, sets) != Rules.servers(other.rules, other.sets) ||
        debugLog != other.debugLog || fakeDns != other.fakeDns || serverDns != other.serverDns || serverDnsCustom != other.serverDnsCustom || mtu != other.mtu ||
        ownDns() != other.ownDns() || perServerDns != other.perServerDns ||
        fragment != other.fragment || fragmentLength != other.fragmentLength || fragmentInterval != other.fragmentInterval ||
        chainEntry() != other.chainEntry() || background != other.background ||
        vpnOnlyApps() != other.vpnOnlyApps() || proxyMode != other.proxyMode || proxyPort != other.proxyPort

    fun autoSkipped(sub: String): List<String> = autoSkip[sub].orEmpty()

    fun autoSkip(sub: String, remarks: String, on: Boolean): Settings {
        val now = autoSkipped(sub).filter { it != remarks } + if (on) listOf(remarks) else emptyList()
        return copy(autoSkip = if (now.isEmpty()) autoSkip - sub else autoSkip + (sub to now))
    }

    fun autoSkipAll(sub: String, remarks: List<String>, on: Boolean): Settings = copy(autoSkip = if (on) autoSkip + (sub to remarks.distinct()) else autoSkip - sub)

    val autoWlMarked: Boolean get() = autoWlSubs.isNotEmpty() || autoWl.values.any { it.isNotEmpty() }

    fun autoWlServers(sub: String): List<String> = autoWl[sub].orEmpty()

    fun isAutoWl(sub: String, remarks: String): Boolean = sub in autoWlSubs || remarks in autoWlServers(sub)

    fun autoWlServer(sub: String, remarks: String, on: Boolean): Settings {
        val now = autoWlServers(sub).filter { it != remarks } + if (on) listOf(remarks) else emptyList()
        return copy(autoWl = if (now.isEmpty()) autoWl - sub else autoWl + (sub to now))
    }

    fun autoWlSub(sub: String, on: Boolean): Settings = copy(autoWlSubs = autoWlSubs.filter { it != sub } + if (on) listOf(sub) else emptyList(), autoWl = if (on) autoWl - sub else autoWl)

    fun onlySubs(ids: Set<String>): Settings = copy(
        autoSkipSubs = autoSkipSubs.filter { it in ids },
        autoSkip = autoSkip.filterKeys { it in ids },
        autoWlSubs = autoWlSubs.filter { it in ids },
        autoWl = autoWl.filterKeys { it in ids }
    )

    fun serviceDiffers(other: Settings): Boolean = autoSelect != other.autoSelect || autoInterval != other.autoInterval || reserveOnlyOnWhitelists != other.reserveOnlyOnWhitelists || autoWhitelist != other.autoWhitelist ||
        healthCheck != other.healthCheck || healthSec != other.healthSec || notifyDetails != other.notifyDetails || liveUpdate != other.liveUpdate ||
        background != other.background || autoUpdate != other.autoUpdate || wifiPause != other.wifiPause || lockOff != other.lockOff

    fun vpnOnlyApps(): List<String> = if (vpnAppsOnly) {
        (
            rules.filter { it.kind == RuleKind.App && it.action != RuleAction.Direct }.map { it.value.trim() } +
                sets.filter { it.enabled && it.action != RuleAction.Direct }.flatMap { it.apps }
            ).distinct().sorted()
    } else {
        emptyList()
    }

    fun directApps(): Set<String> {
        val out = mutableSetOf<String>()
        val decided = mutableSetOf<String>()
        for (line in effectiveRules().lines()) {
            val rule = line.trim().removePrefix("-").trim()
            if (rule.isEmpty() || rule.startsWith("#")) continue
            val m = PROCESS_RULE.matchEntire(rule) ?: continue
            val (app, action) = m.destructured
            if (decided.add(app) && action.equals("DIRECT", ignoreCase = true)) out += app
        }
        return out
    }

    fun effectiveRules(): String = Rules.mihomo(rules, userRules, sets, appsOnly = vpnAppsOnly)

    fun ruleSetUrls(): List<String> = RULE_SET.findAll(effectiveRules()).map { it.groupValues[1] }.toList()

    companion object {
        val MTU_RANGE = 576..9000
        const val HEALTH_MIN_SEC = 3
        val HEALTH_STEPS = listOf(3, 5, 10, 15, 30, 60, 120, 300, 600, 3600)
        const val AUTO_HOME = "yandex.ru, vk.ru, max.ru, gosuslugi.ru"
        private val AUTO_HOME_OLD = setOf(
            "ya.ru, gosuslugi.ru, vk.com",
            "yandex.ru, vk.com, max.ru, gosuslugi.ru",
            "yandex.ru, vk.ru, vk.com, max.ru, gosuslugi.ru"
        )
        const val AUTO_FREE = "google.com, cloudflare.com"
        private const val FILE = "settings.json"
        private const val NOTICES_KEPT = 200

        const val LIVE_UPDATE_SDK = 36

        fun noticeKey(scope: String, text: String): String = scope + ":" + java.security.MessageDigest.getInstance("SHA-1").digest(text.trim().toByteArray()).take(8).joinToString("") { "%02x".format(it) }
        private val PROCESS_RULE = Regex("""(?i)PROCESS-NAME\s*,\s*([\w.]+)\s*,\s*(\w+).*""")
        private val RULE_SET = Regex("""(?im)^\s*-?\s*RULE-SET\s*,\s*(https?://[^,\s]+)""")

        fun load(ctx: Context): Settings = Store.read(ctx, FILE)?.let {
            runCatching { fromJson(JSONObject(it)) }.getOrNull()
        } ?: Settings()

        fun fromJson(o: JSONObject): Settings = migrate(
            Settings(
                routing = RoutingMode.of(o.optString("routingMode"))
                    ?: if (o.optBoolean("subscriptionRules", true)) RoutingMode.Subscription else RoutingMode.Global,
                pingMode = PingMode.of(o.optString("pingMode")) ?: PingMode.Proxy,
                pingOnStart = o.optBoolean("pingOnStart", false),
                sort = ServerSort.of(o.optString("sort")) ?: ServerSort.Default,
                view = ServerView.of(o.optString("view")) ?: ServerView.Groups,
                grid = ServerGrid.of(o.optString("grid")) ?: ServerGrid.Two,
                sectionRadius = o.optInt("sectionRadius", 16).coerceIn(0, 32),
                sectionDivider = SectionDivider.of(o.optString("sectionDivider")) ?: SectionDivider.Gap,
                serverInfo = ServerInfo.of(o.optString("serverInfo"))
                    ?: if (o.optBoolean("descriptions", true)) ServerInfo.Description else ServerInfo.None,
                exitFlags = o.optBoolean("exitFlags", false),
                pingParallel = o.optInt("pingParallel", 16).let { if (it == 0) 32 else it },
                pingTimeout = o.optInt("pingTimeout", 5),
                pingUrl = o.optString("pingUrl"),
                ipUrl = o.optString("ipUrl"),
                fakeDns = o.optBoolean("fakeDns", false),
                serverDns = ServerDns.of(o.optString("serverDns")) ?: ServerDns.Auto,
                serverDnsCustom = o.optString("serverDnsCustom"),
                dnsServers = o.optString("dnsServers"),
                pins = o.optJSONObject("pins")?.let { p -> p.keys().asSequence().associateWith { k -> p.optJSONArray(k)?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty() }.filterValues { it.isNotEmpty() } }.orEmpty(),
                perServerDns = o.optJSONObject("perServerDns")?.let { p -> p.keys().asSequence().associateWith { k -> p.optJSONObject(k)?.let { m -> m.keys().asSequence().associateWith(m::optString).filterValues(String::isNotBlank) }.orEmpty() }.filterValues { it.isNotEmpty() } }.orEmpty(),
                hwid = o.optString("hwid"),
                sendHwid = o.optBoolean("sendHwid", true),
                autoUpdate = o.optBoolean("autoUpdate", true),
                subHeaders = o.optString("subHeaders"),
                subClient = SubClient.of(o.optString("subClient")) ?: SubClient.Happ,
                userRules = o.optString("userRules"),
                rules = migrateApps(Rules.fromJson(o.optJSONArray("rules")), o),
                sets = RuleSets.fromJson(o.optJSONArray("sets")),
                rest = RuleAction.of(o.optString("rest")) ?: RuleAction.Proxy,
                autostart = o.optBoolean("autostart", false),
                wifiPause = o.optBoolean("wifiPause", false),
                lockOff = o.optBoolean("lockOff", false),
                proxyMode = o.optBoolean("proxyMode", false),
                proxyPort = o.optInt("proxyPort", 10808).takeIf { it in 1024..65535 } ?: 10808,
                remoteControl = o.optBoolean("remoteControl", false),
                theme = ThemeMode.of(o.optString("theme")) ?: ThemeMode.System,
                wallpaperColors = o.optBoolean("wallpaperColors", true),
                seedColor = seedOf(o.optString("seedColor")),
                debugLog = o.optBoolean("debugLog", false),
                sendLogs = o.optBoolean("sendLogs", false),
                uiDebug = o.optBoolean("uiDebug", false),
                orbStyle = o.optString("orbStyle", "wave"),
                healthCheck = o.optBoolean("healthCheck", false),
                healthSec = o.optInt("healthSec", o.optInt("healthInterval", 5) * 60).let { s -> HEALTH_STEPS.minBy { kotlin.math.abs(it - s) } },
                healthNotify = o.optBoolean("healthNotify", true),
                autoSwitch = o.optBoolean("autoSwitch", false),
                healthIdle = o.optBoolean("healthIdle", false),
                killSwitch = o.optBoolean("killSwitch", false),
                background = o.optBoolean("background", false),
                notifyDetails = o.optBoolean("notifyDetails", false),
                liveUpdate = o.optBoolean("liveUpdate", true),
                refreshNotices = o.optBoolean("refreshNotices", false),
                mtu = o.optInt("mtu", 0),
                vpnAppsOnly = o.optBoolean("vpnAppsOnly", false),
                fragment = o.optBoolean("fragment", false),
                fragmentLength = o.optString("fragmentLength", Fragment.Light.length),
                fragmentInterval = o.optString("fragmentInterval", Fragment.Light.interval),
                chain = o.optBoolean("chain", false),
                chainSub = o.optString("chainSub"),
                chainProfile = o.optString("chainProfile"),
                autoSelect = o.optBoolean("autoSelect", false),
                autoFeature = o.optBoolean("autoFeature", o.optBoolean("autoSelect", false)),
                autoSkipSubs = o.optJSONArray("autoSkipSubs")?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty(),
                autoWlSubs = o.optJSONArray("autoWlSubs")?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty(),
                autoWl = o.optJSONObject("autoWl")?.let { p -> p.keys().asSequence().associateWith { k -> p.optJSONArray(k)?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty() }.filterValues { it.isNotEmpty() } }.orEmpty(),
                autoSkip = o.optJSONObject("autoSkip")?.let { p -> p.keys().asSequence().associateWith { k -> p.optJSONArray(k)?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty() }.filterValues { it.isNotEmpty() } }.orEmpty(),
                autoPins = o.optBoolean("autoPins", true),
                autoTransports = o.optJSONArray("autoTransports")?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty(),
                autoWhitelist = WhitelistMode.of(o.optString("autoWhitelist")) ?: WhitelistMode.Auto,
                autoHome = o.optString("autoHome", AUTO_HOME).takeIf { it.isNotBlank() && it !in AUTO_HOME_OLD } ?: AUTO_HOME,
                autoFree = o.optString("autoFree", AUTO_FREE).ifBlank { AUTO_FREE },
                autoInterval = o.optInt("autoInterval", 15),
                autoMargin = o.optInt("autoMargin", 50),
                reserveOnlyOnWhitelists = o.optBoolean("reserveOnlyOnWhitelists", false),
                autoSpeed = o.optBoolean("autoSpeed", true),
                autoSaveData = o.optBoolean("autoSaveData", false),
                autoNotify = o.optBoolean("autoNotify", true),
                dismissedNotices = o.optJSONArray("dismissedNotices")?.let { a -> (0 until a.length()).map(a::optString).filter(String::isNotEmpty) }.orEmpty()
            )
        )

        private fun migrate(s: Settings): Settings {
            if (s.routing != RoutingMode.Custom) return s
            val direct = s.rest == RuleAction.Direct && s.userRules.lines().none { it.trim().startsWith("MATCH", ignoreCase = true) }
            return s.copy(
                routing = RoutingMode.Global,
                userRules = if (direct) (s.userRules.trimEnd() + "\nMATCH,DIRECT").trimStart() else s.userRules
            )
        }

        fun save(ctx: Context, s: Settings) = Store.write(ctx, FILE, s.toJson().toString())

        fun seedHex(argb: Int): String = if (argb == 0) "" else "#%06x".format(argb and 0xFFFFFF)

        fun seedOf(hex: String?): Int = hex?.trim()?.removePrefix("#")?.takeIf { it.length == 6 }?.toIntOrNull(16)?.let { it or (0xFF shl 24) } ?: 0

        fun pruneSubs(ctx: Context) {
            val s = load(ctx)
            val kept = s.onlySubs(Subs.list(ctx).map { it.id }.toSet())
            if (kept != s) save(ctx, kept)
        }

        fun change(ctx: Context, old: Settings, new: Settings) {
            save(ctx, new)
            if (old.serviceDiffers(new) && VpnState.load(ctx).status == Status.On) {
                runCatching { dev.yzewe.rynet.vpn.RynetService.applySettings(ctx) }
            }
            if (old.exitFlags && !new.exitFlags) Countries.clear(ctx)
            if (old.killSwitch && !new.killSwitch) {
                runCatching { dev.yzewe.rynet.vpn.RynetService.unblock(ctx) }
            }
            if (new.effectiveRules() != old.effectiveRules()) {
                val app = ctx.applicationContext
                Thread {
                    runCatching {
                        val sub = Subs.selection(app)?.let { sel -> Subs.list(app).firstOrNull { it.id == sel.sub } }
                        val body = sub?.let { Subs.body(app, it.id).orEmpty() + "\n" + it.info.routing?.let { r -> GeoDat.localize(app, r) }.orEmpty() }.orEmpty()
                        Geo.update(app, body + "\n" + new.effectiveRules())
                    }
                }.start()
            }
        }
    }
}

enum class ServerDns(val key: String, private val ruTitle: String, private val enTitle: String) {
    Auto("auto", "Авто", "Auto"),
    Cloudflare("cloudflare", "Cloudflare", "Cloudflare"),
    Google("google", "Google", "Google"),
    System("system", "Системный", "System"),
    Custom("custom", "Свой", "Custom");

    val title: String get() = tr(ruTitle, enTitle)
    companion object {
        fun of(key: String) = entries.firstOrNull { it.key == key }
    }
}

private fun migrateApps(rules: List<Rule>, o: JSONObject): List<Rule> {
    val out = rules.toMutableList()
    val has = { pkg: String -> out.any { it.kind == RuleKind.App && it.value == pkg } }
    o.optJSONArray("appServers")?.let { a ->
        for (i in 0 until a.length()) {
            val x = a.optJSONObject(i) ?: continue
            val pkg = x.optString("pkg")
            val sub = x.optString("sub")
            val profile = x.optString("profile")
            if (pkg.isNotEmpty() && sub.isNotEmpty() && profile.isNotEmpty() && !has(pkg)) out += Rule(RuleKind.App, pkg, RuleAction.Server, Selection(sub, profile))
        }
    }
    if (o.optBoolean("vpnAppsOnly")) {
        o.optJSONArray("vpnApps")?.let { a ->
            for (i in 0 until a.length()) a.optString(i).takeIf { it.isNotEmpty() && !has(it) }?.let { out += Rule(RuleKind.App, it, RuleAction.Proxy) }
        }
    }
    return out
}
