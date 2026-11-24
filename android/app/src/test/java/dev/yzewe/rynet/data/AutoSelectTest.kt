package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AutoSelectTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun profile(name: String) = Profile(name, name, "", Group.Main, "VLESS", true, emptyList())

    @Test
    fun whitelistsAreForeignSitesClosedAndHomeOnesOpen() {
        val home = listOf(Probe("ya.ru", 20, true), Probe("vk.com", 30, true))
        assertEquals(NetMode.Whitelist, AutoSelect.classify(home + Probe("google.com", -1, false)))
        assertEquals(NetMode.Free, AutoSelect.classify(home + Probe("google.com", -1, false) + Probe("cloudflare.com", 90, false)))
        assertEquals(NetMode.Offline, AutoSelect.classify(listOf(Probe("ya.ru", -1, true), Probe("google.com", -1, false))))
    }

    @Test
    fun learningFollowsTheServerAddressNotItsName() {
        val a = profile("Москва").copy(server = "84.201.140.7:443", sni = "vk.com")
        assertEquals(AutoSelect.sig(a), AutoSelect.sig(a.copy(remarks = "Переименован", name = "Переименован")))
        assertTrue(AutoSelect.sig(a) != AutoSelect.sig(a.copy(server = "84.201.140.8:443")))
    }

    @Test
    fun udpProtocolsAreSpotted() {
        assertTrue(AutoSelect.isUdp(profile("x").copy(kind = "Hysteria 2 · salamander")))
        assertTrue(AutoSelect.isUdp(profile("x").copy(kind = "TUIC")))
        assertFalse(AutoSelect.isUdp(profile("x").copy(kind = "REALITY · gRPC")))
    }

    @Test
    fun transportsComeFromTheKind() {
        val of = { kind: String -> AutoSelect.transport(profile("x").copy(kind = kind)) }
        assertEquals("XHTTP", of("REALITY · XHTTP"))
        assertEquals("gRPC", of("Trojan · TLS · gRPC"))
        assertEquals("WebSocket", of("VMess · TLS · WebSocket"))
        assertEquals("Hysteria", of("Hysteria 2 · salamander"))
        assertEquals("TCP", of("REALITY"))
        assertEquals("TCP", of("Shadowsocks · 2022-blake3-aes-128-gcm"))
        assertTrue(AutoSelect.TRANSPORTS.containsAll(listOf(of("TLS · HTTPUpgrade"), of("VLESS"))))
    }

    @Test
    fun preferredOnesWinOnlyWhenThereAreAny() {
        assertEquals(listOf(2, 4), AutoSelect.preferring(listOf(1, 2, 3, 4)) { it % 2 == 0 })
        assertEquals(listOf(1, 3), AutoSelect.preferring(listOf(1, 3)) { it % 2 == 0 })
    }

    @Test
    fun hostListsSplitOnCommasAndLines() {
        assertEquals(listOf("ya.ru", "vk.com", "https://x.example/p"), AutoSelect.hosts(" ya.ru,vk.com\nhttps://x.example/p ; "))
    }

    @Test
    fun stateSurvivesStorage() {
        val st = AutoState(
            at = 5,
            mode = NetMode.Whitelist,
            network = "Wi-Fi",
            probes = listOf(Probe("ya.ru", 12, true)),
            candidates = listOf(Candidate("s", "p", "P", "", "Sub", 18, 18, whitelist = true, learned = false, pool = true)),
            total = 3,
            chosen = Selection("s", "p"),
            next = 9
        )
        AutoSelect.publish(ctx, st)
        assertEquals(st, AutoSelect.load(ctx))
    }

    @Test
    fun settingsKeepAutoChoices() {
        val s = Settings(
            autoSelect = true,
            autoSkipSubs = listOf("a"),
            autoWhitelist = WhitelistMode.Always,
            autoInterval = 0,
            autoMargin = 100,
            autoHome = "yandex.ru",
            autoSaveData = true,
            autoTransports = listOf("XHTTP", "gRPC")
        ).autoSkip("b", "Резерв", true)
        val back = Settings.fromJson(s.toJson())
        assertEquals(s, back)
        assertEquals(listOf("Резерв"), back.autoSkipped("b"))
        assertTrue(back.autoSkip("b", "Резерв", false).autoSkip.isEmpty())
    }

    @Test
    fun oldDefaultHomeSitesBecomeTheCurrentOnes() {
        for (old in listOf("ya.ru, gosuslugi.ru, vk.com", "yandex.ru, vk.com, max.ru, gosuslugi.ru", "yandex.ru, vk.ru, vk.com, max.ru, gosuslugi.ru")) {
            assertEquals(Settings.AUTO_HOME, Settings.fromJson(Settings().toJson().put("autoHome", old)).autoHome)
        }
        assertEquals("vk.com", Settings.fromJson(Settings().toJson().put("autoHome", "vk.com")).autoHome)
        assertTrue(AutoSelect.hosts(Settings.AUTO_HOME).all { it.endsWith(".ru") })
        assertTrue("vk.ru" in AutoSelect.hosts(Settings.AUTO_HOME))
    }

    @Test
    fun squeezedIsOnlyTheTooSlowPage() {
        assertEquals(AutoSelect.Carry.Ok, AutoSelect.carryOf(840))
        assertEquals(AutoSelect.Carry.Ok, AutoSelect.carryOf(0))
        assertEquals(AutoSelect.Carry.Slow, AutoSelect.carryOf("Too slow: data barely gets through"))
        assertEquals(AutoSelect.Carry.Slow, AutoSelect.carryOf("Слишком медленно: данные почти не проходят"))
        for (other in listOf("Timeout: the server did not answer in time", "Unexpected server reply: the check failed", "Ошибка: body ended at 9000 bytes", -1, null)) {
            assertEquals(AutoSelect.Carry.Unknown, AutoSelect.carryOf(other))
        }
    }

    @Test
    fun balancerChoicesMoveOntoItsServers() {
        val sep = Profile.MEMBER_SEP
        val sub = Sub("s", SubInfo("https://x", null, 0, 0, 0, 0, 0, null))
        val groups = listOf(sub to listOf(profile("Балансер${sep}a"), profile("Балансер${sep}b"), profile("Один")))
        val old = Settings(
            autoSkip = mapOf("s" to listOf("Балансер"), "t" to listOf("Балансер")),
            autoWl = mapOf("s" to listOf("Балансер", "Один"))
        )
        val now = AutoSelect.withMembers(old, groups)
        assertEquals(listOf("Балансер${sep}a", "Балансер${sep}b"), now.autoSkipped("s"))
        assertEquals(listOf("Балансер"), now.autoSkipped("t"))
        assertTrue(now.isAutoWl("s", "Балансер${sep}b") && now.isAutoWl("s", "Один"))
        assertEquals(now, AutoSelect.withMembers(now, groups))
        assertEquals(Settings(), AutoSelect.withMembers(Settings(), groups))
        assertEquals("Балансер", Profile.parentOf("Балансер${sep}a"))
        assertEquals(null, Profile.parentOf("Один"))
    }

    @Test
    fun whitelistServersAreToldByName() {
        for (n in listOf("Обход 2 | LTE", "🇷🇺 LTE", "Москва (БС)", "Б/С Мегафон", "WL-1", "Белые списки", "для белых списков", "Мобильный интернет", "whitelist")) {
            assertTrue(n, AutoSelect.namedForWhitelists(n))
        }
        for (n in listOf("Германия", "Обход блокировок", "Нидерланды 2", "Bslte", "Сбс", "Albania", "Helsinki")) {
            assertFalse(n, AutoSelect.namedForWhitelists(n))
        }
        assertTrue(AutoSelect.namedForWhitelists("Москва", "Обход БС"))
        assertTrue(AutoSelect.namedForWhitelists("Балансер LTE${Profile.MEMBER_SEP}a"))
    }

    @Test
    fun reserveServersOnlyOnWhitelistsWhenAsked() {
        val named = profile("LTE 1")
        val plain = profile("Германия")
        val off = Settings()
        val on = Settings(reserveOnlyOnWhitelists = true)
        assertFalse(off.reserveOnlyOnWhitelists)
        assertFalse(AutoSelect.reserve(on, "s", named))
        assertFalse(AutoSelect.reserve(on, "s", plain))
        val marked = Settings(autoWl = mapOf("s" to listOf("Германия")), reserveOnlyOnWhitelists = true)
        assertTrue(AutoSelect.reserve(marked, "s", plain))
        assertFalse(AutoSelect.reserve(marked, "s", named))
        for (mode in NetMode.values()) assertEquals(mode != NetMode.Whitelist, AutoSelect.heldBack(off, mode, true))
        assertTrue(AutoSelect.heldBack(on, NetMode.Free, true))
        assertTrue(AutoSelect.heldBack(on, NetMode.Offline, true))
        assertFalse(AutoSelect.heldBack(on, NetMode.Whitelist, true))
        assertFalse(AutoSelect.heldBack(on, NetMode.Free, false))
        assertTrue(Settings.fromJson(on.toJson()).reserveOnlyOnWhitelists)
    }

    @Test
    fun switchingNeedsAClearGainSeenTwice() {
        assertFalse("50 ms margin is not enough any more", AutoSelect.switches(140, 200, 50, 3, 3))
        assertFalse("30% of a slow server", AutoSelect.switches(500, 650, 50, 3, 3))
        assertTrue(AutoSelect.switches(400, 650, 50, 3, 3))
        assertTrue(AutoSelect.switches(100, 200, 50, 2, 2))
        assertFalse("one sample of the new one", AutoSelect.switches(100, 200, 50, 1, 3))
        assertFalse("one sample of the current one", AutoSelect.switches(100, 200, 50, 3, 1))
        assertFalse("the user's margin still counts", AutoSelect.switches(100, 200, 150, 3, 3))
    }

    @Test
    fun autoPingsLandInTheSubscriptionWithoutTouchingTheShownOne() {
        Pings.publish(ctx, Pings("shown", mapOf("x" to 40)))
        Pings.merge(ctx, "other", mapOf("y" to 70))
        assertEquals(mapOf("x" to 40), Pings.load(ctx).results)
        assertEquals(70, Pings.forSub(ctx, "other").results["y"])
        Pings.merge(ctx, "shown", mapOf("z" to 25))
        assertEquals(mapOf("x" to 40, "z" to 25), Pings.load(ctx).results)
    }

    @Test
    fun removedSubscriptionLeavesNoWhitelistMarks() {
        val s = Settings(
            autoWlSubs = listOf("gone", "kept"),
            autoWl = mapOf("gone" to listOf("A"), "kept" to listOf("B")),
            autoSkipSubs = listOf("gone"),
            autoSkip = mapOf("gone" to listOf("C"))
        )
        val kept = s.onlySubs(setOf("kept"))
        assertEquals(listOf("kept"), kept.autoWlSubs)
        assertEquals(mapOf("kept" to listOf("B")), kept.autoWl)
        assertTrue(kept.autoSkipSubs.isEmpty() && kept.autoSkip.isEmpty())
        assertFalse(s.onlySubs(emptySet()).autoWlMarked)
    }
}
