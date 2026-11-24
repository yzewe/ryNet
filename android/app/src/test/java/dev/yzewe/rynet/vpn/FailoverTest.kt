package dev.yzewe.rynet.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class FailoverTest {
    private val pings = mapOf("wl-fast" to 20, "lte" to 30, "de" to 80, "nl" to 50, "fi" to -1)

    @Test
    fun ordinaryServersFirstWhitelistOnesOnlyAfter() {
        val rounds = RynetService.failoverRounds(listOf("de", "wl-fast", "nl", "lte", "fi"), pings, setOf("wl-fast", "lte"), 6)
        assertEquals(listOf(listOf("nl", "de", "fi"), listOf("wl-fast", "lte")), rounds)
    }

    @Test
    fun underWhitelistsItIsPingOrderOnly() {
        val rounds = RynetService.failoverRounds(listOf("de", "wl-fast", "nl", "lte"), pings, emptySet(), 3)
        assertEquals(listOf(listOf("wl-fast", "lte", "nl")), rounds)
    }

    @Test
    fun onlyWhitelistServersLeft() {
        assertEquals(listOf(listOf("lte")), RynetService.failoverRounds(listOf("lte"), pings, setOf("lte"), 6))
    }
}
