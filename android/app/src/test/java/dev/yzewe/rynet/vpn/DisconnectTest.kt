package dev.yzewe.rynet.vpn

import android.content.ComponentName
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.VpnState
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class DisconnectTest {
    private val app = ApplicationProvider.getApplicationContext<Context>()

    @Test
    fun disconnectFromBackgroundWithoutServiceClearsStaleState() {
        VpnState.publish(app, VpnState(Status.On, "Германия"))
        val background = object : ContextWrapper(app) {
            override fun startService(service: Intent?): ComponentName? = throw IllegalStateException("Not allowed to start service: app is in background")
        }
        RynetService.disconnect(background, "tile")
        assertEquals(Status.Off, VpnState.load(app).status)
    }
}
