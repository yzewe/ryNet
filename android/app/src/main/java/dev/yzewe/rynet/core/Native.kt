package dev.yzewe.rynet.core

import android.net.VpnService

object Native {
    init {
        System.loadLibrary("rynet")
        syncLanguage()
    }

    private external fun setEnglish(en: Boolean)

    fun syncLanguage() = setEnglish(!dev.yzewe.rynet.Lang.ru)

    external fun start(
        service: VpnService,
        tunFd: Int,
        config: String,
        profile: String,
        hosts: String,
        mtu: Int,
        geoDir: String,
        options: String
    ): Long

    external fun stop(handle: Long)

    external fun reconfigure(
        service: VpnService,
        handle: Long,
        config: String,
        profile: String,
        hosts: String,
        geoDir: String,
        options: String
    )

    external fun resolver(handle: Long): Long
    external fun resolve(resolver: Long, name: String): String?
    external fun freeResolver(resolver: Long)

    external fun addHosts(hosts: String)

    external fun bootstrapHosts(service: VpnService?, names: String, mode: String): String

    external fun setRoots(certs: Array<ByteArray>): Int

    external fun networkChanged(handle: Long, dns: String)

    external fun coreTid(handle: Long): Int

    external fun flushLog()

    external fun setQuiet(quiet: Boolean)

    external fun describe(text: String): String

    external fun smartRules(): String

    external fun importRules(text: String): String

    external fun details(text: String, remarks: String): String

    external fun ping(
        service: VpnService?,
        config: String,
        remarks: String,
        mode: Int,
        parallel: Int,
        timeoutMs: Int,
        sink: PingSink?,
        pingUrl: String,
        tune: String
    ): String

    external fun countries(service: VpnService?, config: String, remarks: String, parallel: Int, timeoutMs: Int, tune: String): String

    external fun speedtest(service: VpnService?, config: String, remarks: String, tune: String, sink: SpeedSink): String

    external fun pingCancel()

    external fun speedtestCancel()

    external fun speedtestReset()
}

interface SpeedSink {
    fun progress(phase: Int, value: Double, fraction: Double)

    fun server(json: String)
}

fun interface PingSink {
    fun result(name: String, ms: Int, error: String?)
}
