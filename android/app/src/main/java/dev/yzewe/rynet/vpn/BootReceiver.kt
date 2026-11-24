package dev.yzewe.rynet.vpn

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.VpnService
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Subs

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        if (!Settings.load(ctx).autostart || RynetService.consent(ctx) != null) return
        val sel = Subs.selection(ctx) ?: return
        RynetService.connect(ctx, sel)
    }
}
