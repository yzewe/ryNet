package dev.yzewe.rynet.ui

import android.content.Context
import android.content.Intent
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import dev.yzewe.rynet.R
import dev.yzewe.rynet.tr
import dev.yzewe.rynet.vpn.Control
import dev.yzewe.rynet.vpn.ShortcutActivity

object Shortcuts {
    const val EXTRA = "shortcut"
    const val REFRESH = "refresh"
    const val SPEED = "speed"

    fun publish(ctx: Context) {
        val app = ctx.applicationContext
        Thread({
            runCatching {
                fun home(extra: Intent.() -> Unit) = Intent(app, HomeActivity::class.java).setAction(Intent.ACTION_MAIN).apply(extra)
                val list = listOf(
                    item(
                        app,
                        "toggle",
                        tr("Вкл / выкл", "On / off"),
                        tr("Включить или выключить VPN", "Turn VPN on or off"),
                        R.drawable.sc_power,
                        Intent(app, ShortcutActivity::class.java).setAction(Control.TOGGLE)
                    ),
                    item(
                        app,
                        "servers",
                        tr("Серверы", "Servers"),
                        tr("Список серверов", "Server list"),
                        R.drawable.sc_servers,
                        home { putExtra(HomeActivity.EXTRA_SERVERS, true) }
                    ),
                    item(
                        app,
                        "refresh",
                        tr("Обновить", "Update"),
                        tr("Обновить подписки", "Update subscriptions"),
                        R.drawable.sc_refresh,
                        home { putExtra(EXTRA, REFRESH) }
                    ),
                    item(
                        app,
                        "speed",
                        tr("Скорость", "Speed"),
                        tr("Проверить скорость", "Test speed"),
                        R.drawable.sc_speed,
                        home { putExtra(EXTRA, SPEED) }
                    )
                )
                ShortcutManagerCompat.setDynamicShortcuts(app, list.take(ShortcutManagerCompat.getMaxShortcutCountPerActivity(app).coerceAtLeast(1)))
            }
        }, "rynet-shortcuts").start()
    }

    private fun item(ctx: Context, id: String, short: String, long: String, icon: Int, intent: Intent) = ShortcutInfoCompat.Builder(ctx, id)
        .setShortLabel(short)
        .setLongLabel(long)
        .setIcon(IconCompat.createWithResource(ctx, icon))
        .setIntent(intent)
        .build()
}
