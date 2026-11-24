package dev.yzewe.rynet.data

import android.content.Context
import org.json.JSONObject

object Countries {
    const val ACTION = "dev.yzewe.rynet.COUNTRIES"
    private const val FILE = "countries.json"
    const val TTL_MS = 7 * 24 * 3600_000L

    private fun all(ctx: Context): JSONObject = Store.read(ctx, FILE)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: JSONObject()

    fun endpoint(p: Profile): String = p.server ?: p.hosts.joinToString(",")

    fun endpoints(profiles: List<Profile>): Map<String, String> = profiles.flatMap { p -> listOf(p) + p.members }.associate { it.remarks to endpoint(it) }

    fun load(ctx: Context, sub: String, endpoints: Map<String, String>, now: Long = System.currentTimeMillis()): Map<String, String> {
        val o = all(ctx).optJSONObject(sub) ?: return emptyMap()
        return o.keys().asSequence().mapNotNull { k ->
            o.optJSONObject(k)
                ?.takeIf { now - it.optLong("at") in 0 until TTL_MS && it.optString("e") == endpoints[k] }
                ?.optString("c")?.takeIf { it.length == 2 }?.let { k to it }
        }.toMap()
    }

    @Synchronized
    fun put(ctx: Context, sub: String, found: Map<String, String>, endpoints: Map<String, String>, now: Long = System.currentTimeMillis()) {
        val known = found.filterKeys { it in endpoints }
        if (known.isEmpty()) return
        val root = all(ctx)
        val o = root.optJSONObject(sub) ?: JSONObject()
        val changed = known.any { (k, c) -> o.optJSONObject(k)?.optString("c") != c.uppercase() || o.optJSONObject(k)?.optString("e") != endpoints[k] }
        known.forEach { (k, c) -> o.put(k, JSONObject().put("c", c.uppercase()).put("e", endpoints[k]).put("at", now)) }
        o.keys().asSequence().toList().filter { now - (o.optJSONObject(it)?.optLong("at") ?: 0) !in 0 until TTL_MS }.forEach(o::remove)
        Store.write(ctx, FILE, root.put(sub, o).toString())
        if (changed) ctx.sendBroadcast(android.content.Intent(ACTION).setPackage(ctx.packageName))
    }

    @Synchronized
    fun clear(ctx: Context) {
        if (Store.read(ctx, FILE) == null) return
        Store.delete(ctx, FILE)
        ctx.sendBroadcast(android.content.Intent(ACTION).setPackage(ctx.packageName))
    }

    @Synchronized
    fun forget(ctx: Context, sub: String) {
        val root = all(ctx)
        if (root.remove(sub) != null) Store.write(ctx, FILE, root.toString())
    }

    fun flags(ctx: Context, sub: String, profiles: List<Profile>): List<Profile> {
        val known = load(ctx, sub, endpoints(profiles))
        if (known.isEmpty()) return profiles
        return profiles.map { p ->
            val members = p.members.takeIf { it.isNotEmpty() }?.map { m -> known[m.remarks]?.let { m.copy(flag = flag(it)) } ?: m }
            val own = known[p.remarks]?.let(::flag)
            if (own == null && members == null) p else p.copy(flag = own ?: p.flag, members = members ?: p.members)
        }
    }

    fun flag(code: String): String = ExitIp("", code).flag
}
