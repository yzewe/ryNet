package dev.yzewe.rynet.data

import android.content.Context
import java.io.File

object GeoWarn {
    private val FILES = listOf("geosite-category-ru.list", "geoip-ru.list")

    fun needed(mode: RoutingMode, present: Set<String>): Boolean = mode == RoutingMode.Smart && !present.containsAll(FILES)

    fun needed(ctx: Context, mode: RoutingMode): Boolean {
        val dir = Geo.dir(ctx)
        return needed(mode, FILES.filter { File(dir, it).length() > 0 }.toSet())
    }

    fun download(ctx: Context): Boolean {
        runCatching { Geo.update(ctx, "") }
        return !needed(ctx, RoutingMode.Smart)
    }
}
