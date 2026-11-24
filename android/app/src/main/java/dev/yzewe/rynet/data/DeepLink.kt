package dev.yzewe.rynet.data

import android.net.Uri
import java.util.Base64

object DeepLink {
    sealed interface Target {
        data class Subscription(val url: String, val name: String? = null) : Target
        data class Servers(val text: String) : Target
    }

    private val HOSTS = setOf("add", "import", "install-config", "install-sub", "sub", "subscription")

    fun parse(link: String?): Target? {
        val raw = link?.trim()?.takeIf { it.startsWith("rynet://", ignoreCase = true) } ?: return null
        val rest = raw.substring("rynet://".length)
        val host = rest.substringBefore('/').substringBefore('?').lowercase()
        if (host !in HOSTS) return null
        val after = rest.substring(host.length).removePrefix("/")
        val params = if (after.startsWith("?")) {
            after.substring(1).split('&').mapNotNull { kv -> kv.split('=', limit = 2).takeIf { it.size == 2 }?.let { it[0].lowercase() to Uri.decode(it[1]) } }.toMap()
        } else {
            emptyMap()
        }
        val name = (params["name"] ?: params["title"])?.trim()?.takeIf { it.isNotEmpty() }
        val payload = params["url"] ?: params["link"] ?: params["config"]
            ?: after.takeIf { it.isNotBlank() && !it.startsWith("?") }?.let { p -> if ("://" in p) p else Uri.decode(p) }
            ?: return null
        val value = payload.trim().let { if ("://" in it) it else decode(it) ?: it }
        val (body, fragment) = value.substringBefore('#') to value.substringAfter('#', "").takeIf { it.isNotEmpty() }
        return when {
            Local.isServers(value) -> Target.Servers(value)

            value.startsWith("http://", true) || value.startsWith("https://", true) ->
                Target.Subscription(body, name ?: fragment?.let(Uri::decode))

            else -> null
        }
    }

    private fun decode(s: String): String? = runCatching {
        val t = s.trim().replace('-', '+').replace('_', '/')
        String(Base64.getDecoder().decode(t + "=".repeat((4 - t.length % 4) % 4)))
    }.getOrNull()?.takeIf { "://" in it }
}
