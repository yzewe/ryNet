package dev.yzewe.rynet.vpn

import dev.yzewe.rynet.data.Rule
import dev.yzewe.rynet.data.RuleAction
import dev.yzewe.rynet.data.RuleKind
import dev.yzewe.rynet.data.RuleSet
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.TunRoutes
import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TunRoutesTest {
    private val server = Selection("test", "server")
    private fun includes(routes: List<Pair<String, Int>>, address: String): Boolean {
        val ip = InetAddress.getByName(address).address
        return routes.any { (base, length) ->
            val prefix = InetAddress.getByName(base).address
            ip.size == prefix.size && (0 until length).all { bit ->
                (ip[bit / 8].toInt() shr (7 - bit % 8) and 1) == (prefix[bit / 8].toInt() shr (7 - bit % 8) and 1)
            }
        }
    }

    @Test fun tailnetRoutesStayInTunOnModernAndLegacyAndroid() {
        val routes = TunRoutes.of(
            Settings(
                sets = listOf(
                    RuleSet(
                        "tailscale",
                        action = RuleAction.Server,
                        server = server,
                        ips = listOf("100.64.0.0/10", "fd7a:115c:a1e0::/48")
                    )
                )
            )
        )
        assertFalse(includes(routes.excluded, "100.100.100.100"))
        assertTrue(includes(RynetService.routesExcept(routes.excluded), "100.100.100.100"))
        assertTrue(includes(routes.ipv6, "fd7a:115c:a1e0::1234"))
        assertFalse(includes(routes.ipv6, "2606:4700:4700::1111"))
        assertFalse(includes(routes.ipv6, "fd7a:115c:a1e1::1"))
    }

    @Test fun explicitLanServerRuleCarvesOnlyItsNetworkOutOfExclusions() {
        val routes = TunRoutes.of(Settings(rules = listOf(Rule(RuleKind.Ip, "192.168.20.0/24", RuleAction.Server, server))))
        assertFalse(includes(routes.excluded, "192.168.20.42"))
        assertTrue(includes(routes.excluded, "192.168.21.42"))
        assertTrue(includes(RynetService.routesExcept(routes.excluded), "192.168.20.42"))
        assertFalse(includes(RynetService.routesExcept(routes.excluded), "192.168.21.42"))
    }

    @Test fun rawProxyAndRejectRulesAlsoOverrideExclusions() {
        val routes = TunRoutes.of(Settings(userRules = "IP-CIDR,10.1.0.0/16,PROXY\nIP-CIDR,172.16.10.0/24,REJECT\nIP-CIDR,192.168.0.0/16,DIRECT"))
        assertFalse(includes(routes.excluded, "10.1.2.3"))
        assertFalse(includes(routes.excluded, "172.16.10.2"))
        assertTrue(includes(routes.excluded, "10.2.2.3"))
        assertTrue(includes(routes.excluded, "192.168.1.1"))
    }

    @Test fun disabledSetDoesNotEnableIpv6OrChangeRoutes() {
        val set = RuleSet(
            "tailscale",
            enabled = false,
            action = RuleAction.Server,
            server = server,
            ips = listOf("192.168.0.0/16", "fd7a:115c:a1e0::/48")
        )
        assertEquals(TunRoutes.of(Settings()), TunRoutes.of(Settings(sets = listOf(set))))
        assertNotEquals(TunRoutes.of(Settings()), TunRoutes.of(Settings(sets = listOf(set.copy(enabled = true)))))
    }

    @Test fun restOfIpv6IsRejectedOnceAnIpv6RuleTurnsTheFamilyOn() {
        val plain = "DOMAIN-SUFFIX,example.com,PROXY\nIP-CIDR,10.1.0.0/16,PROXY\nIP-CIDR6,fd7a:115c:a1e0::/48,DIRECT"
        assertEquals(plain, TunRoutes.guarded(plain))
        assertEquals("", TunRoutes.guarded(""))
        val v6 = "IP-CIDR6,fd7a:115c:a1e0::/48,PROXY,no-resolve\nDOMAIN-SUFFIX,example.com,DIRECT"
        assertEquals("$v6\n${TunRoutes.V6_REST}", TunRoutes.guarded(v6))
        assertEquals("$v6\n${TunRoutes.V6_REST}\nMATCH,DIRECT\nDOMAIN,late.test,PROXY", TunRoutes.guarded("$v6\nMATCH,DIRECT\nDOMAIN,late.test,PROXY"))
        assertEquals("$v6\n${TunRoutes.V6_REST}\n- match,PROXY", TunRoutes.guarded("$v6\n- match,PROXY"))
        val set = Settings(sets = listOf(RuleSet("tailscale", action = RuleAction.Server, server = server, ips = listOf("fd7a:115c:a1e0::/48"))))
        assertTrue(TunRoutes.guarded(set.effectiveRules()).lines().last() == TunRoutes.V6_REST)
    }

    @Test fun directIpv6RuleDoesNotEnableIpv6AndMalformedRawRulesAreIgnored() {
        assertEquals(TunRoutes.of(""), TunRoutes.of("IP-CIDR6,fd7a:115c:a1e0::/48,DIRECT\nIP-CIDR,invalid,PROXY\nIP-CIDR,10.0.0.0/8,"))
    }
}
