package dev.yzewe.rynet.data

import android.content.Context
import android.content.Intent
import android.net.Uri
import dev.yzewe.rynet.tr
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import org.json.JSONArray
import org.json.JSONObject

object Update {
    private const val BASE = "${Endpoints.DOWNLOADS}/android/"
    private const val CHECK_MS = 20_000L
    private const val DOWNLOAD_MS = 600_000L
    private const val MAX_APK = 200L * 1024 * 1024
    private const val API = "${Endpoints.API}/v1/downloads"
    private const val HOST = Endpoints.DOWNLOADS
    private const val RELEASES = "${Endpoints.API}/v1/releases"
    private const val MAX_API = 256 * 1024
    private const val MAX_NEWS = 4000

    class Release(val version: String, val build: Long, val url: String, val sha256: String? = null, val bytes: Long = 0) {
        var news: News? = null
    }

    class News(val text: String, val link: String?)

    fun track(leanback: Boolean, sdk: Int, abis: Array<String>): String = when {
        leanback -> "tv"
        sdk < 26 || "arm64-v8a" !in abis -> "legacy"
        else -> ""
    }

    private val FILE = Regex("""ryNet-(\d{1,4}(?:\.\d{1,4}){1,3})-(\d{1,9})(?:-(legacy|tv))?\.apk""")
    private val HASH = Regex("""/releases/([0-9a-fA-F]{64})/""")

    fun parseLocation(location: String, base: String): Release? {
        val url = runCatching { URI(base).resolve(location.trim()) }.getOrNull() ?: return null
        val path = url.path ?: return null
        val m = FILE.matchEntire(path.substringAfterLast('/')) ?: return null
        val build = m.groupValues[2].toLongOrNull() ?: return null
        return Release(m.groupValues[1], build, url.toString(), HASH.find(path)?.groupValues?.get(1)?.lowercase())
    }

    private val SHA = Regex("[0-9a-fA-F]{64}")

    fun parseDownloads(body: String, track: String): Release? {
        val arr = runCatching { JSONArray(body) }.getOrNull() ?: return null
        val alias = "/android/${if (track.isEmpty()) "" else "$track/"}latest.apk"
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            if (o.opt("alias") == alias) return entry(o, track)
        }
        return null
    }

    private fun entry(o: JSONObject, track: String): Release? {
        val file = o.opt("file") as? String ?: return null
        val sha = (o.opt("sha256") as? String)?.takeIf { SHA.matches(it) }?.lowercase() ?: return null
        val bytes = (o.opt("bytes") as? Number)?.toLong() ?: return null
        if (bytes <= 0 || bytes > MAX_APK) return null
        val m = FILE.matchEntire(file) ?: return null
        if (m.groupValues[3] != track) return null
        val build = m.groupValues[2].toLongOrNull() ?: return null
        val url = o.opt("url") as? String
        if (url != null && !url.startsWith("$HOST/")) return null
        val path = o.opt("path") as? String
        val link = if (path.isNullOrEmpty()) {
            url ?: return null
        } else {
            if (!path.startsWith("/releases/") || !path.lowercase().contains("/$sha/") || path.substringAfterLast('/') != file) return null
            HOST + path
        }
        val u = runCatching { URI(link) }.getOrNull() ?: return null
        if (u.scheme != "https" || u.host != Endpoints.DOWNLOADS_HOST || u.port != -1 || u.rawUserInfo != null || u.rawQuery != null || u.rawFragment != null) return null
        return Release(m.groupValues[1], build, u.toString(), sha, bytes)
    }

    fun parseNews(body: String, release: Release): News? {
        val android = (runCatching { JSONObject(body) }.getOrNull() ?: return null).optJSONObject("android") ?: return null
        val version = android.opt("version") as? String ?: return null
        val code = android.opt("versionCode") as? Number ?: return null
        if (code.toDouble() != release.build.toDouble() || version.trim().removePrefix("v") != release.version) return null
        val text = cleanNews(android.opt("changelog") as? String ?: return null).takeIf { it.isNotEmpty() } ?: return null
        val link = (android.opt("url") as? String)?.takeIf { post(it) }
        return News(text, link)
    }

    private fun cleanNews(raw: String): String {
        val s = raw.replace("\r\n", "\n").replace('\r', '\n').filter { it == '\n' || !it.isISOControl() }.trim()
        return if (s.length > MAX_NEWS) s.take(MAX_NEWS).trimEnd() + "…" else s
    }

    private fun post(link: String): Boolean {
        val u = runCatching { URI(link) }.getOrNull() ?: return false
        return u.scheme == "https" && u.host == "t.me" && u.port == -1 && u.rawUserInfo == null && !u.rawPath.isNullOrEmpty()
    }

    fun compare(a: String, b: String): Int {
        fun parts(v: String) = v.trim().removePrefix("v").takeWhile { it.isDigit() || it == '.' }.split('.').mapNotNull { it.toIntOrNull() }
        val x = parts(a)
        val y = parts(b)
        for (i in 0 until maxOf(x.size, y.size)) {
            val c = x.getOrElse(i) { 0 }.compareTo(y.getOrElse(i) { 0 })
            if (c != 0) return c
        }
        return 0
    }

    fun isNewer(remote: String, remoteBuild: Long, installed: String, installedBuild: Long): Boolean {
        val c = compare(remote, installed)
        return c > 0 || (c == 0 && installedBuild > 0 && remoteBuild > installedBuild)
    }

    private fun installed(ctx: Context): String = runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull().orEmpty()

    private fun installedBuild(ctx: Context): Long = runCatching { androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(ctx.packageManager.getPackageInfo(ctx.packageName, 0)) }.getOrDefault(0L)

    private fun trackOf(ctx: Context): String = track(ctx.packageManager.hasSystemFeature("android.software.leanback"), android.os.Build.VERSION.SDK_INT, android.os.Build.SUPPORTED_ABIS)

    private fun open(ctx: Context, url: String, redirects: Boolean = true): HttpURLConnection {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 20_000
        conn.instanceFollowRedirects = redirects
        conn.setRequestProperty("User-Agent", "ryNet/${installed(ctx)} (Android ${android.os.Build.VERSION.RELEASE})")
        return conn
    }

    private fun failed(code: Int) = java.io.IOException(tr("Сайт ответил $code", "The site responded with $code"))

    private fun fetch(ctx: Context, api: String): String? {
        val conn = open(ctx, api)
        return try {
            withDeadline(conn, CHECK_MS) { d ->
                if (conn.responseCode != 200) return@withDeadline null
                d.wrap(conn.inputStream).use { input ->
                    val out = java.io.ByteArrayOutputStream()
                    val buf = ByteArray(16 * 1024)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        if (out.size() > MAX_API) return@withDeadline null
                    }
                    out.toString("UTF-8")
                }
            }
        } catch (_: Exception) {
            null
        } finally {
            conn.disconnect()
        }
    }

    private fun Release.withNews(ctx: Context): Release {
        news = runCatching { fetch(ctx, RELEASES)?.let { parseNews(it, this) } }.getOrNull()
        return this
    }

    fun check(ctx: Context): Release? {
        val track = trackOf(ctx)
        val api = fetch(ctx, API)?.let { parseDownloads(it, track) }
        if (api != null) return api.takeIf { isNewer(it.version, it.build, installed(ctx), installedBuild(ctx)) }?.withNews(ctx)
        val url = "$BASE${if (track.isEmpty()) "" else "$track/"}latest.apk"
        val conn = open(ctx, url, redirects = false)
        conn.requestMethod = "HEAD"
        try {
            val location = withDeadline(conn, CHECK_MS) {
                if (conn.responseCode !in 301..308) throw failed(conn.responseCode)
                conn.getHeaderField("Location")
            }
            val release = location?.let { parseLocation(it, url) }
                ?: error(tr("Не удалось определить версию на сервере", "Couldn't determine the version on the server"))
            return release.takeIf { isNewer(it.version, it.build, installed(ctx), installedBuild(ctx)) }?.withNews(ctx)
        } finally {
            conn.disconnect()
        }
    }

    fun download(ctx: Context, release: Release, onProgress: (Float) -> Unit): File {
        val dir = File(ctx.cacheDir, "update").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val file = File(dir, "rynet-${release.version}-${release.build}.apk")
        val sha = java.security.MessageDigest.getInstance("SHA-256")
        val conn = open(ctx, release.url)
        try {
            withDeadline(conn, DOWNLOAD_MS) { d ->
                if (conn.responseCode != 200) throw failed(conn.responseCode)
                val total = conn.contentLengthLong.takeIf { it > 0 } ?: release.bytes.takeIf { it > 0 } ?: -1L
                require(total <= MAX_APK) { tr("Файл слишком большой", "File is too large") }
                require(release.bytes <= 0 || conn.contentLengthLong <= 0 || conn.contentLengthLong == release.bytes) { tr("Размер файла не совпал", "The file size doesn't match") }
                var done = 0L
                d.wrap(conn.inputStream).use { input ->
                    file.outputStream().use { out ->
                        val buf = ByteArray(64 * 1024)
                        while (true) {
                            val n = input.read(buf)
                            if (n < 0) break
                            out.write(buf, 0, n)
                            sha.update(buf, 0, n)
                            done += n
                            require(done <= MAX_APK) { tr("Файл слишком большой", "File is too large") }
                            onProgress(if (total > 0) done.toFloat() / total else -1f)
                        }
                    }
                }
            }
        } catch (e: Exception) {
            file.delete()
            throw e
        } finally {
            conn.disconnect()
        }
        if (release.bytes > 0 && file.length() != release.bytes) {
            file.delete()
            error(tr("Размер скачанного файла не совпал", "The downloaded file's size doesn't match"))
        }
        if (!isApk(file)) {
            file.delete()
            error(tr("Скачанный файл не похож на APK", "The downloaded file doesn't look like an APK"))
        }
        if (release.sha256 != null && sha.digest().joinToString("") { "%02x".format(it) } != release.sha256) {
            file.delete()
            error(tr("Контрольная сумма скачанного файла не совпала", "The downloaded file's checksum doesn't match"))
        }
        return file
    }

    fun isApk(file: File): Boolean = file.length() > 1024 && file.inputStream().use { it.read() == 'P'.code && it.read() == 'K'.code }

    fun canInstall(ctx: Context): Boolean = ctx.packageManager.canRequestPackageInstalls()

    fun permissionIntent(ctx: Context): Intent = Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${ctx.packageName}")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    fun install(ctx: Context, file: File) {
        val uri = androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file)
        ctx.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }
}
