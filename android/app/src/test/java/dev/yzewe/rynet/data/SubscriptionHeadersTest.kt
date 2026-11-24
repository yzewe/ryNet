package dev.yzewe.rynet.data

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class SubscriptionHeadersTest {
    @Test
    fun happRoutingLink() {
        val json = """{"GlobalProxy":"true","DirectSites":["geosite:category-ru"]}"""
        val b64 = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray())
        val r = Subscription.happRouting("happ://routing/onadd/$b64")
        assertEquals("true", JSONObject(r!!).getString("GlobalProxy"))
        assertEquals(r, Subscription.happRouting("happ://routing/add/$b64"))
        assertNull(Subscription.happRouting("happ://routing/off"))
        assertNull(Subscription.happRouting("happ://routing/onadd/!!!"))
        assertEquals(r, Subscription.happRouting("incy://routing/onadd/$b64"))
        assertEquals(r, Subscription.happRouting("://routing/add?data=$b64"))
        assertEquals(r, Subscription.happRouting(b64))
        assertNull(Subscription.happRouting("incy://routing/off"))
    }

    @Test
    fun urlMigration() {
        val u = "https://old.example/sub/abc?x=1"
        assertEquals("https://new.example/s/1", Subscription.migrated(u, "https://new.example/s/1", null))
        assertEquals("https://new.example/sub/abc?x=1", Subscription.migrated(u, null, "new.example"))
        assertEquals("https://new.example:8443/sub/abc?x=1", Subscription.migrated(u, null, "new.example:8443"))
        assertEquals(u, Subscription.migrated(u, "ftp://x", "bad domain"))
        assertEquals(u, Subscription.migrated(u, "http://new.example/s/1", null))
        assertEquals("http://new.example/s/1", Subscription.migrated("http://old.example/s", "http://new.example/s/1", null))
        assertEquals("https://new.example/s/1", Subscription.migrated("http://old.example/s", "https://new.example/s/1", null))
    }
}
