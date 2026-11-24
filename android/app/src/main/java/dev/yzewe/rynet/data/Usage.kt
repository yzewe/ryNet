package dev.yzewe.rynet.data

import android.content.Context

object Usage {
    private const val PREFS = "server-usage"
    private const val SEP = "\u0000"

    fun bump(ctx: Context, sel: Selection) {
        val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val key = sel.sub + SEP + sel.profile
        p.edit().putInt(key, p.getInt(key, 0) + 1).apply()
    }

    fun top(ctx: Context, sub: String, min: Int = 2): List<String> = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).all
        .mapNotNull { (k, v) -> if (k.startsWith(sub + SEP) && v is Int && v >= min) k.substringAfter(SEP) to v else null }
        .sortedByDescending { it.second }
        .map { it.first }

    fun forget(ctx: Context, sub: String) {
        val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        p.edit().apply { p.all.keys.filter { it.startsWith(sub + SEP) }.forEach(::remove) }.apply()
    }
}
