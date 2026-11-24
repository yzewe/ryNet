package dev.yzewe.rynet.data

import android.annotation.SuppressLint
import android.content.Context
import android.os.Build
import android.provider.Settings as SysSettings
import java.net.HttpURLConnection
import java.security.SecureRandom

object Hwid {
    @SuppressLint("HardwareIds")
    fun default(ctx: Context): String = SysSettings.Secure.getString(ctx.contentResolver, SysSettings.Secure.ANDROID_ID)?.takeIf { it.isNotBlank() }
        ?: "0000000000000000"

    fun current(ctx: Context): String = Settings.load(ctx).hwid.trim().ifEmpty { default(ctx) }

    fun random(): String = ByteArray(8).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }

    fun cleartext(url: java.net.URL): Boolean = url.protocol.equals("http", ignoreCase = true)

    fun apply(ctx: Context, conn: HttpURLConnection) {
        val s = Settings.load(ctx)
        if (s.sendHwid && !cleartext(conn.url)) {
            conn.setRequestProperty("x-hwid", current(ctx))
            conn.setRequestProperty("x-device-os", "Android")
            conn.setRequestProperty("x-ver-os", Build.VERSION.RELEASE)
            conn.setRequestProperty("x-device-model", Build.MODEL)
        }
        if (s.subClient == SubClient.Custom) for ((k, v) in headers(s.subHeaders)) conn.setRequestProperty(k, v)
    }

    fun headers(text: String): List<Pair<String, String>> = text.lines().mapNotNull { l ->
        val i = l.indexOf(':')
        if (i <= 0) return@mapNotNull null
        val k = l.substring(0, i).trim()
        k.takeIf { it.isNotEmpty() && it.none { c -> c.isWhitespace() } }?.let { it to l.substring(i + 1).trim() }
    }
}
