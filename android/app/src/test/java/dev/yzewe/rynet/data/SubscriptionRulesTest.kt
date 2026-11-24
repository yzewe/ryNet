package dev.yzewe.rynet.data

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class SubscriptionRulesTest {
    @Test
    fun readsCaseInsensitivePayloadInTheSameOrderAsTheCore() {
        val view = SubscriptionRules.parse(
            """{"name":"RU","routeorder":"proxy-direct-proxy","globalproxy":"false",
            "proxysites":["full:example.org","regexp:.*\\.example"],"directip":["geoip:ru"],
            "blocksites":["geosite:category-ads-all"]}"""
        )!!
        assertEquals("RU", view.name)
        assertEquals(listOf(RuleAction.Proxy, RuleAction.Direct, RuleAction.Block), view.groups.map { it.action })
        assertEquals(listOf("full:example.org", "regexp:.*\\.example"), view.groups.first().domains)
        assertEquals(RuleAction.Direct, view.rest)
        assertEquals(4, view.size)
    }

    @Test
    fun fallbackMatchesCoreAndDoesNotModifyUserSets() {
        assertEquals(RuleAction.Proxy, SubscriptionRules.parse("{}")!!.rest)
        assertEquals(RuleAction.Proxy, SubscriptionRules.parse("""{"GlobalProxy":""}""")!!.rest)
        assertEquals(RuleAction.Direct, SubscriptionRules.parse("""{"GlobalProxy":false}""")!!.rest)
        assertNull(SubscriptionRules.parse("not json"))
        assertFalse(Settings().pingOnStart)
        assertFalse(Settings.fromJson(JSONObject()).pingOnStart)
        assertTrue(Settings.fromJson(Settings(pingOnStart = true).toJson()).pingOnStart)
    }
}
