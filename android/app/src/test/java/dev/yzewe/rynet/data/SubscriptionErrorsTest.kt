package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import dev.yzewe.rynet.tr
import java.net.ServerSocket
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class SubscriptionErrorsTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun serve(status: String, headers: String = "", body: String = ""): Int {
        val s = ServerSocket(0)
        thread(isDaemon = true) {
            s.use { srv ->
                srv.accept().use { c ->
                    val r = c.getInputStream().bufferedReader()
                    while (r.readLine()?.isNotEmpty() == true) Unit
                    val bytes = body.toByteArray()
                    c.getOutputStream().write("HTTP/1.1 $status\r\n${headers}Content-Length: ${bytes.size}\r\nConnection: close\r\n\r\n".toByteArray() + bytes)
                }
            }
        }
        return s.localPort
    }

    private fun failure(url: String): Throwable = runCatching { Subscription.download(ctx, url) }.exceptionOrNull()!!

    @Test
    fun refusedConnectionSaysTheSiteIsNotResponding() {
        val e = failure("http://127.0.0.1:9/sub")
        assertEquals(tr("Сайт подписки не отвечает — возможно, он недоступен в этой сети", "The subscription site isn't responding — it may be unreachable on this network"), e.message)
    }

    @Test
    fun notFoundSaysTheLinkIsOutdated() {
        val port = serve("404 Not Found")
        val e = failure("http://127.0.0.1:$port/sub")
        assertEquals(tr("Ссылка не найдена — возможно, устарела: возьмите новую у провайдера", "Link not found — it may be outdated: get a new one from the provider"), e.message)
    }

    @Test
    fun emptyAnswerWithoutHwidIsItsOwnError() {
        Settings.save(ctx, Settings.load(ctx).copy(sendHwid = false))
        val port = serve("200 OK", "x-hwid-not-supported: true\r\nContent-Type: text/plain\r\n", "")
        val e = failure("http://127.0.0.1:$port/sub")
        assertTrue(e.toString(), e is Subscription.NeedHwid)
    }

    @Test
    fun hwidOverCleartextAsksForHttps() {
        Settings.save(ctx, Settings.load(ctx).copy(sendHwid = true))
        val port = serve("200 OK", "x-hwid-not-supported: true\r\nContent-Type: text/plain\r\n", "")
        val e = failure("http://127.0.0.1:$port/sub")
        assertTrue(e.toString(), e !is Subscription.NeedHwid && e.message.orEmpty().contains("https://"))
    }

    @Test
    fun splitTurnsBalancerMembersIntoRows() {
        val member = Profile("Литва\u001fproxy", "proxy", "🇱🇹", Group.Main, "REALITY", true, emptyList())
        val balancer = Profile("🇱🇹 Литва", "Литва", "🇱🇹", Group.Main, "Балансер", true, emptyList(), members = listOf(member))
        val solo = Profile("🇫🇮 Финляндия", "Финляндия", "🇫🇮", Group.Main, "REALITY", true, emptyList())
        val out = Subscription.split(listOf(balancer, solo))
        assertEquals(listOf("Литва [proxy]", "Финляндия"), out.map { it.name })
        assertEquals("Литва\u001fproxy", out[0].remarks)
    }
}
