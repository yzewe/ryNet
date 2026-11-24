package dev.yzewe.rynet.data

import android.content.Context
import dev.yzewe.rynet.plural
import dev.yzewe.rynet.tr
import java.time.LocalDate
import org.json.JSONObject

data class ExpiryAlert(val sub: String, val title: String, val left: Long) {
    val text: String
        get() = if (left <= 0) {
            tr("Подписка «$title» истекла", "Subscription \"$title\" has expired")
        } else {
            tr("Подписка «$title» истекает через ${span(left)}", "Subscription \"$title\" expires in ${span(left)}")
        }

    companion object {
        private const val WINDOW = 3 * 24 * 3600_000L
        private const val FILE = "expiry_alerts.json"

        fun pick(ctx: Context, subs: List<Sub>, now: Long = System.currentTimeMillis()): ExpiryAlert? {
            val hidden = hidden(ctx)
            val today = LocalDate.now().toString()
            return subs.mapNotNull { s ->
                val expire = s.info.expire.takeIf { it > 0 } ?: return@mapNotNull null
                val left = expire * 1000 - now
                if (left >= WINDOW || hidden.optString(s.id) == today) return@mapNotNull null
                ExpiryAlert(s.id, s.title, left)
            }.minByOrNull { it.left }
        }

        fun dismiss(ctx: Context, sub: String) {
            Store.write(ctx, FILE, hidden(ctx).put(sub, LocalDate.now().toString()).toString())
        }

        private fun hidden(ctx: Context): JSONObject = Store.read(ctx, FILE)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: JSONObject()

        internal fun span(ms: Long): String {
            val hours = ms / 3600_000L
            val days = (hours / 24).toInt()
            val h = (hours % 24).toInt()
            return when {
                days > 0 && h > 0 -> plural(days, "день", "дня", "дней", "day") + " " + plural(h, "час", "часа", "часов", "hour")
                days > 0 -> plural(days, "день", "дня", "дней", "day")
                h > 0 -> plural(h, "час", "часа", "часов", "hour")
                else -> tr("меньше часа", "less than an hour")
            }
        }
    }
}
