package dev.yzewe.rynet

import android.app.Application
import dev.yzewe.rynet.data.Diag

class RynetApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Diag.catchCrashes(this)
    }
}
