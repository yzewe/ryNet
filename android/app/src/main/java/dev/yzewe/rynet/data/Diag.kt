package dev.yzewe.rynet.data

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.app.usage.UsageStatsManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.os.health.HealthStats
import android.os.health.SystemHealthManager
import android.os.health.UidHealthStats
import androidx.core.content.ContextCompat
import java.io.File
import java.io.RandomAccessFile
import java.time.Instant
import java.util.concurrent.TimeUnit
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import org.json.JSONObject

object Diag {
    fun dir(ctx: Context) = File(ctx.filesDir, "diag").apply { mkdirs() }

    private const val ROTATE_AT = 8L shl 20

    @SuppressLint("StaticFieldLeak")
    private var recorder: Recorder? = null

    fun start(ctx: Context) {
        if (recorder != null) return
        val r = Recorder(ctx.applicationContext)
        recorder = r
        r.start()
    }

    fun stop() {
        recorder?.stop()
        recorder = null
    }

    fun event(ctx: Context, name: String, vararg kv: Pair<String, Any?>) {
        if (recorder != null) write(ctx, line(name, kv))
    }

    private fun line(name: String, kv: Array<out Pair<String, Any?>>): JSONObject {
        val o = JSONObject().put("t", Instant.now().toString()).put("ev", name)
        for ((k, v) in kv) o.put(k, v ?: JSONObject.NULL)
        return o
    }

    @Synchronized
    private fun write(ctx: Context, o: JSONObject) {
        val f = File(dir(ctx), "device.log")
        if (f.length() > ROTATE_AT) f.renameTo(File(dir(ctx), "device.1.log"))
        f.appendText(o.toString() + "\n")
    }

    fun size(ctx: Context): Long = dir(ctx).listFiles()?.sumOf { it.length() } ?: 0

    @Volatile private var crashHook = false

    @Volatile private var crashFlush: (() -> Unit)? = null

    @Synchronized
    fun catchCrashes(ctx: Context, flush: (() -> Unit)? = null) {
        if (flush != null) crashFlush = flush
        if (crashHook) return
        crashHook = true
        val app = ctx.applicationContext
        val prev = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching {
                if (recorder != null || Settings.load(app).debugLog) {
                    val stack = e.stackTraceToString().take(16 shl 10)
                    val o = line("crash", arrayOf("thread" to t.name, "error" to "${e.javaClass.name}: ${e.message.orEmpty()}".take(500), "stack" to stack))
                    synchronized(this) {
                        java.io.FileOutputStream(File(dir(app), "device.log"), true).use { out ->
                            out.write((o.toString() + "\n").toByteArray())
                            out.flush()
                            runCatching { out.fd.sync() }
                        }
                    }
                }
            }
            runCatching { crashFlush?.invoke() }
            if (prev != null) {
                prev.uncaughtException(t, e)
            } else {
                android.os.Process.killProcess(android.os.Process.myPid())
                kotlin.system.exitProcess(10)
            }
        }
    }

    fun clearExported(ctx: Context, sent: Map<String, Long>) {
        for ((name, n) in sent) {
            val f = File(dir(ctx), name)
            if (!f.exists() || n <= 0) continue
            if (name.endsWith(".1.log")) {
                if (f.length() == n) f.delete()
                continue
            }
            if (name == "device.log") synchronized(this) { dropPrefix(f, n) } else dropPrefix(f, n)
        }
    }

    internal fun dropPrefix(f: File, n: Long) {
        RandomAccessFile(f, "rw").use { r ->
            var end = r.length()
            if (end < n) return
            var from = n
            var to = 0L
            val buf = ByteArray(64 shl 10)
            while (true) {
                while (from < end) {
                    r.seek(from)
                    val k = r.read(buf, 0, minOf(buf.size.toLong(), end - from).toInt())
                    if (k <= 0) break
                    r.seek(to)
                    r.write(buf, 0, k)
                    from += k
                    to += k
                }
                val now = r.length()
                if (now == end) break
                end = now
            }
            r.setLength(to)
        }
    }

    fun clear(ctx: Context) {
        dir(ctx).listFiles()?.forEach { f ->
            if (f.name.endsWith(".1.log")) f.delete() else runCatching { RandomAccessFile(f, "rw").use { it.setLength(0) } }
        }
    }

    internal fun mainFirst(trace: String, max: Int): String {
        val blocks = trace.replace("\r\n", "\n").split(Regex("\n[ \t]*\n"))
        val isThread = { b: String -> b.trimStart().startsWith("\"") }
        val head = blocks.takeWhile { !isThread(it) }
        val threads = blocks.drop(head.size)
        val (main, rest) = threads.partition { it.trimStart().startsWith("\"main\"") }
        return (head + main + rest).joinToString("\n\n").take(max)
    }

    fun tail(ctx: Context, maxBytes: Int = 48 shl 10): List<String> {
        val f = File(dir(ctx), "core.log").takeIf { it.exists() } ?: return emptyList()
        RandomAccessFile(f, "r").use { r ->
            val from = (r.length() - maxBytes).coerceAtLeast(0)
            r.seek(from)
            val bytes = ByteArray((r.length() - from).toInt())
            r.readFully(bytes)
            return bytes.decodeToString().lines().drop(if (from > 0) 1 else 0).filter { it.isNotBlank() }
        }
    }

    fun export(ctx: Context, root: Boolean, sub: String = "export", sent: MutableMap<String, Long>? = null): File {
        val out = File(ctx.cacheDir, sub).apply {
            mkdirs()
            listFiles()?.forEach { it.delete() }
        }
        val zip = File(out, "rynet-diag-${Instant.now().toString().take(16).replace(':', '-')}.zip")
        ZipOutputStream(zip.outputStream().buffered()).use { z ->
            fun put(name: String, bytes: ByteArray) {
                z.putNextEntry(ZipEntry(name))
                z.write(bytes)
                z.closeEntry()
            }
            dir(ctx).listFiles()?.sortedBy { it.name }?.forEach { f ->
                z.putNextEntry(ZipEntry(f.name))
                val n = f.length()
                f.inputStream().use { input ->
                    var left = n
                    val buf = ByteArray(64 shl 10)
                    while (left > 0) {
                        val k = input.read(buf, 0, minOf(buf.size.toLong(), left).toInt())
                        if (k <= 0) break
                        z.write(buf, 0, k)
                        left -= k
                    }
                    sent?.put(f.name, n - left)
                }
                z.closeEntry()
            }
            put("info.json", info(ctx).toString(2).toByteArray())
            if (root) {
                Root.run("dumpsys batterystats ${ctx.packageName}")?.let { put("batterystats.txt", it.toByteArray()) }
                Root.run("dumpsys batterystats --checkin")?.let { put("batterystats-checkin.txt", it.toByteArray()) }
            }
        }
        return zip
    }

    private fun stream(ctx: Context, sub: String, remarks: String): JSONObject? = runCatching {
        val arr = org.json.JSONArray(Subs.body(ctx, sub) ?: return null)
        val p = (0 until arr.length()).map { arr.getJSONObject(it) }.firstOrNull { it.optString("remarks") == remarks } ?: return null
        val obs = p.optJSONArray("outbounds") ?: return null
        val ob = (0 until obs.length()).map { obs.getJSONObject(it) }.firstOrNull { it.optString("protocol") !in setOf("freedom", "blackhole", "dns") } ?: return null
        val ss = ob.optJSONObject("streamSettings") ?: JSONObject()
        val keys = { o: JSONObject? -> o?.keys()?.asSequence()?.sorted()?.joinToString(",") }
        val port = ob.optJSONObject("settings")?.let { st ->
            st.optJSONArray("vnext")?.optJSONObject(0)?.optInt("port") ?: st.optJSONArray("servers")?.optJSONObject(0)?.optInt("port")
        }
        JSONObject()
            .put("protocol", ob.optString("protocol"))
            .put("port", port)
            .put("network", ss.optString("network", "tcp"))
            .put("security", ss.optString("security", "none"))
            .put("streamKeys", keys(ss))
            .put("tlsKeys", keys(ss.optJSONObject("tlsSettings")))
            .put("realityKeys", keys(ss.optJSONObject("realitySettings")))
    }.getOrNull()

    private fun info(ctx: Context): JSONObject {
        val s = Settings.load(ctx)
        val sel = Subs.selection(ctx)
        val profile = sel?.let { Subs.profiles(ctx, it.sub).byRemarks(it.profile) }
        return JSONObject()
            .put("app", runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull())
            .put("build", runCatching { androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(ctx.packageManager.getPackageInfo(ctx.packageName, 0)) }.getOrNull())
            .put("device", "${Build.MANUFACTURER} ${Build.MODEL}")
            .put("android", "${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            .put("routing", s.routing.key)
            .put("pingMode", s.pingMode.key)
            .put("userRules", s.effectiveRules())
            .put("profile", profile?.remarks)
            .put("kind", profile?.kind)
            .put("stream", sel?.let { stream(ctx, it.sub, it.profile) })
            .put("subscriptions", Subs.list(ctx).size)
    }
}

object Root {
    fun run(cmd: String, timeoutSec: Long = 60): String? = runCatching {
        val p = ProcessBuilder("su", "-c", cmd).redirectErrorStream(true).start()
        val out = p.inputStream.bufferedReader().use { it.readText() }
        val done = if (android.os.Build.VERSION.SDK_INT >= 26) p.waitFor(timeoutSec, TimeUnit.SECONDS) else p.waitFor() >= 0
        if (done && p.exitValue() == 0) out else null
    }.getOrNull()

    fun available(): Boolean = run("id", 20)?.contains("uid=0") == true
}

private const val HEALTH_EVERY_MS = 15 * 60_000L

private val EXIT_REASONS = listOf(
    "unknown", "exit_self", "signaled", "low_memory", "crash", "crash_native", "anr", "init_failure", "permission_change",
    "excessive_resources", "user_requested", "user_stopped", "dependency_died", "other", "freezer", "package_state", "package_updated"
)

private const val ANR_READ_MAX = 1 shl 20
private const val ANR_TRACE_MAX = 64 shl 10

private class Recorder(private val ctx: Context) {
    private val handler = Handler(Looper.getMainLooper())
    private val pm = ctx.getSystemService(PowerManager::class.java)
    private var healthAt = 0L
    private var pssAt = 0L

    private fun quiet() = !pm.isInteractive || pm.isDeviceIdleMode

    private fun every() = TimeUnit.MINUTES.toMillis(if (quiet()) 5 else 1)

    private val tick = object : Runnable {
        override fun run() {
            sample()
            if (SystemClock.elapsedRealtime() - healthAt >= HEALTH_EVERY_MS) health()
            handler.postDelayed(this, every())
        }
    }

    private fun syncQuiet() {
        runCatching { dev.yzewe.rynet.core.Native.setQuiet(quiet()) }
    }

    private fun resched() {
        handler.removeCallbacks(tick)
        handler.postDelayed(tick, every())
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            when (i.action) {
                Intent.ACTION_SCREEN_ON -> {
                    Diag.event(ctx, "screen", "on" to true)
                    syncQuiet()
                    resched()
                }

                Intent.ACTION_SCREEN_OFF -> {
                    Diag.event(ctx, "screen", "on" to false)
                    syncQuiet()
                }

                Intent.ACTION_POWER_CONNECTED -> Diag.event(ctx, "power", "plugged" to true)

                Intent.ACTION_POWER_DISCONNECTED -> Diag.event(ctx, "power", "plugged" to false)

                PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED -> {
                    Diag.event(ctx, "doze", "on" to pm.isDeviceIdleMode)
                    syncQuiet()
                }

                PowerManager.ACTION_POWER_SAVE_MODE_CHANGED -> Diag.event(ctx, "saver", "on" to pm.isPowerSaveMode)
            }
        }
    }

    fun start() {
        Diag.event(
            ctx,
            "recorder",
            "state" to "start",
            "device" to "${Build.MANUFACTURER} ${Build.MODEL}",
            "android" to Build.VERSION.SDK_INT,
            "app" to runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull(),
            "build" to runCatching { androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(ctx.packageManager.getPackageInfo(ctx.packageName, 0)) }.getOrNull(),
            "session" to java.util.UUID.randomUUID().toString().take(8)
        )
        exits()
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_POWER_CONNECTED)
            addAction(Intent.ACTION_POWER_DISCONNECTED)
            addAction(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED)
            addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
        }
        ContextCompat.registerReceiver(ctx, receiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
        syncQuiet()
        health()
        sample()
        handler.postDelayed(tick, every())
    }

    private fun exits() {
        if (Build.VERSION.SDK_INT < 30) return
        val prefs = ctx.getSharedPreferences("diag", Context.MODE_PRIVATE)
        val seen = prefs.getLong("exit_seen", 0L)
        val fresh = runCatching { ctx.getSystemService(ActivityManager::class.java).getHistoricalProcessExitReasons(ctx.packageName, 0, 16) }
            .getOrNull().orEmpty().filter { it.timestamp > seen }.sortedBy { it.timestamp }
        for (e in fresh) {
            val trace = if (e.reason == ApplicationExitInfo.REASON_ANR) anrTrace(e) else null
            Diag.event(
                ctx, "exit",
                "at" to Instant.ofEpochMilli(e.timestamp).toString(),
                "process" to e.processName,
                "reason" to (EXIT_REASONS.getOrNull(e.reason) ?: e.reason.toString()),
                "status" to e.status,
                "importance" to e.importance,
                "pss_kb" to e.pss,
                "rss_kb" to e.rss,
                "desc" to e.description?.take(200),
                *listOfNotNull<Pair<String, Any?>>(trace?.let { "trace" to it }).toTypedArray()
            )
        }
        fresh.lastOrNull()?.let { prefs.edit().putLong("exit_seen", it.timestamp).apply() }
    }

    @androidx.annotation.RequiresApi(30)
    private fun anrTrace(e: ApplicationExitInfo): String? = runCatching {
        e.traceInputStream?.use { input ->
            val buf = java.io.ByteArrayOutputStream()
            val chunk = ByteArray(16 shl 10)
            while (buf.size() < ANR_READ_MAX) {
                val k = input.read(chunk)
                if (k <= 0) break
                buf.write(chunk, 0, k)
            }
            Diag.mainFirst(buf.toString(Charsets.UTF_8.name()), ANR_TRACE_MAX)
        }
    }.getOrNull()

    fun stop() {
        handler.removeCallbacks(tick)
        runCatching { ctx.unregisterReceiver(receiver) }
        runCatching { dev.yzewe.rynet.core.Native.setQuiet(false) }
        health()
        Diag.event(ctx, "recorder", "state" to "stop")
    }

    private fun sample() {
        val bm = ctx.getSystemService(BatteryManager::class.java)
        val sticky = ContextCompat.registerReceiver(ctx, null, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
        fun prop(id: Int) = bm.getIntProperty(id).takeIf { it != Int.MIN_VALUE && it != 0 }
        Diag.event(
            ctx, "sample",
            "bat" to prop(BatteryManager.BATTERY_PROPERTY_CAPACITY),
            "ua" to prop(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW),
            "ua_avg" to prop(BatteryManager.BATTERY_PROPERTY_CURRENT_AVERAGE),
            "uah" to prop(BatteryManager.BATTERY_PROPERTY_CHARGE_COUNTER),
            "mv" to sticky?.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1),
            "temp" to sticky?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, -1)?.let { it / 10.0 },
            "plug" to sticky?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0),
            "screen" to pm.isInteractive,
            "doze" to pm.isDeviceIdleMode,
            "saver" to pm.isPowerSaveMode,
            "bucket" to if (Build.VERSION.SDK_INT >= 28) ctx.getSystemService(UsageStatsManager::class.java).appStandbyBucket else null,
            "net" to network(),
            "mem" to memory(pss = (SystemClock.elapsedRealtime() - pssAt >= HEALTH_EVERY_MS).also { if (it) pssAt = SystemClock.elapsedRealtime() }),
            "up" to SystemClock.uptimeMillis(),
            "rt" to SystemClock.elapsedRealtime()
        )
    }

    private fun memory(pss: Boolean): JSONObject? = runCatching {
        fun kb(file: String, key: String) = File("/proc/self/$file").readLines().firstOrNull { it.startsWith(key) }
            ?.substringAfter(':')?.trim()?.substringBefore(' ')?.toLongOrNull()
        val rt = Runtime.getRuntime()
        JSONObject()
            .put("rss_kb", kb("status", "VmRSS"))
            .apply { if (pss) put("pss_kb", kb("smaps_rollup", "Pss")) }
            .put("java_kb", (rt.totalMemory() - rt.freeMemory()) / 1024)
            .put("native_kb", android.os.Debug.getNativeHeapAllocatedSize() / 1024)
    }.getOrNull()

    private fun network(): String? {
        val cm = ctx.getSystemService(ConnectivityManager::class.java)

        @Suppress("DEPRECATION")
        val caps = dev.yzewe.rynet.vpn.RynetService.underlying?.let { cm.getNetworkCapabilities(it) }
            ?: cm.allNetworks.mapNotNull { cm.getNetworkCapabilities(it) }
                .firstOrNull { !it.hasTransport(NetworkCapabilities.TRANSPORT_VPN) && it.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) }
            ?: return null
        val type = when {
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
            else -> "other"
        }
        return if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)) type else "$type/metered"
    }

    private fun health() {
        healthAt = SystemClock.elapsedRealtime()
        val stats = runCatching { ctx.getSystemService(SystemHealthManager::class.java).takeMyUidSnapshot() }.getOrNull() ?: return
        val names = UidHealthStats::class.java.fields
            .filter { it.type == Int::class.javaPrimitiveType }
            .associate { it.getInt(null) to it.name.lowercase() }
        Diag.event(ctx, "health", "stats" to json(stats, names))
    }

    private fun json(s: HealthStats, names: Map<Int, String>): JSONObject {
        val o = JSONObject()
        for (i in 0 until s.measurementKeyCount) {
            val k = s.getMeasurementKeyAt(i)
            o.put(names[k] ?: "m$k", s.getMeasurement(k))
        }
        for (i in 0 until s.timerKeyCount) {
            val k = s.getTimerKeyAt(i)
            o.put(names[k] ?: "t$k", JSONObject().put("n", s.getTimerCount(k)).put("ms", s.getTimerTime(k)))
        }
        for (i in 0 until s.timersKeyCount) {
            val k = s.getTimersKeyAt(i)
            val m = JSONObject()
            for ((name, t) in s.getTimers(k)) m.put(name, JSONObject().put("n", t.count).put("ms", t.time))
            if (m.length() > 0) o.put(names[k] ?: "ts$k", m)
        }
        return o
    }
}
