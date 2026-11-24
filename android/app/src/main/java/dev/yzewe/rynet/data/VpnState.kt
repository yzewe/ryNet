package dev.yzewe.rynet.data

import android.content.Context
import android.content.Intent
import dev.yzewe.rynet.widget.ToggleWidget
import org.json.JSONObject

enum class Status { Off, Connecting, On, Error }

data class VpnState(
    val status: Status = Status.Off,
    val profile: String? = null,
    val since: Long = 0,
    val error: String? = null,
    val blocked: Boolean = false,
    val failing: Boolean = false
) {
    companion object {
        const val ACTION = "dev.yzewe.rynet.STATE"

        fun load(ctx: Context): VpnState = Store.read(ctx, Store.STATE)?.let {
            runCatching {
                val o = JSONObject(it)
                VpnState(
                    Status.valueOf(o.getString("status")),
                    o.optString("profile").ifEmpty { null },
                    o.optLong("since"),
                    o.optString("error").ifEmpty { null },
                    o.optBoolean("blocked"),
                    o.optBoolean("failing")
                )
            }.getOrNull()
        } ?: VpnState()

        fun publish(ctx: Context, s: VpnState) {
            val o = JSONObject().put("status", s.status.name).put("profile", s.profile ?: "")
                .put("since", s.since).put("error", s.error ?: "").put("blocked", s.blocked).put("failing", s.failing)
            Store.write(ctx, Store.STATE, o.toString())
            ctx.sendBroadcast(Intent(ACTION).setPackage(ctx.packageName))
            ToggleWidget.refresh(ctx)
        }
    }
}
