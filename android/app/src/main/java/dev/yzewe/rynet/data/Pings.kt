package dev.yzewe.rynet.data

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

data class Pings(
    val sub: String? = null,
    val results: Map<String, Int> = emptyMap(),
    val running: Boolean = false,
    val errors: Map<String, String> = emptyMap(),
    val done: Int = 0,
    val total: Int = 0,
    val target: String? = null,
    val targets: List<String>? = null
) {
    private fun targeted(name: String): Boolean = if (targets != null) {
        name in targets
    } else if (target == null) {
        Profile.MEMBER_SEP !in name
    } else {
        target == name || target.endsWith(Profile.MEMBER_SEP) && name.startsWith(target)
    }

    fun waiting(name: String): Boolean = running && name !in results && targeted(name)

    fun restart(only: String?, names: List<String>? = null): Pings {
        val again = Pings(sub, target = only, targets = names)
        return Pings(sub, results.filterKeys { !again.targeted(it) }, running = true, errors = errors.filterKeys { !again.targeted(it) }, target = only, targets = names)
    }

    companion object {
        const val TIMEOUT = -1
        const val FAILED = -2

        fun classify(v: Any?): Pair<Int, String?> = when {
            v is Number -> v.toInt().coerceAtLeast(TIMEOUT) to null
            v is String && ((v.startsWith("Таймаут") || v.startsWith("Timeout")) || v.isBlank()) -> TIMEOUT to v.ifBlank { null }
            v is String -> FAILED to v
            else -> TIMEOUT to null
        }

        const val ACTION = "dev.yzewe.rynet.PINGS"
        private const val FILE = "pings.json"
        const val STALE_MS = 120_000L

        fun load(ctx: Context): Pings = Store.read(ctx, FILE)?.let(::parse) ?: Pings()

        fun forSub(ctx: Context, sub: String): Pings = load(ctx).takeIf { it.sub == sub } ?: archived(ctx, sub) ?: Pings(sub)

        fun archived(ctx: Context, sub: String): Pings? = Store.read(ctx, archive(sub))?.let(::parse)?.takeIf { it.sub == sub }?.copy(running = false, done = 0, total = 0, target = null, targets = null)

        fun forget(ctx: Context, sub: String) = Store.delete(ctx, archive(sub))

        private fun archive(sub: String): String {
            val d = java.security.MessageDigest.getInstance("SHA-256").digest(sub.toByteArray())
            return "pings-" + d.take(4).joinToString("") { "%02x".format(it) } + ".json"
        }

        private fun parse(text: String): Pings? = runCatching {
            val o = JSONObject(text)
            val r = o.optJSONObject("results") ?: JSONObject()
            val e = o.optJSONObject("errors") ?: JSONObject()
            Pings(
                o.optString("sub").ifEmpty { null },
                r.keys().asSequence().associateWith { k -> r.getInt(k) },
                o.optBoolean("running") && fresh(o.optLong("at")),
                e.keys().asSequence().associateWith { k -> e.getString(k) },
                o.optInt("done"),
                o.optInt("total"),
                o.optString("target").ifEmpty { null },
                o.optJSONArray("targets")?.let { a -> (0 until a.length()).map { a.getString(it) } }
            )
        }.getOrNull()

        internal fun fresh(at: Long, now: Long = System.currentTimeMillis()): Boolean = at in (now - STALE_MS)..(now + STALE_MS)

        fun merge(ctx: Context, sub: String, found: Map<String, Int>) {
            if (found.isEmpty()) return
            val now = load(ctx)
            if (now.sub == sub) {
                if (!now.running) publish(ctx, now.copy(results = now.results + found, errors = now.errors - found.filterValues { it >= 0 }.keys))
                return
            }
            val old = archived(ctx, sub) ?: Pings(sub)
            val p = old.copy(results = old.results + found, errors = old.errors - found.filterValues { it >= 0 }.keys)
            val r = JSONObject()
            p.results.forEach { (k, v) -> r.put(k, v) }
            val e = JSONObject()
            p.errors.forEach { (k, v) -> e.put(k, v) }
            Store.write(ctx, archive(sub), JSONObject().put("sub", sub).put("results", r).put("errors", e).put("running", false).put("done", 0).put("total", 0).put("target", "").toString())
            ctx.sendBroadcast(android.content.Intent(ACTION).setPackage(ctx.packageName))
        }

        fun publish(ctx: Context, p: Pings) {
            val r = JSONObject()
            p.results.forEach { (k, v) -> r.put(k, v) }
            val e = JSONObject()
            p.errors.forEach { (k, v) -> e.put(k, v) }
            val text = JSONObject().put("sub", p.sub ?: "").put("results", r).put("errors", e).put("running", p.running).put("done", p.done).put("total", p.total)
                .put("target", p.target ?: "").apply { p.targets?.let { put("targets", JSONArray(it)) } }.put("at", System.currentTimeMillis()).toString()
            Store.write(ctx, FILE, text)
            if (!p.running && p.sub != null && p.results.isNotEmpty()) Store.write(ctx, archive(p.sub), text)
            ctx.sendBroadcast(android.content.Intent(ACTION).setPackage(ctx.packageName))
        }
    }
}
