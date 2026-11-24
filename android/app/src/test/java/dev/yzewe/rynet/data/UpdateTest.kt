package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class UpdateTest {
    private val hash = "a".repeat(64)

    @Test
    fun parsesRealLocations() {
        val main = Update.parseLocation("/releases/$hash/ryNet-1.17.0-177.apk", "https://dl.rynet.app/android/latest.apk")!!
        assertEquals("1.17.0", main.version)
        assertEquals(177L, main.build)
        assertEquals("https://dl.rynet.app/releases/$hash/ryNet-1.17.0-177.apk", main.url)
        assertEquals(hash, main.sha256)
        val legacy = Update.parseLocation("/releases/$hash/ryNet-1.17.0-177-legacy.apk", "https://dl.rynet.app/android/legacy/latest.apk")!!
        assertEquals("1.17.0", legacy.version)
        assertEquals(177L, legacy.build)
        val tv = Update.parseLocation("/releases/$hash/ryNet-1.17.0-177-tv.apk", "https://dl.rynet.app/android/tv/latest.apk")!!
        assertEquals("1.17.0", tv.version)
        assertEquals("https://dl.rynet.app/releases/$hash/ryNet-1.17.0-177-tv.apk", tv.url)
    }

    @Test
    fun absoluteLocationAndNoHash() {
        val r = Update.parseLocation("https://cdn.example/x/ryNet-1.18.2-180.apk", "https://dl.rynet.app/android/latest.apk")!!
        assertEquals("https://cdn.example/x/ryNet-1.18.2-180.apk", r.url)
        assertNull(r.sha256)
        assertNull(Update.parseLocation("/releases/abc/ryNet-1.0.0-1.apk", "https://dl.rynet.app/")!!.sha256)
    }

    @Test
    fun unexpectedLocationGivesNull() {
        assertNull(Update.parseLocation("/releases/$hash/other.apk", "https://dl.rynet.app/android/latest.apk"))
        assertNull(Update.parseLocation("/login", "https://dl.rynet.app/android/latest.apk"))
        assertNull(Update.parseLocation("/ryNet-1.17.0-177.zip", "https://dl.rynet.app/android/latest.apk"))
    }

    private val mainSha = "e66f595f95df682744c805b2852ed66c9b771560cbe91d48389de47a6f426129"
    private val legacySha = "f0e200bb2c706373f879975b6f7829433a108e8868f690222e579fa63e3e66ad"
    private val tvSha = "b".repeat(64)

    private fun item(file: String, sha: String, alias: String, bytes: Long = 4898432, path: String? = "/releases/$sha/$file", url: String = "https://dl.rynet.app$alias") = """{"file":"$file","bytes":$bytes,"sha256":"$sha","source":"https://t.me/rynetforum/4/7803","alias":"$alias",""" +
        (if (path != null) """"path":"$path",""" else "") + """"url":"$url"}"""

    private val real = "[" + listOf(
        item("ryNet-0.12.0.msi", "c".repeat(64), "/windows/latest.msi"),
        item("ryNet-1.17.0-177.apk", mainSha, "/android/latest.apk"),
        item("rynet_0.12.0-1_amd64.deb", "d".repeat(64), "/linux/latest.deb"),
        item("ryNet-1.17.0-177-legacy.apk", legacySha, "/android/legacy/latest.apk", 7176557),
        item("ryNet-1.17.0-177-tv.apk", tvSha, "/android/tv/latest.apk", 7176557)
    ).joinToString(",") + "]"

    @Test
    fun downloadsPickTrackEntry() {
        val m = Update.parseDownloads(real, "")!!
        assertEquals("1.17.0", m.version)
        assertEquals(177L, m.build)
        assertEquals(mainSha, m.sha256)
        assertEquals(4898432L, m.bytes)
        assertEquals("https://dl.rynet.app/releases/$mainSha/ryNet-1.17.0-177.apk", m.url)
        val l = Update.parseDownloads(real, "legacy")!!
        assertEquals(legacySha, l.sha256)
        assertEquals("https://dl.rynet.app/releases/$legacySha/ryNet-1.17.0-177-legacy.apk", l.url)
        val t = Update.parseDownloads(real, "tv")!!
        assertEquals(tvSha, t.sha256)
        assertEquals(7176557L, t.bytes)
        assertEquals("https://dl.rynet.app/releases/$tvSha/ryNet-1.17.0-177-tv.apk", t.url)
    }

    @Test
    fun downloadsWithoutPathUseUrl() {
        val body = "[" + item("ryNet-1.17.0-177.apk", mainSha, "/android/latest.apk", path = null) + "]"
        assertEquals("https://dl.rynet.app/android/latest.apk", Update.parseDownloads(body, "")!!.url)
    }

    @Test
    fun downloadsRejectBadEntries() {
        val f = "ryNet-1.17.0-177.apk"
        val a = "/android/latest.apk"
        fun bad(entry: String, track: String = "") = assertNull(entry, Update.parseDownloads("[$entry]", track))
        bad(item(f, "abc", a))
        bad(item(f, mainSha.dropLast(1) + "z", a))
        bad(item(f, mainSha, a, url = "https://evil.example/android/latest.apk"))
        bad(item(f, mainSha, a, url = "http://dl.rynet.app/android/latest.apk"))
        bad(item(f, mainSha, a, url = "https://dl.rynet.app.evil.example/x"))
        bad(item(f, mainSha, a, path = "/releases/${"0".repeat(64)}/$f"))
        bad(item(f, mainSha, a, path = "/other/$mainSha/$f"))
        bad(item(f, mainSha, a, path = "/releases/$mainSha/$f?x=1"))
        bad(item(f, mainSha, a, path = "/releases/$mainSha/other.apk"))
        bad(item(f, mainSha, a, bytes = 0))
        bad(item(f, mainSha, a, bytes = 10_000_000_000))
        bad(item("ryNet-1.17.0-177-tv.apk", tvSha, a))
        bad(item("ryNet-1.17.0-177.zip", mainSha, a))
        bad(item(f, mainSha, "/android/tv/latest.apk"), "")
        bad(item(f, mainSha, a), "tv")
        assertNull(Update.parseDownloads("[" + item("ryNet-0.12.0.msi", "c".repeat(64), "/windows/latest.msi") + "]", ""))
    }

    @Test
    fun downloadsRejectBadBodies() {
        assertNull(Update.parseDownloads("[]", ""))
        assertNull(Update.parseDownloads("""{"android":null,"pc":null}""", ""))
        assertNull(Update.parseDownloads("<html>502</html>", ""))
        assertNull(Update.parseDownloads("", ""))
        assertNull(Update.parseDownloads("""[1,null,"x",[]]""", ""))
        assertNull(Update.parseDownloads("""[{"alias":"/android/latest.apk","file":null,"sha256":null,"bytes":"1"}]""", ""))
    }

    private val rel = Update.Release("1.17.0", 177, "https://dl.rynet.app/releases/$mainSha/ryNet-1.17.0-177.apk", mainSha, 4898432)
    private val log = "новое:\n• можно вернуть старый дизайн\n• быстрые действия в лаунчере\n\nбатарея:\n• меньше расход памяти VPN\n\nфиксы:\n• gRPC не рвёт все соединения разом"

    private fun releases(
        version: String = "\"1.17.0\"",
        code: String = "177",
        changelog: String = org.json.JSONObject.quote(log),
        url: String = "\"https://t.me/rynetforum/4/7803\""
    ) = """{"android":{"version":$version,"versionCode":$code,"url":$url,"changelog":$changelog,"downloadUrl":"https://dl.rynet.app/releases/$mainSha/ryNet-1.17.0-177.apk"},"pc":{"version":"0.12.0","url":"https://t.me/rynetforum/700/7809","changelog":"x"}}"""

    @Test
    fun newsFromRealResponse() {
        val n = Update.parseNews(releases(), rel)!!
        assertEquals(log, n.text)
        assertTrue(n.text.contains("\n\nбатарея:\n• меньше"))
        assertEquals("https://t.me/rynetforum/4/7803", n.link)
    }

    @Test
    fun newsAbsentOrGarbageGivesNull() {
        assertNull(Update.parseNews("""{"android":null,"pc":null}""", rel))
        assertNull(Update.parseNews("<html>502</html>", rel))
        assertNull(Update.parseNews("", rel))
        assertNull(Update.parseNews("[]", rel))
        assertNull(Update.parseNews("""{"android":"x"}""", rel))
        assertNull(Update.parseNews("""{"android":{}}""", rel))
        assertNull(Update.parseNews(releases(changelog = "null"), rel))
        assertNull(Update.parseNews(releases(changelog = "123"), rel))
        assertNull(Update.parseNews(releases(changelog = "\"  \\n \""), rel))
    }

    @Test
    fun newsNeedsMatchingVersionAndBuild() {
        assertNull(Update.parseNews(releases(version = "\"1.18.0\""), rel))
        assertNull(Update.parseNews(releases(code = "178"), rel))
        assertNull(Update.parseNews(releases(code = "177.5"), rel))
        assertNull(Update.parseNews(releases(code = "\"177\""), rel))
        assertNull(Update.parseNews(releases(code = "null"), rel))
        assertNull(Update.parseNews(releases(version = "117"), rel))
        assertNotNull(Update.parseNews(releases(), Update.Release("1.17.0", 177, "https://dl.rynet.app/x")))
    }

    @Test
    fun newsLinkOnlyTelegramHttps() {
        fun link(u: String) = Update.parseNews(releases(url = org.json.JSONObject.quote(u)), rel)!!.link
        assertEquals("https://t.me/rynetforum/4/7803", link("https://t.me/rynetforum/4/7803"))
        assertNull(link("http://t.me/rynetforum/4/7803"))
        assertNull(link("https://evil.example/rynetforum/4/7803"))
        assertNull(link("https://t.me.evil.example/x"))
        assertNull(link("https://user@t.me/x"))
        assertNull(link("https://t.me:8443/x"))
        assertNull(link("https://t.me"))
        assertNull(link("javascript:alert(1)"))
        assertNull(link("not a url"))
        assertNull(Update.parseNews(releases(url = "null"), rel)!!.link)
        assertNull(Update.parseNews(releases(url = "5"), rel)!!.link)
        assertEquals(log, Update.parseNews(releases(url = "\"http://x\""), rel)!!.text)
    }

    @Test
    fun newsIsTrimmedAndCleaned() {
        val long = "а".repeat(10_000)
        val n = Update.parseNews(releases(changelog = org.json.JSONObject.quote(long)), rel)!!
        assertEquals(4001, n.text.length)
        assertTrue(n.text.endsWith("…"))
        val dirty = "a\u0000b\u001b[31mc\r\nd\te\u0085f"
        assertEquals("ab[31mc\ndef", Update.parseNews(releases(changelog = org.json.JSONObject.quote(dirty)), rel)!!.text)
    }

    @Test
    fun versionsAndBuildsCompare() {
        assertTrue(Update.isNewer("1.18", 1, "1.17.9", 177))
        assertTrue(Update.isNewer("1.17.10", 1, "1.17.9", 177))
        assertFalse(Update.isNewer("1.17.0", 177, "1.17.0-debug", 177))
        assertFalse(Update.isNewer("1.17", 177, "1.17.0", 177))
        assertFalse(Update.isNewer("1.16.9", 999, "1.17.0", 177))
        assertTrue(Update.isNewer("1.17.0", 178, "1.17.0", 177))
        assertFalse(Update.isNewer("1.17.0", 178, "1.17.0", 0))
    }

    @Test
    fun trackSelection() {
        val arm64 = arrayOf("arm64-v8a", "armeabi-v7a")
        assertEquals("", Update.track(false, 34, arm64))
        assertEquals("tv", Update.track(true, 34, arm64))
        assertEquals("tv", Update.track(true, 24, arrayOf("armeabi-v7a")))
        assertEquals("legacy", Update.track(false, 25, arm64))
        assertEquals("legacy", Update.track(false, 34, arrayOf("armeabi-v7a")))
        assertEquals("", Update.track(false, 26, arm64))
    }
}
