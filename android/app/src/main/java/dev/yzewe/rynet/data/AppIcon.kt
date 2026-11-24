package dev.yzewe.rynet.data

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import dev.yzewe.rynet.R
import dev.yzewe.rynet.tr

enum class AppIcon(private val ruTitle: String, private val enTitle: String, private val alias: String, val res: Int, val notif: Int) {
    Classic("Классика", "Classic", "MainActivity", R.mipmap.ic_launcher, R.drawable.ic_shield),
    Cat("Котик", "Kitty", "IconCat", R.mipmap.ic_launcher_cat, R.drawable.ic_notif_cat),
    Bsod("Синий экран", "Blue screen", "IconBsod", R.mipmap.ic_launcher_bsod, R.drawable.ic_notif_bsod),
    Offline("Нет сети", "No network", "IconOffline", R.mipmap.ic_launcher_offline, R.drawable.ic_notif_offline),
    Layout("Макет", "Wireframe", "IconLayout", R.mipmap.ic_launcher_layout, R.drawable.ic_notif_layout),
    G26("Google ’26", "Google ’26", "IconG26", R.mipmap.ic_launcher_g26, R.drawable.ic_notif_g26),
    Aurora("Аврора", "Aurora", "IconAurora", R.mipmap.ic_launcher_aurora, R.drawable.ic_notif_aurora);

    val title: String get() = tr(ruTitle, enTitle)

    private fun component(ctx: Context) = ComponentName(ctx.packageName, "dev.yzewe.rvpn.ui.$alias")

    private fun enabled(ctx: Context): Boolean = when (ctx.packageManager.getComponentEnabledSetting(component(ctx))) {
        PackageManager.COMPONENT_ENABLED_STATE_ENABLED -> true
        PackageManager.COMPONENT_ENABLED_STATE_DEFAULT -> this == Classic
        else -> false
    }

    companion object {
        fun current(ctx: Context): AppIcon = entries.firstOrNull { it.enabled(ctx) } ?: Classic

        fun notif(ctx: Context): Int = runCatching { current(ctx).notif }.getOrDefault(R.drawable.ic_shield)

        fun apply(ctx: Context, icon: AppIcon) {
            val pm = ctx.packageManager
            val flags = PackageManager.DONT_KILL_APP
            pm.setComponentEnabledSetting(
                icon.component(ctx),
                if (icon == Classic) PackageManager.COMPONENT_ENABLED_STATE_DEFAULT else PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
                flags
            )
            for (other in entries) {
                if (other == icon) continue
                pm.setComponentEnabledSetting(
                    other.component(ctx),
                    if (other == Classic) PackageManager.COMPONENT_ENABLED_STATE_DISABLED else PackageManager.COMPONENT_ENABLED_STATE_DEFAULT,
                    flags
                )
            }
            runCatching { dev.yzewe.rynet.vpn.RynetService.iconChanged(ctx) }
            runCatching { dev.yzewe.rynet.widget.ToggleWidget.refresh(ctx) }
            runCatching {
                ctx.startActivity(
                    Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER).setComponent(icon.component(ctx))
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
                )
            }
        }
    }
}
