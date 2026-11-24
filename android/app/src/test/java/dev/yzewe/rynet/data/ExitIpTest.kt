package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ExitIpTest {
    @Test
    fun ipwho() {
        assertEquals(ExitIp("95.216.12.40", "FI"), ExitIp.parse("""{"success":true,"ip":"95.216.12.40","country_code":"FI"}"""))
        assertNull(ExitIp.parse("""{"success":false,"message":"limit"}"""))
    }

    @Test
    fun ipApiLike() {
        assertEquals(ExitIp("1.2.3.4", "DE"), ExitIp.parse("""{"status":"success","query":"1.2.3.4","countryCode":"de"}"""))
        assertEquals(ExitIp("1.2.3.4", "NL"), ExitIp.parse("""{"ip_addr":"1.2.3.4","country":"NL"}"""))
        assertEquals(ExitIp("1.2.3.4", ""), ExitIp.parse("""{"origin":"1.2.3.4","country":"Netherlands"}"""))
    }

    @Test
    fun plainText() {
        assertEquals(ExitIp("203.0.113.7", ""), ExitIp.parse("203.0.113.7\n"))
    }

    @Test
    fun ipv6() {
        assertEquals(ExitIp("2001:db8::1", ""), ExitIp.parse("2001:db8::1"))
        assertEquals(ExitIp("2a01:4f9:c010::1", "FI"), ExitIp.parse("""{"ip":"2a01:4f9:c010::1","country_code":"FI"}"""))
    }

    @Test
    fun garbage() {
        assertNull(ExitIp.parse("999.1.1.1"))
        assertNull(ExitIp.parse("hello world"))
        assertNull(ExitIp.parse("""{"ip":"not-an-ip"}"""))
        assertNull(ExitIp.parse("<!doctype html><html><body>1.2.3.4</body></html>"))
        assertNull(ExitIp.parse("1.2.3.4, 5.6.7.8"))
        assertNull(ExitIp.parse(""))
        assertNull(ExitIp.parse("   "))
        assertNull(ExitIp.parse("{broken"))
    }
}
