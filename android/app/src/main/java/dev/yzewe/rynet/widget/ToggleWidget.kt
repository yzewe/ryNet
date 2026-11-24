package dev.yzewe.rynet.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.service.quicksettings.TileService
import android.widget.RemoteViews
import dev.yzewe.rynet.R
import dev.yzewe.rynet.data.AppIcon
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.Subs
import dev.yzewe.rynet.data.VpnState
import dev.yzewe.rynet.ui.HomeActivity
import dev.yzewe.rynet.vpn.RynetService

open class ToggleWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, mgr: AppWidgetManager, ids: IntArray) {
        mgr.updateAppWidget(ids, views(ctx))
    }

    override fun onReceive(ctx: Context, intent: Intent) {
        if (intent.action == ACTION_TOGGLE) toggle(ctx) else super.onReceive(ctx, intent)
    }

    private fun toggle(ctx: Context) {
        val s = VpnState.load(ctx)
        if (s.status == Status.On || s.status == Status.Connecting) {
            RynetService.disconnect(ctx, "widget")
            return
        }
        Subs.selection(ctx)?.let { RynetService.connect(ctx, it) }
    }

    companion object {
        private const val ACTION_TOGGLE = "dev.yzewe.rynet.WIDGET_TOGGLE"

        internal fun views(ctx: Context): RemoteViews {
            val s = VpnState.load(ctx)
            val on = s.status == Status.On
            val busy = s.status == Status.Connecting
            val v = RemoteViews(ctx.packageName, R.layout.widget_toggle)
            v.setInt(R.id.widget_root, "setBackgroundResource", if (on) R.drawable.widget_bg_on else R.drawable.widget_bg_off)
            v.setInt(R.id.widget_badge, "setBackgroundResource", if (on) R.drawable.widget_badge_on else R.drawable.widget_badge_off)
            v.setTextViewText(
                R.id.widget_title,
                ctx.getString(
                    when {
                        on -> R.string.status_on
                        busy -> R.string.status_connecting
                        else -> R.string.status_off
                    }
                )
            )
            val server = (s.profile ?: Subs.selection(ctx)?.profile).orEmpty()
            v.setTextViewText(R.id.widget_subtitle, server)
            v.setTextColor(R.id.widget_title, ctx.getColor(if (on) R.color.widget_on_fg else R.color.widget_off_fg))
            v.setTextColor(R.id.widget_subtitle, ctx.getColor(if (on) R.color.widget_on_fg2 else R.color.widget_off_fg2))
            v.setImageViewResource(R.id.widget_icon, AppIcon.notif(ctx))
            v.setInt(R.id.widget_icon, "setColorFilter", ctx.getColor(if (on) R.color.widget_badge_on_fg else R.color.widget_badge_off_fg))
            val needsApp = !on && !busy && (Subs.selection(ctx) == null || dev.yzewe.rynet.vpn.RynetService.consent(ctx) != null)
            val click = if (needsApp) {
                PendingIntent.getActivity(ctx, 0, Intent(ctx, HomeActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
            } else {
                PendingIntent.getBroadcast(
                    ctx,
                    0,
                    Intent(ctx, dev.yzewe.rvpn.widget.ToggleWidget::class.java).setAction(ACTION_TOGGLE),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
                )
            }
            v.setOnClickPendingIntent(R.id.widget_root, click)
            return v
        }

        fun refresh(ctx: Context) {
            val mgr = AppWidgetManager.getInstance(ctx) ?: return
            val ids = mgr.getAppWidgetIds(ComponentName(ctx, dev.yzewe.rvpn.widget.ToggleWidget::class.java))
            if (ids.isNotEmpty()) mgr.updateAppWidget(ids, views(ctx))
            runCatching { TileService.requestListeningState(ctx, ComponentName(ctx, dev.yzewe.rvpn.vpn.QuickTile::class.java)) }
        }
    }
}
