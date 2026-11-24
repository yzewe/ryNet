package dev.yzewe.rynet.data

import java.io.ByteArrayOutputStream
import org.junit.Assert.assertEquals
import org.junit.Test

class GeoDatTest {
    private fun varint(o: ByteArrayOutputStream, v: Long) {
        var x = v
        while (x >= 0x80) {
            o.write(((x and 0x7f) or 0x80).toInt())
            x = x ushr 7
        }
        o.write(x.toInt())
    }
    private fun bytes(field: Int, b: ByteArray) = ByteArrayOutputStream().apply {
        varint(this, (field shl 3 or 2).toLong())
        varint(this, b.size.toLong())
        write(b)
    }.toByteArray()
    private fun num(field: Int, v: Long) = ByteArrayOutputStream().apply {
        varint(this, (field shl 3).toLong())
        varint(this, v)
    }.toByteArray()
    private fun msg(vararg parts: ByteArray) = parts.fold(ByteArray(0)) { a, b -> a + b }

    private fun domain(type: Long, value: String, vararg attrs: String) = msg(num(1, type), bytes(2, value.toByteArray()), *attrs.map { bytes(3, msg(bytes(1, it.toByteArray()), num(2, 1))) }.toTypedArray())

    @Test
    fun sitesWithTypesAndAttributes() {
        val google = msg(bytes(1, "GOOGLE".toByteArray()), bytes(2, domain(2, "google.com")), bytes(2, domain(3, "g.cn", "cn")), bytes(2, domain(0, "gstatic")), bytes(2, domain(1, "^a\\.b$")))
        val other = msg(bytes(1, "RU".toByteArray()), bytes(2, domain(2, "ya.ru")))
        val dat = msg(bytes(1, other), bytes(1, google))
        val out = GeoDat.sites(dat.inputStream(), setOf("google", "google@cn", "google@!cn"))
        assertEquals(listOf("domain:google.com", "full:g.cn", "keyword:gstatic", "regexp:^a\\.b$"), out["google"])
        assertEquals(listOf("full:g.cn"), out["google@cn"])
        assertEquals(listOf("domain:google.com", "keyword:gstatic", "regexp:^a\\.b$"), out["google@!cn"])
        assertEquals(setOf("google", "google@cn", "google@!cn"), out.keys)
    }

    @Test
    fun ipsSkipInverse() {
        fun cidr(ip: ByteArray, p: Long) = bytes(2, msg(bytes(1, ip), num(2, p)))
        val ru = msg(
            bytes(1, "RU".toByteArray()),
            cidr(byteArrayOf(5, 8, 0, 0), 13),
            cidr(
                ByteArray(16).also {
                    it[0] = 0x2a
                    it[1] = 0x00
                },
                16
            )
        )
        val inv = msg(bytes(1, "XX".toByteArray()), cidr(byteArrayOf(1, 1, 1, 1), 32), num(3, 1))
        val out = GeoDat.ips(msg(bytes(1, ru), bytes(1, inv)).inputStream(), setOf("ru", "xx"))
        assertEquals(listOf("5.8.0.0/13", "2a00:0:0:0:0:0:0:0/16"), out["ru"])
        assertEquals(setOf("ru"), out.keys)
    }
}
