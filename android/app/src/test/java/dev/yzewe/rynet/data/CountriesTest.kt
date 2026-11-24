package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class CountriesTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun p(remarks: String, flag: String, server: String = "$remarks.example:443", members: List<Profile> = emptyList()) = Profile(remarks, remarks, flag, Group.Main, "vless", true, emptyList(), server = server, members = members)

    @Test
    fun knownCountryReplacesTheNameFlagUntilItExpires() {
        val now = 1_000_000_000L
        val ep = mapOf("de" to "de.example:443", "fr" to "fr.example:443")
        Countries.put(ctx, "a", mapOf("de" to "nl"), ep, now)
        assertEquals(mapOf("de" to "NL"), Countries.load(ctx, "a", ep, now + 1000))
        assertEquals(emptyMap<String, String>(), Countries.load(ctx, "b", ep, now))
        assertEquals(emptyMap<String, String>(), Countries.load(ctx, "a", ep, now + Countries.TTL_MS))

        Countries.put(ctx, "a", mapOf("fr" to "FR"), ep, now + Countries.TTL_MS)
        assertEquals(mapOf("fr" to "FR"), Countries.load(ctx, "a", ep, now + Countries.TTL_MS))
        Countries.forget(ctx, "a")
        assertEquals(emptyMap<String, String>(), Countries.load(ctx, "a", ep, now + Countries.TTL_MS))
    }

    @Test
    fun aServerThatMovedToAnotherAddressLosesItsCountry() {
        Countries.put(ctx, "m", mapOf("de" to "US"), mapOf("de" to "old.example:443"))
        assertEquals(listOf("🇩🇪"), Countries.flags(ctx, "m", listOf(p("de", "🇩🇪", server = "new.example:443"))).map { it.flag })
        assertEquals(listOf("🇺🇸"), Countries.flags(ctx, "m", listOf(p("de", "🇩🇪", server = "old.example:443"))).map { it.flag })
    }

    @Test
    fun flagsComeFromTheCacheForServersAndBalancerMembers() {
        val member = "bal${Profile.MEMBER_SEP}x"
        val list = listOf(p("de", "🇩🇪"), p("fr", "🇫🇷"), p("bal", "🇱🇹", members = listOf(p(member, "🇱🇹"))))
        Countries.put(ctx, "s", mapOf("de" to "NL", member to "FI"), Countries.endpoints(list))
        val out = Countries.flags(ctx, "s", list)
        assertEquals(listOf("🇳🇱", "🇫🇷", "🇱🇹"), out.map { it.flag })
        assertEquals("🇫🇮", out[2].members.single().flag)

        Countries.clear(ctx)
        assertEquals(listOf("🇩🇪", "🇫🇷", "🇱🇹"), Countries.flags(ctx, "s", list).map { it.flag })
    }
}
