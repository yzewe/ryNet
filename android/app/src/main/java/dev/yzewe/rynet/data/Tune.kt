package dev.yzewe.rynet.data

import android.content.Context
import org.json.JSONObject

object Tune {
    fun json(ctx: Context, s: Settings, chain: Boolean, exit: Selection?, servers: Boolean = true): JSONObject {
        val o = JSONObject().put("serverDns", s.serverDnsArg())
        val entry = s.chainEntry()
        if (chain && entry != null && entry != exit) {
            Subs.body(ctx, entry.sub)?.let { body ->
                o.put("chain", JSONObject().put("config", body).put("profile", entry.profile))
            }
        }
        val list = org.json.JSONArray()
        if (servers) {
            for ((sel, tag) in Rules.servers(s.rules, s.sets)) {
                val body = Subs.body(ctx, sel.sub) ?: continue
                list.put(JSONObject().put("config", body).put("profile", sel.profile).put("tag", tag))
            }
        }
        if (list.length() > 0) o.put("servers", list)
        if (s.fragment) {
            o.put("fragment", JSONObject().put("length", s.fragmentLength.trim()).put("interval", s.fragmentInterval.trim()))
        }
        if (s.background) o.put("keepalive", true)
        return o
    }

    fun entry(ctx: Context, s: Settings, exit: Selection?): Profile? {
        val e = s.chainEntry()?.takeIf { it != exit } ?: return null
        return Subs.body(ctx, e.sub)?.let { body -> Subscription.parse(body).byRemarks(e.profile) }
    }
}
