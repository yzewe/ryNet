package dev.yzewe.rynet.data

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class LiveUpdateTest {
    private val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Test fun liveUpdateIsOnByDefaultAndForOldSettingsFiles() {
        assertTrue(Settings().liveUpdate)
        val old = Settings().toJson().apply { remove("liveUpdate") }
        assertTrue(Settings.fromJson(old).liveUpdate)
    }

    @Test fun choiceSurvivesStorage() {
        Settings.save(ctx, Settings(liveUpdate = false))
        assertFalse(Settings.load(ctx).liveUpdate)
        Settings.save(ctx, Settings(liveUpdate = true))
        assertTrue(Settings.load(ctx).liveUpdate)
    }

    @Test fun promotionIsRequestedOnlyFromAndroid16AndWhenEnabled() {
        val on = Settings()
        assertFalse(on.requestsLiveUpdate(35))
        assertTrue(on.requestsLiveUpdate(36))
        assertTrue(on.requestsLiveUpdate(37))
        assertFalse(Settings(liveUpdate = false).requestsLiveUpdate(36))
    }

    @Test fun togglingItRestartsTheNotification() {
        assertTrue(Settings().serviceDiffers(Settings(liveUpdate = false)))
    }
}
