package dev.yzewe.rynet.vpn

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.VpnService
import dev.yzewe.rynet.data.Diag
import dev.yzewe.rynet.data.Selection
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.ui.HomeActivity

object Control {
    const val CONNECT = "dev.yzewe.rynet.CONNECT"
    const val DISCONNECT = "dev.yzewe.rynet.DISCONNECT"
    const val TOGGLE = "dev.yzewe.rynet.TOGGLE"
    const val EXTRA_SERVER = "server"
    const val EXTRA_SUBSCRIPTION = "subscription"
    const val EXTRA_MODE = "mode"

    enum class Result { Done, Disabled, NeedsApp }

    private fun action(i: Intent): String? = when {
        i.action in setOf(CONNECT, DISCONNECT, TOGGLE) -> i.action

        i.data?.scheme == "rynet" -> when (i.data?.host) {
            "connect" -> CONNECT
            "disconnect" -> DISCONNECT
            "toggle" -> TOGGLE
            else -> null
        }

        else -> null
    }

    private fun extra(i: Intent, key: String): String? = (i.getStringExtra(key) ?: i.data?.getQueryParameter(key))?.trim()?.takeIf { it.isNotEmpty() }

    fun run(ctx: Context, i: Intent, own: Boolean = false): Result {
        val action = action(i) ?: return Result.Done
        if (!own && !Settings.load(ctx).remoteControl) return Result.Disabled
        when (extra(i, EXTRA_MODE)?.lowercase()) {
            "proxy", "tun", "vpn" -> {
                val st = Settings.load(ctx)
                val want = extra(i, EXTRA_MODE).equals("proxy", ignoreCase = true)
                if (st.proxyMode != want) Settings.change(ctx, st, st.copy(proxyMode = want))
            }
        }
        val s = VpnState.load(ctx).status
        val on = s == Status.On || s == Status.Connecting
        if (action == DISCONNECT || action == TOGGLE && on) {
            if (on) RynetService.disconnect(ctx, "remote")
            return Result.Done
        }
        if (RynetService.consent(ctx) != null) return Result.NeedsApp
        val sel = pick(ctx, extra(i, EXTRA_SERVER), extra(i, EXTRA_SUBSCRIPTION)) ?: return Result.NeedsApp
        Subs.select(ctx, sel)
        Diag.event(ctx, "remote", "action" to action.substringAfterLast('.').lowercase())
        RynetService.connect(ctx, sel)
        return Result.Done
    }

    private fun pick(ctx: Context, server: String?, sub: String?): Selection? {
        val subs = Subs.list(ctx).filter { sub == null || it.id == sub || it.info.title.equals(sub, ignoreCase = true) }
        if (subs.isEmpty()) return null
        val current = Subs.selection(ctx)
        if (server == null) {
            current?.takeIf { c -> subs.any { it.id == c.sub } }?.let { return it }
            return subs.firstNotNullOfOrNull { s -> Subs.profiles(ctx, s.id).firstOrNull { it.supported }?.let { Selection(s.id, it.remarks) } }
        }
        for (s in subs) {
            val list = Subs.profiles(ctx, s.id).filter { it.supported }
            val p = list.firstOrNull { it.remarks == server }
                ?: list.firstOrNull { it.remarks.equals(server, ignoreCase = true) || it.name.equals(server, ignoreCase = true) }
            if (p != null) return Selection(s.id, p.remarks)
        }
        return null
    }
}

class ControlActivity : Activity() {
    override fun onResume() {
        super.onResume()
        if (isFinishing) return
        val r = runCatching { Control.run(this, intent) }.getOrDefault(Control.Result.NeedsApp)
        if (r == Control.Result.NeedsApp) startActivity(Intent(this, HomeActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }
}

class ShortcutActivity : Activity() {
    override fun onResume() {
        super.onResume()
        if (isFinishing) return
        val r = runCatching { Control.run(this, intent, own = true) }.getOrDefault(Control.Result.NeedsApp)
        if (r == Control.Result.NeedsApp) startActivity(Intent(this, HomeActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }
}

open class ControlReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        runCatching { Control.run(ctx, intent) }.onFailure { Diag.event(ctx, "remote", "error" to (it.javaClass.simpleName)) }
    }
}
