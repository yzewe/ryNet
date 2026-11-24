package dev.yzewe.rynet.data

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SubIconTest {
    @Test fun localAddressesAreNotFetched() {
        for (a in listOf(
            "127.0.0.1", "0.0.0.0", "0.1.2.3", "10.0.0.1", "172.16.5.4", "192.168.1.1", "169.254.169.254", "100.64.0.1", "100.127.255.255",
            "224.0.0.1", "255.255.255.255", "240.0.0.1", "::1", "::", "fe80::1", "fc00::1", "fd7a:115c:a1e0::1", "ff02::1", "::ffff:192.168.1.1"
        )) {
            assertTrue(a, SubIcon.local(InetAddress.getByName(a)))
        }
        for (a in listOf("1.1.1.1", "100.63.255.255", "100.128.0.1", "172.32.0.1", "198.18.0.5", "198.19.3.4", "2606:4700:4700::1111", "2a02:6b8::2:242")) {
            assertFalse(a, SubIcon.local(InetAddress.getByName(a)))
        }
    }

    @Test fun hugeAndTinyImagesAreRefusedAndLargeOnesAreSampledDown() {
        assertNull(SubIcon.sampleFor(15, 64))
        assertNull(SubIcon.sampleFor(-1, -1))
        assertNull(SubIcon.sampleFor(30000, 30000))
        assertNull(SubIcon.sampleFor(64, 2049))
        assertEquals(1, SubIcon.sampleFor(16, 16))
        assertEquals(1, SubIcon.sampleFor(180, 180))
        assertEquals(1, SubIcon.sampleFor(511, 300))
        assertEquals(2, SubIcon.sampleFor(512, 512))
        assertEquals(8, SubIcon.sampleFor(2048, 100))
    }
}
