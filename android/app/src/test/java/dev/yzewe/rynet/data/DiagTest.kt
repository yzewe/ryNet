package dev.yzewe.rynet.data

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowActivityManager.ApplicationExitInfoBuilder

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class DiagTest {
    private val ctx = ApplicationProvider.getApplicationContext<Context>()

    private fun exits() = File(Diag.dir(ctx), "device.log").readLines().map(::JSONObject).filter { it.optString("ev") == "exit" }

    @Test
    fun previousProcessExitsAreLoggedOnce() {
        shadowOf(ctx.getSystemService(ActivityManager::class.java)).addApplicationExitInfo(
            ApplicationExitInfoBuilder.newBuilder().setProcessName("${ctx.packageName}:core")
                .setReason(ApplicationExitInfo.REASON_CRASH_NATIVE).setTimestamp(1_000).setPss(4096).build()
        )
        Diag.start(ctx)
        Diag.stop()
        Diag.start(ctx)
        Diag.stop()
        val logged = exits()
        assertEquals(1, logged.size)
        assertEquals("crash_native", logged[0].getString("reason"))
        assertEquals("${ctx.packageName}:core", logged[0].getString("process"))
    }

    @Test
    fun reportClearsOnlyWhatWasExported() {
        Diag.clear(ctx)
        val dev = File(Diag.dir(ctx), "device.log")
        val core = File(Diag.dir(ctx), "core.log")
        dev.writeText("old-1\nold-2\n")
        core.writeText("core-old\n")
        val sent = HashMap<String, Long>()
        val zip = Diag.export(ctx, root = false, sub = "report-test", sent = sent)
        assertEquals(dev.length(), sent["device.log"])
        dev.appendText("during\n")
        core.appendText("core-during\n")
        Diag.clearExported(ctx, sent)
        assertEquals("during\n", dev.readText())
        assertEquals("core-during\n", core.readText())
        val names = java.util.zip.ZipFile(zip).use { z -> z.entries().toList().map { it.name } }
        org.junit.Assert.assertTrue(names.containsAll(listOf("device.log", "core.log")))
        zip.delete()
    }

    @Test
    fun rotatedFileIsKept() {
        Diag.clear(ctx)
        val f = File(Diag.dir(ctx), "device.log")
        f.writeText("new")
        Diag.clearExported(ctx, mapOf("device.log" to 100L))
        assertEquals("new", f.readText())
    }

    @Test
    fun dropPrefixKeepsTail() {
        val f = File.createTempFile("drop", ".log")
        val big = (0 until 200_000).joinToString("") { (it % 10).toString() }
        f.writeText(big)
        Diag.dropPrefix(f, 70_001)
        assertEquals(big.substring(70_001), f.readText())
        f.delete()
    }

    @Test
    fun anrTraceMainThreadFirstAndTrimmed() {
        val trace = """
            ----- pid 123 at 2026-10-20 20:34:50 -----
            Cmd line: dev.yzewe.rynet

            "Signal Catcher" daemon prio=10 tid=2 Runnable
              at nothing

            "rynet-health" prio=5 tid=20 Waiting
              at java.lang.Object.wait

            "main" prio=5 tid=1 Blocked
              at dev.yzewe.rynet.ui.SettingsActivity.onCreate(SettingsActivity.kt:42)
              - waiting to lock <0x01> held by thread 20
        """.trimIndent()
        val out = Diag.mainFirst(trace, 64 shl 10)
        val blocks = out.split("\n\n")
        org.junit.Assert.assertTrue(blocks[0], blocks[0].startsWith("----- pid 123"))
        org.junit.Assert.assertTrue(blocks[1], blocks[1].startsWith("\"main\""))
        org.junit.Assert.assertTrue(out.contains("SettingsActivity.kt:42"))
        assertEquals(3 + 1, blocks.size)
        val short = Diag.mainFirst(trace, 120)
        assertEquals(120, short.length)
        org.junit.Assert.assertTrue(short.contains("\"main\""))
    }

    @Test
    fun anrExitCarriesTrace() {
        shadowOf(ctx.getSystemService(ActivityManager::class.java)).addApplicationExitInfo(
            ApplicationExitInfoBuilder.newBuilder().setProcessName(ctx.packageName)
                .setReason(ApplicationExitInfo.REASON_ANR).setTimestamp(2_000)
                .setTraceInputStream("----- pid 1 -----\n\n\"x\" prio=5\n  at a\n\n\"main\" prio=5 tid=1\n  at b\n".byteInputStream())
                .build()
        )
        Diag.start(ctx)
        Diag.stop()
        val anr = exits().last { it.getString("reason") == "anr" }
        org.junit.Assert.assertTrue(anr.getString("trace").contains("\"main\" prio=5 tid=1\n  at b"))
        org.junit.Assert.assertTrue(anr.getString("trace").indexOf("\"main\"") < anr.getString("trace").indexOf("\"x\""))
    }

    @Test
    fun crashIsWrittenAndChained() {
        File(Diag.dir(ctx), "device.log").delete()
        val before = Thread.getDefaultUncaughtExceptionHandler()
        var chained: Throwable? = null
        Thread.setDefaultUncaughtExceptionHandler { _, e -> chained = e }
        try {
            Settings.save(ctx, Settings.load(ctx).copy(debugLog = true))
            Diag.catchCrashes(ctx)
            val handler = Thread.getDefaultUncaughtExceptionHandler()!!
            val beforeService = IllegalStateException("before VPN")
            handler.uncaughtException(Thread.currentThread(), beforeService)
            assertEquals(beforeService, chained)
            var flushed = false
            Diag.catchCrashes(ctx) { flushed = true }
            org.junit.Assert.assertSame(handler, Thread.getDefaultUncaughtExceptionHandler())
            val boom = IllegalStateException("boom")
            Thread.getDefaultUncaughtExceptionHandler()!!.uncaughtException(Thread.currentThread(), boom)
            assertEquals(boom, chained)
            org.junit.Assert.assertTrue(flushed)
            val crash = File(Diag.dir(ctx), "device.log").readLines().map(::JSONObject).last { it.optString("ev") == "crash" }
            assertEquals("java.lang.IllegalStateException: boom", crash.getString("error"))
            org.junit.Assert.assertTrue(crash.getString("stack").contains("crashIsWrittenAndChained"))
        } finally {
            Diag.stop()
            Thread.setDefaultUncaughtExceptionHandler(before)
        }
    }
}
