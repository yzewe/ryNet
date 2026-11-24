package dev.yzewe.rynet.data

import android.content.Context
import android.os.Debug
import java.io.File
import java.time.Instant

object Profiling {
    fun exportDir(ctx: Context) = File(ctx.cacheDir, "export").apply { mkdirs() }

    fun stamp(): String = Instant.now().toString().take(19).replace(':', '-')

    fun clean(ctx: Context) {
        exportDir(ctx).listFiles()?.filter { it.name.endsWith(".hprof") || it.name.endsWith(".trace") }?.forEach { it.delete() }
    }

    fun dumpHeap(file: File) {
        val tmp = File(file.path + ".part")
        runCatching {
            Debug.dumpHprofData(tmp.path)
            tmp.renameTo(file)
        }.onFailure { tmp.delete() }
    }

    private var tracing: File? = null

    val isTracing: Boolean get() = tracing != null

    fun startTrace(ctx: Context): File {
        val f = File(exportDir(ctx), "rynet-ui-${stamp()}.trace")
        Debug.startMethodTracingSampling(f.path, 64 shl 20, 1000)
        tracing = f
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({ if (tracing == f) stopTrace() }, TRACE_MAX_MS)
        return f
    }

    private const val TRACE_MAX_MS = 60_000L

    fun stopTrace(): File? {
        val f = tracing ?: return null
        Debug.stopMethodTracing()
        tracing = null
        return f
    }
}
