package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val info = SubInfo("https://sub.example/a", "Моя", 0, 0, 0, 0, 1, null)

    @Test
    fun onlyChosenPartsTravel() {
        Settings.save(ctx, Settings(routing = RoutingMode.Smart, fakeDns = true, mtu = 1400, theme = ThemeMode.Dark))
        Subs.import(ctx, Sub("s1", info), "[]")
        Subs.select(ctx, Selection("s1", "p"))
        val file = Backup.export(ctx, setOf(Backup.Part.Routing, Backup.Part.Subs, Backup.Part.Network), "test")
        assertTrue(String(file.copyOfRange(0, 5)) == "RYNET")

        Settings.save(ctx, Settings())
        Subs.remove(ctx, "s1")
        val snap = Backup.read(file)
        assertEquals(setOf(Backup.Part.Routing, Backup.Part.Subs, Backup.Part.Network), snap.parts.keys)
        val locale = java.util.Locale.getDefault()
        try {
            java.util.Locale.setDefault(java.util.Locale.forLanguageTag("ru"))
            assertEquals("1 подписка", snap.summary(Backup.Part.Subs))
            java.util.Locale.setDefault(java.util.Locale.ENGLISH)
            assertEquals("1 subscription", snap.summary(Backup.Part.Subs))
        } finally {
            java.util.Locale.setDefault(locale)
        }
        val (_, new) = Backup.apply(ctx, snap, setOf(Backup.Part.Routing, Backup.Part.Subs))
        assertEquals(RoutingMode.Smart, new.routing)
        assertTrue(new.fakeDns)
        assertEquals(0, new.mtu)
        assertEquals(ThemeMode.System, new.theme)
        assertEquals(listOf("s1"), Subs.list(ctx).map { it.id })
        assertEquals("[]", Subs.body(ctx, "s1"))
        assertEquals(Selection("s1", "p"), Subs.selection(ctx))
    }

    @Test
    fun sameUrlReplacesLocalSubscription() {
        Subs.import(ctx, Sub("local", info), "[1]")
        Subs.import(ctx, Sub("fromFile", info.copy(title = "Из файла")), "[2]")
        assertEquals(listOf("fromFile"), Subs.list(ctx).map { it.id })
    }

    @Test
    fun rejectsForeignFiles() {
        assertThrows(IllegalArgumentException::class.java) { Backup.read("{\"a\":1}".toByteArray()) }
        assertThrows(IllegalArgumentException::class.java) { Backup.read("RYNET\u0001garbage".toByteArray()) }
    }

    @Test fun oldCustomModeMigrates() {
        val o = Settings(routing = RoutingMode.Custom, rest = RuleAction.Direct, userRules = "GEOSITE,x,REJECT").toJson()
        val s = Settings.fromJson(o)
        assertEquals(RoutingMode.Global, s.routing)
        assertEquals("GEOSITE,x,REJECT\nMATCH,DIRECT", s.userRules)
        assertEquals(RoutingMode.Global, Settings.fromJson(Settings(routing = RoutingMode.Custom).toJson()).routing)
    }
}
