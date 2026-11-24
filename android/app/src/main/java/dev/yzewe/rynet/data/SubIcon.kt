package dev.yzewe.rynet.data

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

object SubIcon {
    private const val MAX = 256 * 1024
    private const val SIDE_MAX = 2048
    private const val SIDE = 256
    private const val REDIRECTS = 5
    private val LINK = Regex("""<link\b[^>]*>""", RegexOption.IGNORE_CASE)
    private val REL = Regex("""rel\s*=\s*["']([^"']+)["']""", RegexOption.IGNORE_CASE)
    private val HREF = Regex("""href\s*=\s*["']([^"']+)["']""", RegexOption.IGNORE_CASE)

    private fun file(ctx: Context, id: String) = File(File(ctx.filesDir, "subs").apply { mkdirs() }, "$id.icon.png")

    fun load(ctx: Context, id: String): Bitmap? = file(ctx, id).takeIf { it.exists() }?.let { runCatching { BitmapFactory.decodeFile(it.path) }.getOrNull() }

    fun fresh(ctx: Context, id: String): Boolean = file(ctx, id).let { it.exists() && System.currentTimeMillis() - it.lastModified() < FRESH_MS }

    private const val FRESH_MS = 7 * 24 * 3_600_000L

    fun remove(ctx: Context, id: String) {
        file(ctx, id).delete()
    }

    fun fetch(ctx: Context, id: String, info: SubInfo) {
        val page = URL(info.webPage ?: info.url)
        val origin = "${page.protocol}://${page.authority}"
        val own = runCatching { URL(info.url).host }.getOrNull()
        val declared = info.webPage?.let { runCatching { declaredIcons(it, own) }.getOrNull() }.orEmpty()
        val candidates = declared + listOf("$origin/apple-touch-icon.png", "$origin/favicon.ico")
        for (url in candidates.distinct()) {
            val bmp = runCatching { get(url, own)?.let(::decode) }.getOrNull() ?: continue
            file(ctx, id).outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
            return
        }
    }

    private fun decode(bytes: ByteArray): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        val sample = sampleFor(bounds.outWidth, bounds.outHeight) ?: return null
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
    }

    internal fun sampleFor(width: Int, height: Int): Int? {
        if (width < 16 || height < 16 || width > SIDE_MAX || height > SIDE_MAX) return null
        var sample = 1
        while (maxOf(width, height) / (sample * 2) >= SIDE) sample *= 2
        return sample
    }

    internal fun local(a: java.net.InetAddress): Boolean {
        if (a.isAnyLocalAddress || a.isLoopbackAddress || a.isLinkLocalAddress || a.isSiteLocalAddress || a.isMulticastAddress) return true
        val b = a.address
        val first = b[0].toInt() and 0xff
        return when (b.size) {
            4 -> first == 0 || (first == 100 && (b[1].toInt() and 0xc0) == 64) || first >= 240
            else -> (first and 0xfe) == 0xfc
        }
    }

    private fun allowed(u: URL, own: String?): Boolean {
        if (u.protocol != "http" && u.protocol != "https") return false
        if (own != null && u.host.equals(own, ignoreCase = true)) return true
        val addrs = runCatching { java.net.InetAddress.getAllByName(u.host) }.getOrNull() ?: return false
        return addrs.isNotEmpty() && addrs.none(::local)
    }

    private fun declaredIcons(page: String, own: String?): List<String> {
        val html = get(page, own)?.decodeToString() ?: return emptyList()
        return LINK.findAll(html).mapNotNull { m ->
            val rel = REL.find(m.value)?.groupValues?.get(1)?.lowercase() ?: return@mapNotNull null
            val href = HREF.find(m.value)?.groupValues?.get(1) ?: return@mapNotNull null
            val rank = when {
                "apple-touch-icon" in rel -> 0
                "icon" in rel -> 1
                else -> return@mapNotNull null
            }
            rank to URL(URL(page), href).toString()
        }.sortedBy { it.first }.map { it.second }.toList()
    }

    private fun get(url: String, own: String?): ByteArray? {
        var at = URL(url)
        repeat(REDIRECTS) {
            if (!allowed(at, own)) return null
            when (val r = getOnce(at)) {
                is URL -> at = r
                else -> return r as ByteArray?
            }
        }
        return null
    }

    private fun getOnce(url: URL): Any? {
        val c = url.openConnection() as HttpURLConnection
        c.connectTimeout = 4000
        c.readTimeout = 4000
        c.instanceFollowRedirects = false
        c.setRequestProperty("User-Agent", "Mozilla/5.0")
        return try {
            withDeadline(c, 15_000) { d ->
                if (c.responseCode in setOf(301, 302, 303, 307, 308)) {
                    return c.getHeaderField("Location")?.let { runCatching { URL(url, it) }.getOrNull() }
                }
                if (c.responseCode != 200) return null
                d.wrap(c.inputStream).use { input ->
                    val out = java.io.ByteArrayOutputStream()
                    val buf = ByteArray(16 * 1024)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        if (out.size() > MAX) return null
                    }
                    out.toByteArray()
                }
            }
        } finally {
            c.disconnect()
        }
    }
}
