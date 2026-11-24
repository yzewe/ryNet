package dev.yzewe.rynet.data

import android.content.Context
import android.content.pm.ApplicationInfo
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

object TvLink {
    private const val PATH = "/tv"
    private const val MAX_BODY = 256 * 1024

    class Offer(val host: String, val port: Int, val key: ByteArray) {
        val url: String get() = "http://$host:$port$PATH#${b64(key)}"
        val appLink: String get() = "rynet://tv?h=$host&p=$port&k=${b64(key)}"
    }

    data class Payload(val subs: List<String>, val servers: String? = null, val hwid: String? = null) {
        fun json(): String = JSONObject()
            .put("v", 1)
            .put("subs", JSONArray(subs))
            .apply { servers?.let { put("servers", it) } }
            .apply { hwid?.let { put("hwid", it) } }
            .toString()

        companion object {
            fun of(json: String): Payload? = runCatching {
                val o = JSONObject(json)
                val a = o.optJSONArray("subs") ?: JSONArray()
                Payload(
                    (0 until a.length()).map { a.getString(it).trim() }.filter { it.startsWith("http://", true) || it.startsWith("https://", true) },
                    o.optString("servers").takeIf { it.isNotBlank() },
                    o.optString("hwid").trim().takeIf { it.isNotEmpty() && it.length <= 128 }
                )
            }.getOrNull()?.takeIf { it.subs.isNotEmpty() || it.servers != null }
        }
    }

    fun parse(text: String?): Offer? {
        val t = text?.trim() ?: return null
        return runCatching {
            if (t.startsWith("rynet://tv", true)) {
                val u = Uri.parse(t)
                offer(u.getQueryParameter("h"), u.getQueryParameter("p")?.toIntOrNull(), u.getQueryParameter("k"))
            } else if (t.startsWith("http://", true)) {
                val u = URL(t.substringBefore('#'))
                if (u.path != PATH || !local(u.host)) null else offer(u.host, u.port, t.substringAfter('#', ""))
            } else {
                null
            }
        }.getOrNull()
    }

    private fun offer(host: String?, port: Int?, key: String?): Offer? {
        if (host == null || port == null || port !in 1..65535 || !local(host)) return null
        val k = key?.let { runCatching { Base64.getUrlDecoder().decode(it) }.getOrNull() }?.takeIf { it.size == 32 } ?: return null
        return Offer(host, port, k)
    }

    private fun local(host: String): Boolean {
        val p = host.split('.').mapNotNull { it.toIntOrNull() }
        if (p.size != 4 || p.any { it !in 0..255 }) return false
        return p[0] == 10 || p[0] == 192 && p[1] == 168 || p[0] == 172 && p[1] in 16..31 || p[0] == 169 && p[1] == 254
    }

    private fun b64(b: ByteArray) = Base64.getUrlEncoder().withoutPadding().encodeToString(b)

    fun seal(key: ByteArray, text: String): String {
        val nonce = ByteArray(12).also { SecureRandom().nextBytes(it) }
        val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce)) }
        return b64(nonce + c.doFinal(text.toByteArray()))
    }

    fun open(key: ByteArray, body: String): String? = runCatching {
        val raw = Base64.getUrlDecoder().decode(body.trim())
        val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, raw, 0, 12)) }
        String(c.doFinal(raw, 12, raw.size - 12))
    }.getOrNull()

    suspend fun send(offer: Offer, payload: Payload) = withContext(Dispatchers.IO) {
        val conn = URL("http://${offer.host}:${offer.port}$PATH").openConnection() as HttpURLConnection
        try {
            conn.connectTimeout = 5000
            conn.readTimeout = 60_000
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "text/plain")
            conn.outputStream.use { it.write(seal(offer.key, payload.json()).toByteArray()) }
            val code = conn.responseCode
            if (code != 200) {
                val msg = runCatching { (conn.errorStream ?: conn.inputStream).bufferedReader().readText() }.getOrNull()?.trim()
                error(msg?.takeIf { it.isNotEmpty() && it.length < 300 } ?: "HTTP $code")
            }
        } finally {
            conn.disconnect()
        }
    }

    fun debugEndpoint(ctx: Context): Pair<String?, Int> {
        if (ctx.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE == 0) return null to 0
        fun prop(name: String): String? = runCatching {
            Class.forName("android.os.SystemProperties").getMethod("get", String::class.java).invoke(null, name) as String
        }.getOrNull()?.trim()?.takeIf { it.isNotEmpty() }
        return prop("debug.rynet.tvhost")?.takeIf { local(it) } to (prop("debug.rynet.tvport")?.toIntOrNull()?.takeIf { it in 1..65535 } ?: 0)
    }

    fun lanAddress(): String? = runCatching {
        NetworkInterface.getNetworkInterfaces().toList()
            .filter { it.isUp && !it.isLoopback && !it.isVirtual && !it.name.startsWith("tun") }
            .sortedBy { if (it.name.startsWith("wlan") || it.name.startsWith("eth")) 0 else 1 }
            .flatMap { i -> i.inetAddresses.toList() }
            .filterIsInstance<Inet4Address>()
            .map { it.hostAddress.orEmpty() }
            .firstOrNull { local(it) }
    }.getOrNull()

    class Server(private val onPayload: suspend (Payload) -> String?) {
        private val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        private var socket: ServerSocket? = null

        fun open(host: String? = lanAddress(), port: Int = 0): Offer? {
            host ?: return null
            val s = ServerSocket()
            s.reuseAddress = true
            s.bind(InetSocketAddress(port))
            socket = s
            return Offer(host, s.localPort, key)
        }

        suspend fun serve() = withContext(Dispatchers.IO) {
            val s = socket ?: return@withContext
            while (!s.isClosed) {
                val client = runCatching { s.accept() }.getOrNull() ?: break
                runCatching { client.use { handle(it) } }
            }
        }

        fun close() {
            runCatching { socket?.close() }
            socket = null
        }

        private suspend fun handle(c: Socket) {
            c.soTimeout = 10_000
            val input = c.getInputStream()
            val head = readHead(input) ?: return
            val lines = head.split("\r\n")
            val (method, path) = lines.first().split(' ').let { (it.getOrNull(0) ?: "") to (it.getOrNull(1) ?: "") }
            if (path.substringBefore('?') != PATH) return reply(c, 404, "text/plain", "Not found")
            when (method) {
                "GET" -> reply(c, 200, "text/html; charset=utf-8", PAGE)

                "POST" -> {
                    val length = lines.firstNotNullOfOrNull { l ->
                        l.takeIf { it.startsWith("content-length:", true) }?.substringAfter(':')?.trim()?.toIntOrNull()
                    } ?: return reply(c, 411, "text/plain", "Length required")
                    if (length !in 1..MAX_BODY) return reply(c, 413, "text/plain", "Too large")
                    val body = String(readBytes(input, length))
                    val payload = open(key, body)?.let { Payload.of(it) }
                        ?: return reply(c, 400, "text/plain", "Bad request")
                    val err = onPayload(payload)
                    if (err == null) reply(c, 200, "text/plain", "ok") else reply(c, 500, "text/plain; charset=utf-8", err)
                }

                else -> reply(c, 405, "text/plain", "Method not allowed")
            }
        }

        private fun readHead(input: InputStream): String? {
            val out = ByteArrayOutputStream()
            var tail = 0
            while (out.size() < 8192) {
                val b = input.read()
                if (b < 0) return null
                out.write(b)
                tail = (tail shl 8) or b
                if (tail == 0x0d0a0d0a) return out.toString("ISO-8859-1").trimEnd()
            }
            return null
        }

        private fun readBytes(input: InputStream, n: Int): ByteArray {
            val buf = ByteArray(n)
            var off = 0
            while (off < n) {
                val r = input.read(buf, off, n - off)
                if (r < 0) break
                off += r
            }
            return buf.copyOf(off)
        }

        private fun reply(c: Socket, code: Int, type: String, text: String) {
            val body = text.toByteArray()
            val status = when (code) {
                200 -> "OK"
                400 -> "Bad Request"
                404 -> "Not Found"
                405 -> "Method Not Allowed"
                411 -> "Length Required"
                413 -> "Payload Too Large"
                else -> "Error"
            }
            c.getOutputStream().apply {
                write("HTTP/1.1 $code $status\r\nContent-Type: $type\r\nContent-Length: ${body.size}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n".toByteArray())
                write(body)
                flush()
            }
        }
    }

    private val PAGE = """<!doctype html><html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>ryNet</title>
<style>body{font-family:sans-serif;background:#101410;color:#e0e4dd;display:flex;min-height:100vh;margin:0;align-items:center;justify-content:center;text-align:center}
main{padding:24px;max-width:420px}a{display:block;margin:24px 0 12px;padding:16px;border-radius:100px;background:#8ad5b5;color:#003827;text-decoration:none;font-weight:600}
p{color:#bfc9c2;line-height:1.4}</style></head><body><main>
<h1>Передать подписку на телевизор</h1><p>Откройте ryNet на этом телефоне, чтобы выбрать подписку.</p>
<a id="open" href="#">Открыть в ryNet</a><p>Если кнопка не открывает приложение, установите ryNet и отсканируйте QR-код из него.</p>
</main><script>
var k=location.hash.slice(1),h=location.hostname,p=location.port;
var link='rynet://tv?h='+h+'&p='+p+'&k='+k;
document.getElementById('open').href='intent://tv?h='+h+'&p='+p+'&k='+k+'#Intent;scheme=rynet;package=dev.yzewe.rynet;end';
if(!/Android/i.test(navigator.userAgent))document.getElementById('open').href=link;
setTimeout(function(){location.href=document.getElementById('open').href},300);
</script></body></html>"""
}
