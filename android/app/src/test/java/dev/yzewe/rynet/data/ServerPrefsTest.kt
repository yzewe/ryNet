package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ServerPrefsTest {
    @Test fun pinsKeepOrderAndSurviveJson() {
        val s = Settings().pin("a", "Финляндия", true).pin("a", "Швеция", true).pin("b", "США", true).pin("a", "Финляндия", false).pin("a", "Финляндия", true)
        assertEquals(listOf("Швеция", "Финляндия"), s.pinned("a"))
        val back = Settings.fromJson(JSONObject(s.toJson().toString()))
        assertEquals(s.pins, back.pins)
        assertTrue(back.pin("b", "США", false).pins.keys == setOf("a"))
    }

    @Test fun serverDnsOverridesOwnDnsOnlyForThatServer() {
        val s = Settings(dnsServers = "1.1.1.1").serverDns("a", "Финляндия", "https://8.8.8.8/dns-query")
        assertEquals(listOf("https://8.8.8.8/dns-query"), s.dnsFor(Selection("a", "Финляндия")))
        assertEquals(listOf("1.1.1.1"), s.dnsFor(Selection("a", "Швеция")))
        assertEquals(listOf("1.1.1.1"), s.dnsFor(null))
        val back = Settings.fromJson(JSONObject(s.toJson().toString()))
        assertEquals(s.perServerDns, back.perServerDns)
        assertTrue(s.tunnelDiffers(s.serverDns("a", "Финляндия", "")))
        assertTrue(s.serverDns("a", "Финляндия", "").perServerDns.isEmpty())
    }

    @Test fun balancerDnsHoldsForItsServers() {
        val sep = Profile.MEMBER_SEP
        val s = Settings(dnsServers = "1.1.1.1").serverDns("a", "Балансер", "8.8.8.8").serverDns("a", "Балансер${sep}b", "9.9.9.9")
        assertEquals(listOf("8.8.8.8"), s.dnsFor(Selection("a", "Балансер${sep}a")))
        assertEquals(listOf("9.9.9.9"), s.dnsFor(Selection("a", "Балансер${sep}b")))
        assertEquals(listOf("1.1.1.1"), s.dnsFor(Selection("a", "Другой${sep}a")))
        assertEquals(listOf("1.1.1.1"), s.dnsFor(Selection("b", "Балансер${sep}a")))
    }

    @Test fun balancerServerHasItsOwnPinAndDns() {
        val sep = Profile.MEMBER_SEP
        val member = Profile("Литва${sep}proxy-2", "proxy-2", "🇱🇹", Group.Main, "REALITY", true, emptyList())
        val balancer = Profile("Литва", "Литва", "🇱🇹", Group.Main, "Балансер", true, emptyList(), members = listOf(member, member.copy(remarks = "Литва${sep}proxy")))
        val solo = Profile("Финляндия", "Финляндия", "🇫🇮", Group.Main, "REALITY", true, emptyList())
        val s = Settings(dnsServers = "1.1.1.1").pin("a", "Финляндия", true).pin("a", member.remarks, true).pin("a", "Нет${sep}x", true).serverDns("a", member.remarks, "9.9.9.9")
        val pinned = listOf(solo, balancer).pinnedBy(s.pinned("a"))
        assertEquals(listOf("Финляндия", member.remarks), pinned.map { it.remarks })
        assertEquals("Литва [proxy-2]", pinned[1].name)
        assertEquals(listOf("9.9.9.9"), s.dnsFor(Selection("a", member.remarks)))
        assertEquals(listOf("1.1.1.1"), s.dnsFor(Selection("a", "Литва${sep}proxy")))
        val back = Settings.fromJson(JSONObject(s.toJson().toString()))
        assertEquals(s.pins, back.pins)
        assertEquals(s.perServerDns, back.perServerDns)
        assertEquals(emptyList<Profile>(), listOf(solo).pinnedBy(Settings().pinned("a")))
    }

    @Test fun lastServerIsRememberedPerSubscription() {
        val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()
        Subs.select(ctx, Selection("a", "Финляндия"))
        Subs.select(ctx, Selection("b", "США"))
        assertEquals("Финляндия", Subs.last(ctx, "a"))
        assertEquals("США", Subs.last(ctx, "b"))
        Subs.remove(ctx, "a")
        assertNull(Subs.last(ctx, "a"))
        assertEquals("США", Subs.last(ctx, "b"))
    }
}
