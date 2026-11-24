package dev.yzewe.rynet.vpn

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.VpnService
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import androidx.core.content.ContextCompat
import dev.yzewe.rynet.R
import dev.yzewe.rynet.data.AppIcon
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.ui.HomeActivity

open class QuickTile : TileService() {
    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = render(VpnState.load(context))
    }
    private var listening = false

    override fun onStartListening() {
        ContextCompat.registerReceiver(this, receiver, IntentFilter(VpnState.ACTION), ContextCompat.RECEIVER_NOT_EXPORTED)
        listening = true
        render(VpnState.load(this))
    }

    override fun onStopListening() {
        if (listening) unregisterReceiver(receiver)
        listening = false
    }

    override fun onClick() {
        val state = VpnState.load(this)
        if (state.status == Status.On || state.status == Status.Connecting) {
            RynetService.disconnect(this, "tile")
            render(VpnState(Status.Off))
            return
        }
        val sel = Subs.selection(this)
        if (sel == null || RynetService.consent(this) != null) {
            openApp()
            return
        }
        RynetService.connect(this, sel)
        render(VpnState(Status.Connecting, sel.profile))
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openApp() {
        val intent = Intent(this, HomeActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE))
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }

    private fun render(state: VpnState) {
        val tile = qsTile ?: return
        tile.icon = android.graphics.drawable.Icon.createWithResource(this, AppIcon.notif(this))
        tile.state = when (state.status) {
            Status.On, Status.Connecting -> Tile.STATE_ACTIVE
            else -> Tile.STATE_INACTIVE
        }
        if (Build.VERSION.SDK_INT >= 29) {
            tile.subtitle = when (state.status) {
                Status.Connecting -> getString(R.string.status_connecting)
                Status.Error -> getString(R.string.status_error)
                else -> state.profile ?: Subs.selection(this)?.profile
            }
        }
        tile.updateTile()
    }
}
