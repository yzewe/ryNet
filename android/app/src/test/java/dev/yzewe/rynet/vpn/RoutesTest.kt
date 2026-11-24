package dev.yzewe.rynet.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RoutesTest {
    private fun ip(a: String) = a.split('.').fold(0L) { n, b -> n shl 8 or b.toLong() }

    @Test
    fun coversEverythingExceptLan() {
        val lan = listOf("10.0.0.0" to 8, "192.168.0.0" to 16, "169.254.0.0" to 16, "224.0.0.0" to 4)
        val routes = RynetService.routesExcept(lan)
        val size = { len: Int -> 1L shl (32 - len) }
        assertEquals(1L shl 32, routes.sumOf { size(it.second) } + lan.sumOf { size(it.second) })
        val inRoutes = { a: String -> routes.any { (r, len) -> ip(a) in ip(r) until ip(r) + size(len) } }
        assertTrue(inRoutes("1.1.1.1"))
        assertTrue(inRoutes("172.19.0.1"))
        assertTrue(!inRoutes("192.168.1.1") && !inRoutes("10.1.2.3") && !inRoutes("239.255.255.250"))
    }
}
