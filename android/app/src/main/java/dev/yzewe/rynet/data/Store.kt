package dev.yzewe.rynet.data

import android.content.Context
import java.io.File
import java.io.IOException

object Store {
    private fun file(ctx: Context, name: String) = File(ctx.filesDir, name)

    fun read(ctx: Context, name: String): String? = file(ctx, name).takeIf { it.exists() }?.readText()

    fun write(ctx: Context, name: String, text: String): Boolean {
        val tmp = file(ctx, "$name.tmp")
        return try {
            tmp.writeText(text)
            tmp.renameTo(file(ctx, name))
        } catch (e: IOException) {
            runCatching { tmp.delete() }
            false
        }
    }

    fun delete(ctx: Context, name: String) {
        file(ctx, name).delete()
    }

    const val SUB_BODY = "sub.json"
    const val SUB_META = "sub_meta.json"
    const val SELECTED = "selected.txt"
    const val STATE = "state.json"
}
