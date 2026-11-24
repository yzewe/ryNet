package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ServerDnsTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Test fun chosenResolverSurvivesStorageAndAppliesToDisconnectedChecks() {
        for (mode in ServerDns.entries) {
            val chosen = Settings(serverDns = mode, serverDnsCustom = "1.1.1.1 tls://dns.example")
            Settings.save(ctx, chosen)
            val saved = Settings.load(ctx)
            assertEquals(mode, saved.serverDns)
            assertEquals(chosen.serverDnsArg(), Tune.json(ctx, saved, chain = false, exit = null).getString("serverDns"))
            assertEquals(if (mode == ServerDns.Custom) "custom:1.1.1.1 tls://dns.example" else mode.key, saved.serverDnsArg())
        }
    }

    @Test fun networkBackupPreservesTheChosenResolver() {
        Settings.save(ctx, Settings(serverDns = ServerDns.Google))
        val snap = Backup.read(Backup.export(ctx, setOf(Backup.Part.Network), "test"))
        Settings.save(ctx, Settings(serverDns = ServerDns.System))
        val (_, restored) = Backup.apply(ctx, snap, setOf(Backup.Part.Network))
        assertEquals(ServerDns.Google, restored.serverDns)
        assertTrue(Settings(serverDns = ServerDns.System).tunnelDiffers(restored))
    }
}
