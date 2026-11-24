package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class SubEditTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val info = SubInfo("https://sub.example/a", "Панель", 0, 0, 0, 0, 1, null)

    private fun sub(id: String) = Subs.list(ctx).first { it.id == id }

    @Test
    fun customNameOverridesPanelTitleAndSurvivesUpdates() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.rename(ctx, "a", "  Моя  ")
        assertEquals("Моя", sub("a").info.title)
        assertEquals("Моя", sub("a").name)
        assertEquals("Панель", sub("a").panelTitle)

        Subs.import(ctx, Sub("b", info.copy(url = "https://sub.example/b", title = "Другая")), "[]")
        Subs.import(ctx, Sub("a", info.copy(title = "Панель 2")), "[1]")
        assertEquals("Моя", sub("a").info.title)
        assertEquals("Панель 2", sub("a").panelTitle)

        Subs.rename(ctx, "a", "")
        assertEquals("Панель 2", sub("a").info.title)
        assertNull(sub("a").name)
    }

    @Test
    fun editKeepsIdAndOldUrlWhenTheNewOneFails() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.select(ctx, Selection("a", "p"))
        Settings.save(ctx, Settings().pin("a", "p", true).serverDns("a", "p", "1.1.1.1").autoWlServer("a", "p", true))
        assertThrows(Exception::class.java) { Subs.edit(ctx, "a", "Новое", "http://127.0.0.1:9/sub") }
        assertEquals("https://sub.example/a", sub("a").info.url)
        assertEquals("Панель", sub("a").info.title)
        assertNull(sub("a").name)
        assertEquals(Selection("a", "p"), Subs.selection(ctx))
        val s = Settings.load(ctx)
        assertEquals(listOf("p"), s.pinned("a"))
        assertEquals("1.1.1.1", s.perServerDns["a"]?.get("p"))
        assertTrue(s.isAutoWl("a", "p"))
    }

    @Test
    fun editWithTheSameUrlOnlyRenames() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.edit(ctx, "a", "Своя", "sub.example/a/")
        assertEquals("Своя", sub("a").info.title)
        assertEquals("https://sub.example/a", sub("a").info.url)
        assertEquals("[]", Subs.body(ctx, "a"))
    }

    @Test
    fun editRefusesAnotherSubscriptionsUrl() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.import(ctx, Sub("b", info.copy(url = "https://sub.example/b")), "[]")
        val e = assertThrows(IllegalStateException::class.java) { Subs.edit(ctx, "a", null, "https://SUB.example/b") }
        assertTrue(e.message!!.contains("Панель"))
        assertEquals("https://sub.example/a", sub("a").info.url)
    }

    @Test
    fun removeForgetsTheName() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.rename(ctx, "a", "Моя")
        Subs.remove(ctx, "a")
        Subs.import(ctx, Sub("a", info), "[]")
        assertEquals("Панель", sub("a").info.title)
    }

    @Test
    fun urlValidation() {
        assertTrue(Subs.validUrl("sub.example/x"))
        assertTrue(Subs.validUrl("http://1.2.3.4:8080/s"))
        assertFalse(Subs.validUrl(""))
        assertFalse(Subs.validUrl("vless://uuid@host:443"))
        assertFalse(Subs.validUrl("https://"))
    }

    @Test
    fun backupCarriesCustomNameInInfo() {
        Subs.import(ctx, Sub("a", info), "[]")
        Subs.rename(ctx, "a", "Моя")
        val file = Backup.export(ctx, setOf(Backup.Part.Subs), "test")
        val o = Backup.read(file).parts.getValue(Backup.Part.Subs).getJSONArray("list").getJSONObject(0).getJSONObject("info")
        assertEquals("Панель", o.getString("title"))
        assertEquals("Моя", o.getString("customName"))

        Subs.remove(ctx, "a")
        Backup.apply(ctx, Backup.read(file), setOf(Backup.Part.Subs))
        assertEquals("Моя", sub("a").info.title)
        assertEquals("Панель", sub("a").panelTitle)
    }

    @Test
    fun seedColorIsHexAndTravelsWithAppearance() {
        val s = Settings(wallpaperColors = false, seedColor = 0xFF6750A4.toInt())
        assertEquals("#6750a4", s.toJson().getString("seedColor"))
        assertEquals(0xFF6750A4.toInt(), Settings.fromJson(s.toJson()).seedColor)
        assertEquals(0, Settings.fromJson(JSONObject().put("seedColor", "")).seedColor)
        assertEquals("", Settings().toJson().getString("seedColor"))

        Settings.save(ctx, s)
        val file = Backup.export(ctx, setOf(Backup.Part.App), "test")
        Settings.save(ctx, Settings())
        val (_, new) = Backup.apply(ctx, Backup.read(file), setOf(Backup.Part.App))
        assertEquals(0xFF6750A4.toInt(), new.seedColor)
        assertFalse(new.wallpaperColors)
    }
}
