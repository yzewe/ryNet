package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class RuleSetsTest {
    @Test
    fun tailscalePresetRoutesBothFamiliesAndNamesThroughTheSelectedServer() {
        val server = Selection("sub", "Германия")
        val set = RuleSets.tailscale(server)
        assertEquals("Tailscale", set.name)
        assertEquals(RuleAction.Server, set.action)
        assertEquals(server, set.server)
        assertEquals(listOf("ts.net"), set.domains)
        assertEquals(listOf("100.64.0.0/10", "fd7a:115c:a1e0::/48"), set.ips)
        val settings = Settings(sets = listOf(set))
        assertEquals(
            listOf(
                "DOMAIN-SUFFIX,ts.net,rynet-app0",
                "IP-CIDR,100.64.0.0/10,rynet-app0,no-resolve",
                "IP-CIDR6,fd7a:115c:a1e0::/48,rynet-app0,no-resolve"
            ),
            settings.effectiveRules().lines()
        )
        assertEquals(settings.sets, Settings.fromJson(settings.toJson()).sets)
    }

    private val work = RuleSet(
        id = "a1", name = "Работа", shape = 2, icon = 3, action = RuleAction.Direct,
        domains = listOf("slack.com", "notion.so"), apps = listOf("com.slack"), ips = listOf("10.0.0.0/8"), geosite = listOf("github")
    )

    @Test
    fun linkCarriesEverythingButServerAndId() {
        val back = RuleSets.parse(RuleSets.link(work.copy(action = RuleAction.Server, server = Selection("s", "p"))))!!
        assertEquals(work.copy(id = back.id, action = RuleAction.Proxy), back)
        assertTrue(back.id != work.id)
    }

    @Test
    fun bigLinkSplitsIntoQrPartsAndComesBackInAnyOrder() {
        val big = work.copy(domains = (1..800).map { "site$it.example${it % 7}.com" })
        val link = RuleSets.link(big)
        val parts = RuleSets.parts(link)
        assertTrue(parts.size > 5 && parts.all { RuleSets.isPart(it) && it.length < 460 })
        assertNull(RuleSets.parse(parts.first()))
        val a = RuleSets.Assembler()
        val order = parts.indices.reversed() + 0
        val got = order.firstNotNullOfOrNull { a.add(parts[it]) }
        assertEquals(link, got)
        assertEquals(big.domains, RuleSets.parse(got!!)!!.domains)
    }

    @Test
    fun partsOfAnotherSetRestartTheAssembly() {
        val one = RuleSets.parts(RuleSets.link(work.copy(domains = (1..300).map { "a$it.com" })))
        val two = RuleSets.parts(RuleSets.link(work.copy(domains = (1..300).map { "b$it.org" })))
        val a = RuleSets.Assembler()
        a.add(one[0])
        assertNull(two.dropLast(1).firstNotNullOfOrNull { a.add(it) })
        assertEquals(two.size - 1, a.count)
    }

    @Test
    fun brokenOrForeignLinksAreRejected() {
        assertNull(RuleSets.parse("rynet://set/!!!"))
        assertNull(RuleSets.parse("https://example.com/set"))
        assertNull(RuleSets.parse(""))
    }

    @Test
    fun junkEntriesAreDropped() {
        val dirty = work.copy(domains = listOf("https://YouTube.com/watch", "not a domain"), apps = listOf("bad pkg"), ips = listOf("999.1.1.1"), geosite = listOf("geosite:GitHub", "x y"))
        val back = RuleSets.parse(RuleSets.link(dirty))!!
        assertEquals(listOf("youtube.com"), back.domains)
        assertEquals(emptyList<String>(), back.apps)
        assertEquals(emptyList<String>(), back.ips)
        assertEquals(listOf("github"), back.geosite)
    }

    @Test
    fun setsGoAfterSingleRulesAndOffSetsAreSkipped() {
        val s = Settings(
            rules = listOf(Rule(RuleKind.Domain, "x.com", RuleAction.Proxy)),
            sets = listOf(work, work.copy(id = "b2", enabled = false, domains = listOf("off.com"))),
            userRules = "MATCH,DIRECT"
        )
        assertEquals(
            listOf(
                "DOMAIN-SUFFIX,x.com,PROXY",
                "DOMAIN-SUFFIX,slack.com,DIRECT",
                "DOMAIN-SUFFIX,notion.so,DIRECT",
                "PROCESS-NAME,com.slack,DIRECT",
                "IP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
                "GEOSITE,github,DIRECT",
                "MATCH,DIRECT"
            ),
            s.effectiveRules().lines()
        )
    }

    @Test
    fun setServersGetTagsAndSurviveSaving() {
        val to = Selection("sub", "Германия")
        val s = Settings(sets = listOf(work.copy(action = RuleAction.Server, server = to)))
        assertEquals(mapOf(to to "rynet-app0"), Rules.servers(s.rules, s.sets))
        assertTrue("DOMAIN-SUFFIX,slack.com,rynet-app0" in s.effectiveRules().lines())
        assertEquals(s.sets, Settings.fromJson(org.json.JSONObject(s.toJson().toString())).sets)
    }
}
