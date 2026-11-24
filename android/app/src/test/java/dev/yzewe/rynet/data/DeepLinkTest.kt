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
class DeepLinkTest {
    private fun sub(url: String, name: String? = null) = DeepLink.Target.Subscription(url, name)

    @Test
    fun subscriptionLinksInEveryShape() {
        assertEquals(sub("https://sub.example/a?t=1"), DeepLink.parse("rynet://add/https://sub.example/a?t=1"))
        assertEquals(sub("https://sub.example/a", "Моя"), DeepLink.parse("rynet://add/https://sub.example/a#%D0%9C%D0%BE%D1%8F"))
        assertEquals(sub("https://sub.example/a?t=1", "Home"), DeepLink.parse("rynet://add?url=https%3A%2F%2Fsub.example%2Fa%3Ft%3D1&name=Home"))
        assertEquals(sub("https://sub.example/a"), DeepLink.parse("rynet://import/https%3A%2F%2Fsub.example%2Fa"))
        assertEquals(sub("https://sub.example/a"), DeepLink.parse("rynet://install-config?url=https://sub.example/a"))
        val b64 = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString("https://sub.example/b".toByteArray())
        assertEquals(sub("https://sub.example/b"), DeepLink.parse("rynet://add/$b64"))
    }

    @Test
    fun serverLinksAreServers() {
        val link = "vless://11111111-2222-3333-4444-555555555555@host.example:443?security=tls#A"
        val t = DeepLink.parse("rynet://add/$link")
        assertTrue(t is DeepLink.Target.Servers)
        assertEquals(link, (t as DeepLink.Target.Servers).text)
    }

    @Test
    fun otherLinksAreIgnored() {
        assertNull(DeepLink.parse("rynet://set/abc"))
        assertNull(DeepLink.parse("rynet://add/"))
        assertNull(DeepLink.parse("happ://add/https://sub.example/a"))
        assertNull(DeepLink.parse("rynet://add/not-a-link"))
    }
}
