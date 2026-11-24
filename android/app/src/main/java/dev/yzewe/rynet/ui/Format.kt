package dev.yzewe.rynet.ui

import dev.yzewe.rynet.Lang
import dev.yzewe.rynet.tr
import java.text.DateFormat
import java.util.Date

private val RU get() = Lang.locale

fun date(ms: Long): String = DateFormat.getDateInstance(DateFormat.SHORT, RU).format(Date(ms))

fun time(ms: Long): String = DateFormat.getTimeInstance(DateFormat.SHORT, RU).format(Date(ms))

fun ago(ms: Long, now: Long = System.currentTimeMillis()): String {
    val min = (now - ms).coerceAtLeast(0) / 60_000
    return when {
        min < 1 -> tr("только что", "just now")
        min < 60 -> tr("$min мин назад", "$min min ago")
        min < 24 * 60 -> tr("${min / 60} ч назад", "${min / 60} h ago")
        else -> tr("${min / (24 * 60)} дн. назад", "${min / (24 * 60)} d ago")
    }
}

fun bytes(b: Long): String {
    fun num(v: Double) = if (v >= 10 || v % 1.0 == 0.0) "%.0f".format(RU, v) else "%.1f".format(RU, v)
    return when {
        b >= 1L shl 40 -> num(b / (1L shl 40).toDouble()) + tr(" ТБ", " TB")
        b >= 1L shl 30 -> num(b / (1L shl 30).toDouble()) + tr(" ГБ", " GB")
        b >= 1L shl 20 -> num(b / (1L shl 20).toDouble()) + tr(" МБ", " MB")
        else -> "${b / 1024}" + tr(" КБ", " KB")
    }
}

fun uptime(ms: Long): String {
    val s = (ms / 1000).coerceAtLeast(0)
    val d = s / 86400
    val h = s / 3600 % 24
    val m = s / 60 % 60
    val sec = s % 60
    return when {
        d > 0 -> tr("%d д %02d:%02d", "%dd %02d:%02d").format(RU, d, h, m)
        h > 0 -> "%d:%02d:%02d".format(RU, h, m, sec)
        else -> "%d:%02d".format(RU, m, sec)
    }
}
