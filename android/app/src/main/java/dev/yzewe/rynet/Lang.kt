package dev.yzewe.rynet

import java.util.Locale

object Lang {
    private val RUSSIAN = setOf("ru", "be", "kk")

    val ru: Boolean get() = Locale.getDefault().language in RUSSIAN

    val locale: Locale get() = if (ru) Locale.forLanguageTag("ru") else Locale.ENGLISH
}

fun tr(ru: String, en: String): String = if (Lang.ru) ru else en

fun plural(n: Int, one: String, few: String, many: String, en: String, enMany: String = en + "s"): String {
    if (!Lang.ru) return "$n ${if (n == 1) en else enMany}"
    val m10 = n % 10
    val m100 = n % 100
    val w = when {
        m10 == 1 && m100 != 11 -> one
        m10 in 2..4 && m100 !in 12..14 -> few
        else -> many
    }
    return "$n $w"
}
