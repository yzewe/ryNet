package dev.yzewe.rynet.data

import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class TvLinkTest {
    private val key = ByteArray(32) { it.toByte() }

    @Test
    fun sealedTextOpensOnlyWithItsKey() {
        val box = TvLink.seal(key, "https://panel.example/sub/abc")
        assertEquals("https://panel.example/sub/abc", TvLink.open(key, box))
        assertNull(TvLink.open(ByteArray(32), box))
        assertNull(TvLink.open(key, box.dropLast(2) + "AA"))
    }

    @Test
    fun parsesTheTvPageAndTheAppLink() {
        val offer = TvLink.Offer("192.168.1.50", 41234, key)
        val page = TvLink.parse(offer.url)
        assertNotNull(page)
        assertEquals("192.168.1.50", page!!.host)
        assertEquals(41234, page.port)
        assertTrue(page.key.contentEquals(key))
        val app = TvLink.parse(offer.appLink)
        assertEquals(41234, app?.port)
        assertTrue(app!!.key.contentEquals(key))
    }

    @Test
    fun ignoresLinksOutsideTheLocalNetworkAndOrdinarySubscriptions() {
        val k = TvLink.Offer("1.1.1.1", 80, key).url
        assertNull(TvLink.parse(k))
        assertNull(TvLink.parse("https://panel.example/sub/abc"))
        assertNull(TvLink.parse("http://192.168.1.50:41234/sub#xyz"))
        assertNull(TvLink.parse("http://192.168.1.50:41234/tv#short"))
    }

    @Test
    fun payloadSurvivesJsonAndDropsJunk() {
        val p = TvLink.Payload(listOf("https://a.example/s", "ftp://bad"), "vless://x@y:1#n", "abcd1234")
        val back = TvLink.Payload.of(p.json())!!
        assertEquals(listOf("https://a.example/s"), back.subs)
        assertEquals("vless://x@y:1#n", back.servers)
        assertEquals("abcd1234", back.hwid)
        assertNull(TvLink.Payload.of("""{"subs":[]}"""))
    }

    @Test
    fun phoneSendsAndTvReceivesOverTheServer() = runBlocking {
        val got = CompletableDeferred<TvLink.Payload>()
        val server = TvLink.Server {
            got.complete(it)
            null
        }
        val offer = server.open("127.0.0.1")!!
        val job = launch(Dispatchers.IO) { server.serve() }
        try {
            val page = URL("http://127.0.0.1:${offer.port}/tv").openConnection() as HttpURLConnection
            assertEquals(200, page.responseCode)
            assertTrue(page.inputStream.bufferedReader().readText().contains("rynet://tv"))

            TvLink.send(offer, TvLink.Payload(listOf("https://panel.example/sub/abc"), hwid = "feedface"))
            val p = withTimeout(5000) { got.await() }
            assertEquals(listOf("https://panel.example/sub/abc"), p.subs)
            assertEquals("feedface", p.hwid)

            val stranger = TvLink.Offer("127.0.0.1", offer.port, ByteArray(32))
            val failed = runCatching { TvLink.send(stranger, TvLink.Payload(listOf("https://evil.example/s"))) }
            assertTrue(failed.isFailure)
        } finally {
            server.close()
            job.cancel()
        }
    }

    @Test
    fun tvErrorReachesThePhone() = runBlocking {
        val server = TvLink.Server { "Подписка не загрузилась" }
        val offer = server.open("127.0.0.1")!!
        val job = launch(Dispatchers.IO) { server.serve() }
        try {
            val e = runCatching { TvLink.send(offer, TvLink.Payload(listOf("https://a.example/s"))) }.exceptionOrNull()
            assertEquals("Подписка не загрузилась", e?.message)
        } finally {
            server.close()
            job.cancel()
        }
    }
}
