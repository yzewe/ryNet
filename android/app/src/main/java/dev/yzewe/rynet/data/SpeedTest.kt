package dev.yzewe.rynet.data

import android.content.Context
import android.content.Intent
import org.json.JSONArray
import org.json.JSONObject

data class SpeedResult(
    val at: Long,
    val name: String,
    val flag: String,
    val target: String,
    val sponsor: String,
    val ping: Double,
    val jitter: Double,
    val download: Double,
    val upload: Double
) {
    fun json(): JSONObject = JSONObject().put("at", at).put("name", name).put("flag", flag).put("target", target).put("sponsor", sponsor)
        .put("ping", ping).put("jitter", jitter).put("download", download).put("upload", upload)

    companion object {
        fun of(o: JSONObject) = SpeedResult(
            o.optLong("at"), o.optString("name"), o.optString("flag"), o.optString("target"), o.optString("sponsor"),
            o.optDouble("ping"), o.optDouble("jitter"), o.optDouble("download"), o.optDouble("upload")
        )
    }
}

data class SpeedRun(
    val phase: Phase = Phase.Idle,
    val value: Double = 0.0,
    val fraction: Double = 0.0,
    val ping: Double? = null,
    val jitter: Double? = null,
    val download: Double? = null,
    val upload: Double? = null,
    val server: String? = null,
    val error: String? = null,
    val result: SpeedResult? = null
) {
    enum class Phase { Idle, Servers, Ping, Download, Upload, Done, Error }

    val running get() = phase in setOf(Phase.Servers, Phase.Ping, Phase.Download, Phase.Upload)
}

object SpeedTest {
    const val ACTION = "dev.yzewe.rynet.SPEED"
    private const val FILE = "speedtests.json"
    private const val KEEP = 50

    fun history(ctx: Context): List<SpeedResult> = Store.read(ctx, FILE)?.let { t ->
        runCatching { JSONArray(t).let { a -> (0 until a.length()).map { SpeedResult.of(a.getJSONObject(it)) } } }.getOrNull()
    }.orEmpty()

    fun add(ctx: Context, r: SpeedResult) {
        val all = (listOf(r) + history(ctx)).take(KEEP)
        Store.write(ctx, FILE, JSONArray(all.map { it.json() }).toString())
    }

    fun clear(ctx: Context) = Store.write(ctx, FILE, "[]")

    fun place(server: JSONObject): String = listOf(server.optString("name"), server.optString("country")).filter { it.isNotBlank() }.joinToString(", ")

    fun publish(ctx: Context, r: SpeedRun) {
        val i = Intent(ACTION).setPackage(ctx.packageName)
            .putExtra("phase", r.phase.name).putExtra("value", r.value).putExtra("fraction", r.fraction)
            .putExtra("error", r.error).putExtra("server", r.server)
        r.ping?.let { i.putExtra("ping", it) }
        r.jitter?.let { i.putExtra("jitter", it) }
        r.download?.let { i.putExtra("download", it) }
        r.upload?.let { i.putExtra("upload", it) }
        r.result?.let { i.putExtra("result", it.json().toString()) }
        ctx.sendBroadcast(i)
    }

    fun read(i: Intent): SpeedRun {
        fun d(k: String) = if (i.hasExtra(k)) i.getDoubleExtra(k, 0.0) else null
        return SpeedRun(
            phase = runCatching { SpeedRun.Phase.valueOf(i.getStringExtra("phase").orEmpty()) }.getOrDefault(SpeedRun.Phase.Idle),
            value = i.getDoubleExtra("value", 0.0),
            fraction = i.getDoubleExtra("fraction", 0.0),
            ping = d("ping"), jitter = d("jitter"), download = d("download"), upload = d("upload"),
            server = i.getStringExtra("server"),
            error = i.getStringExtra("error"),
            result = i.getStringExtra("result")?.let { runCatching { SpeedResult.of(JSONObject(it)) }.getOrNull() }
        )
    }
}
