package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class RuleCoverTest {
    private val smart = ModeRules.fromXray(ModeRules.FALLBACK).map { CoverSource(it, "smart") }

    private fun src(kind: ModeEntry.Kind, v: String, a: RuleAction = RuleAction.Direct, user: Boolean = false) = CoverSource(ModeEntry(kind, v, a), "x", user)

    @Test fun smartListIsParsedFromTheCoreJson() {
        val e = smart.map { it.entry }
        assertTrue(ModeEntry(ModeEntry.Kind.GeoSite, "category-ru", RuleAction.Direct) in e)
        assertTrue(ModeEntry(ModeEntry.Kind.GeoIp, "ru", RuleAction.Direct) in e)
        assertTrue(ModeEntry(ModeEntry.Kind.GeoIp, "private", RuleAction.Direct) in e)
        assertTrue(ModeEntry(ModeEntry.Kind.Suffix, "xn--p1ai", RuleAction.Direct) in e)
        assertTrue(ModeEntry(ModeEntry.Kind.Protocol, "bittorrent", RuleAction.Direct) in e)
        assertEquals(9, ModeRules.privateNets().size)
    }

    @Test fun domainIsCoveredBySuffixAndExactRules() {
        assertEquals("ru", RuleCover.find(RuleKind.Domain, "mail.yandex.ru", smart)?.entry?.value)
        assertEquals("ru", RuleCover.find(RuleKind.Domain, "yandex.ru", smart)?.entry?.value)
        assertNull(RuleCover.find(RuleKind.Domain, "yandex.com.tr", smart))
        assertNull(RuleCover.find(RuleKind.Domain, "notru", smart))
        val s = listOf(src(ModeEntry.Kind.Suffix, "yandex.ru"), src(ModeEntry.Kind.Full, "a.com"), src(ModeEntry.Kind.Keyword, "ads"))
        assertNotNull(RuleCover.find(RuleKind.Domain, "yandex.ru", s))
        assertNull(RuleCover.find(RuleKind.Domain, "ndex.ru", s))
        assertNotNull(RuleCover.find(RuleKind.Domain, "a.com", s))
        assertNull(RuleCover.find(RuleKind.Domain, "b.a.com", s))
        assertNotNull(RuleCover.find(RuleKind.Domain, "x.ads.net", s))
    }

    @Test fun firstApplicableRuleIsReturned() {
        val s = listOf(src(ModeEntry.Kind.Suffix, "ru", RuleAction.Block), src(ModeEntry.Kind.Suffix, "yandex.ru"))
        assertEquals(RuleAction.Block, RuleCover.find(RuleKind.Domain, "yandex.ru", s)?.entry?.action)
    }

    @Test fun networksAreCoveredByWiderNetworks() {
        assertNotNull(RuleCover.find(RuleKind.Ip, "10.1.2.3", listOf(src(ModeEntry.Kind.Cidr, "10.0.0.0/8"))))
        assertNotNull(RuleCover.find(RuleKind.Ip, "10.0.0.0/16", listOf(src(ModeEntry.Kind.Cidr, "10.0.0.0/8"))))
        assertNull(RuleCover.find(RuleKind.Ip, "10.0.0.0/7", listOf(src(ModeEntry.Kind.Cidr, "10.0.0.0/8"))))
        assertNull(RuleCover.find(RuleKind.Ip, "11.0.0.1", listOf(src(ModeEntry.Kind.Cidr, "10.0.0.0/8"))))
        assertNotNull(RuleCover.find(RuleKind.Ip, "fd7a:115c:a1e0::1", listOf(src(ModeEntry.Kind.Cidr, "fc00::/7"))))
        assertNull(RuleCover.find(RuleKind.Ip, "10.0.0.1", listOf(src(ModeEntry.Kind.Cidr, "fc00::/7"))))
    }

    @Test fun geoCategoriesAreUsedOnlyWhenTheListIsThere() {
        val geo = GeoHas { kind, name, v ->
            if (name == "missing") null else kind == ModeEntry.Kind.GeoSite && name == "ru" && v == "a.ru"
        }
        assertNotNull(RuleCover.find(RuleKind.Domain, "a.ru", listOf(src(ModeEntry.Kind.GeoSite, "ru")), geo))
        assertNull(RuleCover.find(RuleKind.Domain, "b.ru", listOf(src(ModeEntry.Kind.GeoSite, "ru")), geo))
        assertNull(RuleCover.find(RuleKind.Domain, "a.ru", listOf(src(ModeEntry.Kind.GeoSite, "missing")), geo))
        assertNull(RuleCover.find(RuleKind.Domain, "a.ru", listOf(src(ModeEntry.Kind.GeoSite, "ru"))))
    }

    @Test fun appsMatchByPackage() {
        val s = listOf(src(ModeEntry.Kind.App, "org.app.one"))
        assertNotNull(RuleCover.find(RuleKind.App, "org.app.one", s))
        assertNull(RuleCover.find(RuleKind.App, "org.app.two", s))
    }

    @Test fun sourcesFollowTheModeAndSkipDisabledSets() {
        val set = RuleSet("a", name = "Mine", action = RuleAction.Proxy, domains = listOf("x.com"))
        val off = RuleSet("b", enabled = false, domains = listOf("y.com"))
        val st = Settings(routing = RoutingMode.Smart, sets = listOf(set, off), rules = listOf(Rule(RuleKind.Ip, "1.2.3.0/24", RuleAction.Block)))
        val all = RuleCover.sources(st, ModeRules.fromXray(ModeRules.FALLBACK), emptyList())
        assertNull(RuleCover.find(RuleKind.Domain, "y.com", all))
        assertEquals(RuleAction.Block, RuleCover.find(RuleKind.Ip, "1.2.3.4", all)?.entry?.action)
        assertTrue(RuleCover.find(RuleKind.Domain, "x.com", all)!!.user)
        assertEquals(false, RuleCover.find(RuleKind.Domain, "a.ru", all)!!.user)
        val global = RuleCover.sources(st.copy(routing = RoutingMode.Global), ModeRules.fromXray(ModeRules.FALLBACK), emptyList())
        assertNull(RuleCover.find(RuleKind.Domain, "a.ru", global))
    }

    @Test fun subscriptionRulesCountOnlyWithoutOwnRules() {
        val sub = ModeRules.subscription(SubscriptionRules.parse("""{"DirectSites":["domain:video.example","geosite:x"],"DirectIp":["geoip:ru"]}"""))
        assertEquals(3, sub.size)
        val st = Settings(routing = RoutingMode.Subscription)
        assertNotNull(RuleCover.find(RuleKind.Domain, "a.video.example", RuleCover.sources(st, emptyList(), sub)))
        val own = st.copy(rules = listOf(Rule(RuleKind.App, "org.app.one", RuleAction.Proxy)))
        assertNull(RuleCover.find(RuleKind.Domain, "a.video.example", RuleCover.sources(own, emptyList(), sub)))
    }
}
