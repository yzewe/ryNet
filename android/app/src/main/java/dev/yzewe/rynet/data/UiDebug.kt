package dev.yzewe.rynet.data

import android.content.Context
import android.os.Build
import java.io.File
import java.time.Instant
import org.json.JSONObject

object UiDebug {
    @Volatile
    var enabled = false

    private const val ROTATE_AT = 2L shl 20

    fun log(ctx: Context, ev: String, vararg kv: Pair<String, Any?>) {
        if (!enabled) return
        val o = JSONObject().put("t", Instant.now().toString()).put("ev", ev)
        for ((k, v) in kv) o.put(k, v ?: JSONObject.NULL)
        write(ctx, o)
    }

    fun device(): Array<Pair<String, Any?>> = arrayOf(
        "device" to "${Build.MANUFACTURER} ${Build.MODEL}",
        "brand" to Build.BRAND,
        "android" to "${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
        "rom" to Build.DISPLAY
    )

    @Synchronized
    private fun write(ctx: Context, o: JSONObject) {
        val dir = Diag.dir(ctx)
        val f = File(dir, "ui.log")
        if (f.length() > ROTATE_AT) f.renameTo(File(dir, "ui.1.log"))
        f.appendText(o.toString() + "\n")
    }
}
