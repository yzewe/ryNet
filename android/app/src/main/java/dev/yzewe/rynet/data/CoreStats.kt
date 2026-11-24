package dev.yzewe.rynet.data

import android.content.Context
import android.os.SystemClock
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteOrder
import java.nio.MappedByteBuffer
import java.nio.channels.FileChannel
import org.json.JSONObject

class CoreStats private constructor(private val buf: MappedByteBuffer) {
    data class Snapshot(
        val generation: Long,
        val started: Long,
        val up: Long,
        val down: Long,
        val tcpActive: Long,
        val udpActive: Long,
        val tcpTotal: Long,
        val dns: Long,
        val at: Long,
        val errors: List<Long> = emptyList(),
        val proxyConns: Long = 0,
        val proxyOk: Long = 0,
        val proxyFail: Long = 0,
        val keepAlive: Long = 0
    )

    fun read(): Snapshot? {
        if (buf.getLong(0) != MAGIC) return null
        fun slot(i: Int) = buf.getLong(i * 8)
        return Snapshot(slot(1), slot(2), slot(3), slot(4), slot(5), slot(6), slot(7), slot(8), SystemClock.elapsedRealtime(), (9..13).map(::slot), slot(17), slot(18), slot(19), slot(20))
    }

    data class Skip(val why: String, val tried: Long, val ok: Long)

    companion object {
        const val HEALTH_MIN_TRIED = 4

        fun healthSkip(mark: Snapshot?, now: Snapshot?): Skip? {
            if (mark == null || now == null || now.generation != mark.generation) return null
            val ok = now.proxyOk - mark.proxyOk
            val tried = (now.proxyConns - mark.proxyConns) + (now.proxyFail - mark.proxyFail)
            if (tried <= 0) return Skip("idle", 0, 0)
            val failing = tried >= HEALTH_MIN_TRIED && (tried - ok) * 2 >= tried
            return if (ok > 0 && !failing) Skip("served", tried, ok) else null
        }

        private const val MAGIC = 0x3154_5354_454E_5952L

        fun file(ctx: Context) = File(ctx.filesDir, "stats.bin")

        fun open(ctx: Context): CoreStats? = runCatching {
            val f = file(ctx).takeIf { it.length() >= 4096 } ?: return null
            RandomAccessFile(f, "r").use { r ->
                CoreStats(r.channel.map(FileChannel.MapMode.READ_ONLY, 0, 4096).apply { order(ByteOrder.LITTLE_ENDIAN) })
            }
        }.getOrNull()
    }
}

data class CoreProc(
    val rssKb: Long,
    val cpuMs: Long,
    val wakeups: Long?,
    val pssKb: Long? = null,
    val javaKb: Long? = null,
    val nativeKb: Long? = null,
    val ageS: Long? = null
) {
    val wakeRate: Double? get() = if (wakeups != null && ageS != null && ageS > 0) wakeups.toDouble() / ageS else null

    companion object {
        private const val FILE = "proc.json"
        private const val PSS_MS = 30_000L
        private var pss: Long? = null
        private var pssAt = 0L

        fun sample(ctx: Context, coreTid: Int) {
            val p = runCatching {
                val self = File("/proc/self")
                fun field(text: String, key: String) = text.lineSequence().firstOrNull { it.startsWith(key) }?.substringAfter(':')?.trim()?.substringBefore(' ')?.toLongOrNull()
                val stat = File(self, "stat").readText().substringAfterLast(')').trim().split(' ')
                val cpu = (stat.getOrNull(11)?.toLongOrNull() ?: 0) + (stat.getOrNull(12)?.toLongOrNull() ?: 0)
                val wake = coreTid.takeIf { it > 0 }?.let {
                    runCatching { field(File(self, "task/$it/status").readText(), "voluntary_ctxt_switches") }.getOrNull()
                }
                val age = coreTid.takeIf { it > 0 }?.let { tid ->
                    runCatching {
                        val start = File(self, "task/$tid/stat").readText().substringAfterLast(')').trim().split(' ')[19].toLong() / 100
                        android.os.SystemClock.elapsedRealtime() / 1000 - start
                    }.getOrNull()
                }
                val rt = Runtime.getRuntime()
                CoreProc(
                    field(File(self, "status").readText(), "VmRSS") ?: 0,
                    cpu * 10,
                    wake,
                    pssKb = SystemClock.elapsedRealtime().let { now ->
                        if (pss == null || now - pssAt >= PSS_MS) {
                            pss = runCatching { field(File(self, "smaps_rollup").readText(), "Pss") }.getOrNull()
                            pssAt = now
                        }
                        pss
                    },
                    javaKb = (rt.totalMemory() - rt.freeMemory()) / 1024,
                    nativeKb = android.os.Debug.getNativeHeapAllocatedSize() / 1024,
                    ageS = age
                )
            }.getOrNull() ?: return
            Store.write(
                ctx,
                FILE,
                JSONObject().put("rss", p.rssKb).put("cpu", p.cpuMs).put("wake", p.wakeups ?: -1)
                    .put("pss", p.pssKb ?: -1).put("java", p.javaKb ?: -1).put("native", p.nativeKb ?: -1).put("age", p.ageS ?: -1)
                    .put("at", System.currentTimeMillis()).toString()
            )
        }

        fun load(ctx: Context): CoreProc? = Store.read(ctx, FILE)?.let {
            runCatching {
                val o = JSONObject(it)
                if (System.currentTimeMillis() - o.getLong("at") > 10_000) return null
                fun opt(k: String) = o.optLong(k, -1).takeIf { v -> v >= 0 }
                CoreProc(o.getLong("rss"), o.getLong("cpu"), opt("wake"), opt("pss"), opt("java"), opt("native"), opt("age"))
            }.getOrNull()
        }
    }
}
