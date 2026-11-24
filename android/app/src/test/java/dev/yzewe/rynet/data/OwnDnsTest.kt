package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class OwnDnsTest {
    @Test
    fun addressesSplitOnSpacesCommasAndLines() {
        val s = Settings(dnsServers = "https://1.1.1.1/dns-query,\n tls+local://dns.google  8.8.8.8 localhost")
        assertEquals(listOf("https://1.1.1.1/dns-query", "tls+local://dns.google", "8.8.8.8", "localhost"), s.ownDns())
        assertEquals(emptyList<String>(), Settings().ownDns())
    }

    @Test
    fun ownDnsSurvivesSaving() {
        val s = Settings(dnsServers = "tls://dns.google")
        assertEquals(s.dnsServers, Settings.fromJson(org.json.JSONObject(s.toJson().toString())).dnsServers)
    }
}
