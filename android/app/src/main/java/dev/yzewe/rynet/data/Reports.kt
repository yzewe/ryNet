package dev.yzewe.rynet.data

import android.content.Context
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

object Reports {
    private const val URL = "${Endpoints.API}/v1/reports"
    private const val FILE = "reports.json"
    const val EVERY_MS = 2 * 60 * 60_000L
    private const val MAX_BYTES = 15L shl 20

    class State(val install: String, val workedMs: Long, val sentAt: Long, val id: String?)

    @Synchronized
    fun state(ctx: Context): State {
        val o = Store.read(ctx, FILE)?.let { runCatching { JSONObject(it) }.getOrNull() }
        val install = o?.optString("install")?.takeIf { it.matches(Regex("[0-9a-f]{32}")) }
            ?: java.util.UUID.randomUUID().toString().replace("-", "")
        return State(install, o?.optLong("worked") ?: 0, o?.optLong("sentAt") ?: 0, o?.optString("id")?.ifEmpty { null })
    }

    @Synchronized
    private fun save(ctx: Context, s: State) = Store.write(
        ctx,
        FILE,
        JSONObject().put("install", s.install).put("worked", s.workedMs).put("sentAt", s.sentAt).put("id", s.id ?: "").toString()
    )

    fun worked(ctx: Context, ms: Long): Boolean {
        val s = state(ctx)
        val next = State(s.install, s.workedMs + ms.coerceIn(0, EVERY_MS), s.sentAt, s.id)
        save(ctx, next)
        return next.workedMs >= EVERY_MS
    }

    fun send(ctx: Context, version: String): Boolean {
        if (Diag.size(ctx) == 0L) return done(ctx, null)
        val sent = HashMap<String, Long>()
        val zip = runCatching { Diag.export(ctx, root = false, sub = "report", sent = sent) }.getOrElse { return false }
        try {
            if (zip.length() > MAX_BYTES) {
                Diag.clearExported(ctx, sent)
                return done(ctx, null)
            }
            val s = state(ctx)
            val c = URL(URL).openConnection() as HttpURLConnection
            try {
                c.requestMethod = "POST"
                c.connectTimeout = 20_000
                c.readTimeout = 60_000
                c.doOutput = true
                c.setFixedLengthStreamingMode(zip.length())
                c.setRequestProperty("Content-Type", "application/zip")
                c.setRequestProperty("X-Rynet-Install", s.install)
                c.setRequestProperty("X-Rynet-Version", version)
                c.outputStream.use { out -> zip.inputStream().use { it.copyTo(out) } }
                return when (c.responseCode) {
                    201 -> {
                        val id = runCatching { JSONObject(c.inputStream.bufferedReader().readText()).optString("id") }.getOrNull()
                        Diag.clearExported(ctx, sent)
                        done(ctx, id)
                    }

                    400, 413, 415 -> {
                        Diag.clearExported(ctx, sent)
                        done(ctx, null)
                    }

                    else -> false
                }
            } finally {
                c.disconnect()
            }
        } catch (_: Exception) {
            return false
        } finally {
            zip.delete()
        }
    }

    private fun done(ctx: Context, id: String?): Boolean {
        val s = state(ctx)
        save(ctx, State(s.install, 0, if (id != null) System.currentTimeMillis() else s.sentAt, id ?: s.id))
        return true
    }

    fun sentAt(ctx: Context): Long = runCatching { state(ctx).sentAt }.getOrDefault(0)
}
