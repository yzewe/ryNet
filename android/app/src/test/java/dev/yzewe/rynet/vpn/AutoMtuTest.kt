package dev.yzewe.rynet.vpn

import dev.yzewe.rynet.data.Settings
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AutoMtuTest {
    @Test
    fun huaweiAndHonorGetSafeMtuOthersKeepDefault() {
        assertEquals(RynetService.HUAWEI_MTU, RynetService.autoMtu("HUAWEI", "HUAWEI"))
        assertEquals(RynetService.HUAWEI_MTU, RynetService.autoMtu("HONOR", "Honor"))
        assertEquals(RynetService.HUAWEI_MTU, RynetService.autoMtu("unknown", "huawei"))
        assertEquals(RynetService.MTU, RynetService.autoMtu("samsung", "samsung"))
        assertEquals(RynetService.MTU, RynetService.autoMtu(null, null))
    }

    @Test
    fun explicitUserMtuWinsOverVendorDefault() {
        val auto = RynetService.autoMtu("HUAWEI", "HUAWEI")
        assertEquals(1400, Settings(mtu = 1400).tunMtu(auto))
        assertEquals(1500, Settings(mtu = 0).tunMtu(auto))
    }
}
