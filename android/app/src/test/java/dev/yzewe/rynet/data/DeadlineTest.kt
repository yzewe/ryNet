package dev.yzewe.rynet.data

import java.net.HttpURLConnection
import java.net.ServerSocket
import java.net.URL
import kotlin.concurrent.thread
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class DeadlineTest {
    @Test
    fun aDribblingServerIsCutAtTheDeadline() {
        val server = ServerSocket(0)
        thread(isDaemon = true) {
            runCatching {
                server.accept().use { s ->
                    s.getInputStream().read(ByteArray(4096))
                    val out = s.getOutputStream()
                    out.write("HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n".toByteArray())
                    repeat(1000) {
                        out.write('x'.code)
                        out.flush()
                        Thread.sleep(200)
                    }
                }
            }
        }
        val conn = URL("http://127.0.0.1:${server.localPort}/sub").openConnection() as HttpURLConnection
        conn.readTimeout = 20_000
        val t0 = System.currentTimeMillis()
        try {
            withDeadline(conn, 1500) { d -> d.wrap(conn.inputStream).readBytes() }
            fail("read the whole dribble")
        } catch (e: java.io.IOException) {
            val took = System.currentTimeMillis() - t0
            assertTrue("ended at the deadline: $took ms", took in 1400..4000)
            assertTrue(e.message.orEmpty(), e.message.orEmpty().contains("did not answer in time") || e.message.orEmpty().contains("не ответил"))
        } finally {
            conn.disconnect()
            server.close()
        }
    }
}
