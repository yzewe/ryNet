package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class ModeRulesConfigTest {
    private val body = """[{"remarks":"A","outbounds":[{"tag":"proxy","protocol":"vless"},{"tag":"direct","protocol":"freedom"},{"tag":"ads","protocol":"blackhole"}],
        "routing":{"rules":[{"type":"field","inboundTag":["api"],"outboundTag":"api"},{"domain":["geosite:category-ru","domain:vk.com"],"outboundTag":"direct"},
        {"ip":["geoip:ru"],"outboundTag":"direct"},{"domain":["geosite:category-ads-all"],"outboundTag":"ads"},{"network":"tcp,udp","outboundTag":"proxy"}]}},
        {"remarks":"B","outbounds":[{"tag":"proxy","protocol":"vless"}]}]"""

    @Test fun configRulesMapActionsByOutboundProtocol() {
        val (entries, rest) = ModeRules.fromConfig(body, "A")!!
        assertEquals(4, entries.size)
        assertEquals(RuleAction.Direct, entries.first { it.value == "category-ru" }.action)
        assertEquals(RuleAction.Block, entries.first { it.value == "category-ads-all" }.action)
        assertEquals(RuleAction.Proxy, rest)
    }

    @Test fun serverWithoutOwnRulesLeavesItToTheHappRouting() {
        assertNull(ModeRules.fromConfig(body, "B"))
    }

    @Test fun plainLinksHaveNoConfigRules() {
        assertNull(ModeRules.fromConfig("vless://x@1.2.3.4:443#a", "a"))
    }
}
