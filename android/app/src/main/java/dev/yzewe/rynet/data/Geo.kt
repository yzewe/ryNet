package dev.yzewe.rynet.data

import android.content.Context
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

object Geo {
    private const val BASE = Endpoints.GEO_FILES
    private val REF = Regex("\"(geoip|geosite):([a-z0-9!@._-]+)\"")
    private val MIHOMO_REF = Regex("""(?im)^\s*-?\s*(GEOSITE|GEOIP)\s*,\s*([a-z0-9!@._-]+)""")
    private val SMART = setOf("geosite" to "category-ru", "geoip" to "ru")

    fun dir(ctx: Context) = File(ctx.filesDir, "geo").apply { mkdirs() }

    fun referenced(text: String): Set<Pair<String, String>> = (
        REF.findAll(text).map { it.groupValues[1] to it.groupValues[2].removePrefix("!") } +
            MIHOMO_REF.findAll(text).map { it.groupValues[1].lowercase() to it.groupValues[2].lowercase() } + SMART
        )
        .filter { it != ("geoip" to "private") && !GeoDat.isOwn(it.second) }
        .toSet()

    fun update(ctx: Context, text: String) {
        for ((kind, name) in referenced(text)) {
            runCatching { fetch(ctx, "$BASE/$kind/$name.list", "$kind-$name") }
        }
        for (url in Settings.load(ctx).ruleSetUrls()) {
            runCatching { fetch(ctx, url, ruleSetKey(url)) }
        }
    }

    fun ruleSetKey(url: String): String {
        val d = java.security.MessageDigest.getInstance("SHA-256").digest(url.toByteArray())
        return "ruleset-" + d.take(8).joinToString("") { "%02x".format(it) }
    }

    private fun fetch(ctx: Context, url: String, name: String) {
        val file = File(dir(ctx), "$name.list")
        val etagFile = File(dir(ctx), "$name.etag")
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 30_000
        if (file.exists() && etagFile.exists()) conn.setRequestProperty("If-None-Match", etagFile.readText())
        try {
            withDeadline(conn, 60_000) { d ->
                when (conn.responseCode) {
                    304 -> return

                    200 -> {
                        val tmp = File(file.path + ".tmp")
                        d.wrap(conn.inputStream).use { input -> tmp.outputStream().use { input.copyTo(it) } }
                        tmp.renameTo(file)
                        conn.getHeaderField("ETag")?.let { etagFile.writeText(it) }
                    }
                }
            }
        } finally {
            conn.disconnect()
        }
    }
}
