package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CoreStatsTest {
    private fun snap(conns: Long, ok: Long, fail: Long, generation: Long = 1) = CoreStats.Snapshot(generation, 0, 0, 0, 0, 0, 0, 0, 0, proxyConns = conns, proxyOk = ok, proxyFail = fail)

    private val mark = snap(100, 80, 5)

    @Test
    fun nothingThroughTheProxyIsIdle() {
        assertEquals("idle", CoreStats.healthSkip(mark, snap(100, 80, 5))?.why)
    }

    @Test
    fun serverThatCannotBeDialedIsPinged() {
        assertNull(CoreStats.healthSkip(mark, snap(100, 80, 217)))
    }

    @Test
    fun attemptsWithoutSuccessesArePinged() {
        assertNull(CoreStats.healthSkip(mark, snap(103, 80, 5)))
        assertNull(CoreStats.healthSkip(mark, snap(101, 80, 5)))
    }

    @Test
    fun servedSkipsWhileMostAttemptsWork() {
        val s = CoreStats.healthSkip(mark, snap(120, 98, 6))
        assertEquals("served", s?.why)
        assertEquals(21L, s?.tried)
        assertEquals(18L, s?.ok)
    }

    @Test
    fun servedDoesNotWinWhenHalfOrMoreFail() {
        assertNull(CoreStats.healthSkip(mark, snap(110, 85, 15)))
        assertNull(CoreStats.healthSkip(mark, snap(102, 82, 7)))
    }

    @Test
    fun fewAttemptsWithASuccessStillSkip() {
        assertEquals("served", CoreStats.healthSkip(mark, snap(101, 81, 7))?.why)
    }

    @Test
    fun newCoreGenerationOrNoMarkPings() {
        assertNull(CoreStats.healthSkip(null, mark))
        assertNull(CoreStats.healthSkip(mark, null))
        assertNull(CoreStats.healthSkip(mark, snap(100, 80, 5, generation = 2)))
    }
}
