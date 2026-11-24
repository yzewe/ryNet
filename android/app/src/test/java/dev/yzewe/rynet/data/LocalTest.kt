package dev.yzewe.rynet.data

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class LocalTest {
    @Test
    fun serversOrSubscription() {
        assertTrue(Local.isServers("vless://id@1.2.3.4:443?security=tls#A"))
        assertTrue(Local.isServers("  hy2://pw@h.example:443#B\ntrojan://pw@t.example:443#C"))
        assertTrue(Local.isServers("""{"outbounds":[]}"""))
        val b64 = java.util.Base64.getEncoder().encodeToString("ss://YWVzOnB3@h.example:8388#D\n".toByteArray())
        assertTrue(Local.isServers(b64))
        assertFalse(Local.isServers("https://sub.example/abc"))
        assertFalse(Local.isServers("sub.example/abc"))
        assertFalse(Local.isServers("happ://add/https://sub.example/abc"))
    }

    @Test
    fun renameKeepsLinkAndDescription() {
        assertEquals("vless://id@h:443?security=tls#%D0%9C%D0%BE%D0%B9%20DE", Local.renamed("vless://id@h:443?security=tls#Old", "Мой DE"))
        assertEquals("trojan://pw@h:443#New", Local.renamed("trojan://pw@h:443", "New"))
        assertEquals("vless://id@h:443#New?serverDescription=ZGVz", Local.renamed("vless://id@h:443#Old?serverDescription=ZGVz", "New"))
        val vmess = "vmess://" + java.util.Base64.getEncoder().encodeToString("""{"add":"h","port":"443","id":"x","ps":"Old"}""".toByteArray())
        val out = Local.renamed(vmess, "New")
        assertEquals("New", JSONObject(String(java.util.Base64.getDecoder().decode(out.removePrefix("vmess://")))).getString("ps"))
    }
}
