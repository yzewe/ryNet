package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PingsTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Test
    fun otherSubscriptionKeepsItsResults() {
        Pings.publish(ctx, Pings("a", mapOf("de" to 40, "nl" to Pings.TIMEOUT)))
        Pings.publish(ctx, Pings("b", running = true))
        assertEquals(mapOf("de" to 40, "nl" to Pings.TIMEOUT), Pings.forSub(ctx, "a").results)
        assertEquals(true, Pings.forSub(ctx, "b").running)

        Pings.forget(ctx, "a")
        assertNull(Pings.archived(ctx, "a"))
        assertEquals(emptyMap<String, Int>(), Pings.forSub(ctx, "a").results)
    }

    @Test
    fun searchRunKeepsResultsOutsideTheSearchAndOnlyWaitsForMatches() {
        val old = Pings("search", mapOf("de" to 50, "nl" to 70), errors = mapOf("nl" to "old"))
        val next = old.restart(null, listOf("de"))
        assertEquals(mapOf("nl" to 70), next.results)
        assertEquals(mapOf("nl" to "old"), next.errors)
        assertEquals(true, next.waiting("de"))
        assertEquals(false, next.waiting("fr"))
        Pings.publish(ctx, next)
        assertEquals(listOf("de"), Pings.load(ctx).targets)
        assertEquals(false, Pings.load(ctx).waiting("fr"))
    }

    @Test
    fun anEmptySearchNeverTargetsAllServers() {
        val next = Pings("search", mapOf("de" to 50)).restart(null, emptyList())
        assertEquals(mapOf("de" to 50), next.results)
        assertEquals(false, next.waiting("fr"))
    }

    @Test
    fun aRunThatStoppedPublishingIsOver() {
        Pings.publish(ctx, Pings("c", running = true))
        assertEquals(true, Pings.load(ctx).running)
        val stale = org.json.JSONObject(Store.read(ctx, "pings.json")!!).put("at", System.currentTimeMillis() - Pings.STALE_MS - 1000)
        Store.write(ctx, "pings.json", stale.toString())
        assertEquals(false, Pings.load(ctx).running)
        Store.write(ctx, "pings.json", stale.apply { remove("at") }.toString())
        assertEquals(false, Pings.load(ctx).running)
        assertEquals("c", Pings.load(ctx).sub)
    }
}
