package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RulesTest {
    @Test fun domainsAreNormalised() {
        assertEquals("youtube.com", Rules.domain("https://www.YouTube.com/watch?v=1")?.removePrefix("www."))
        assertEquals("x.com", Rules.domain("*.x.com"))
        assertEquals("x.com", Rules.domain("+.x.com"))
        assertNull(Rules.domain("not a domain"))
    }

    @Test fun cidrs() {
        assertEquals("1.2.3.4/32" to false, Rules.cidr("1.2.3.4"))
        assertEquals("10.0.0.0/8" to false, Rules.cidr("10.0.0.0/8"))
        assertEquals("2001:db8::/32" to true, Rules.cidr("2001:db8::/32"))
        assertNull(Rules.cidr("300.1.1.1"))
        assertNull(Rules.cidr("10.0.0.0/33"))
    }

    @Test fun mihomoText() {
        val text = Rules.mihomo(
            listOf(
                Rule(RuleKind.Domain, "youtube.com", RuleAction.Proxy),
                Rule(RuleKind.Ip, "192.168.0.0/16", RuleAction.Direct),
                Rule(RuleKind.App, "org.telegram.messenger", RuleAction.Block),
                Rule(RuleKind.App, "bad package", RuleAction.Proxy)
            ),
            "GEOSITE,category-ads-all,REJECT\n\n"
        )
        assertEquals(
            listOf(
                "DOMAIN-SUFFIX,youtube.com,PROXY",
                "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve",
                "PROCESS-NAME,org.telegram.messenger,REJECT",
                "GEOSITE,category-ads-all,REJECT"
            ).joinToString("\n"),
            text
        )
    }

    @Test fun appsOnlyModeKeepsRoutingForProxiedApps() {
        val browser = Rule(RuleKind.App, "com.android.chrome", RuleAction.Proxy)
        val blocked = Rule(RuleKind.App, "com.ads", RuleAction.Block)
        val ru = Rule(RuleKind.Domain, "ya.ru", RuleAction.Direct)
        val set = RuleSet(id = "s", action = RuleAction.Proxy, apps = listOf("org.telegram.messenger"), domains = listOf("t.me"))
        val s = Settings(rules = listOf(browser, blocked, ru), sets = listOf(set), vpnAppsOnly = true)
        assertEquals(listOf("PROCESS-NAME,com.ads,REJECT", "DOMAIN-SUFFIX,ya.ru,DIRECT", "DOMAIN-SUFFIX,t.me,PROXY"), s.effectiveRules().lines())
        assertEquals(listOf("com.android.chrome", "com.ads", "org.telegram.messenger").sorted(), s.vpnOnlyApps())
        assertEquals("PROCESS-NAME,com.android.chrome,PROXY", s.copy(vpnAppsOnly = false).effectiveRules().lines().first())
    }

    @Test fun ownRulesInEveryMode() {
        val s = Settings(rules = listOf(Rule(RuleKind.Domain, "a.test", RuleAction.Block)))
        for (m in RoutingMode.selectable) assertEquals("DOMAIN-SUFFIX,a.test,REJECT", s.copy(routing = m).effectiveRules())
    }

    @Test
    fun directAppRuleLeavesVpnRegardlessOfRuleOrder() {
        val s = Settings(
            routing = RoutingMode.Custom,
            rules = listOf(
                Rule(RuleKind.App, "com.first", RuleAction.Direct),
                Rule(RuleKind.Domain, "example.com", RuleAction.Proxy),
                Rule(RuleKind.App, "com.later", RuleAction.Direct)
            )
        )
        org.junit.Assert.assertEquals(setOf("com.first", "com.later"), s.directApps())
    }

    @Test
    fun otherRulesAboveDoNotKeepAnAppInside() {
        val s = Settings(
            routing = RoutingMode.Custom,
            rules = listOf(
                Rule(RuleKind.Domain, "ru.test", RuleAction.Direct),
                Rule(RuleKind.Ip, "10.0.0.0/8", RuleAction.Direct),
                Rule(RuleKind.App, "com.bank", RuleAction.Direct),
                Rule(RuleKind.Domain, "ads.test", RuleAction.Block),
                Rule(RuleKind.App, "com.later", RuleAction.Direct)
            )
        )
        org.junit.Assert.assertEquals(setOf("com.bank", "com.later"), s.directApps())
    }
}
