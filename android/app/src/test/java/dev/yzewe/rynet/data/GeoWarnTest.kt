package dev.yzewe.rynet.data

import org.junit.Assert.assertEquals
import org.junit.Test

class GeoWarnTest {
    private val both = setOf("geosite-category-ru.list", "geoip-ru.list")

    @Test
    fun smartWithoutFilesWarns() {
        assertEquals(true, GeoWarn.needed(RoutingMode.Smart, emptySet()))
        assertEquals(true, GeoWarn.needed(RoutingMode.Smart, setOf("geoip-ru.list")))
        assertEquals(true, GeoWarn.needed(RoutingMode.Smart, setOf("geosite-category-ru.list")))
    }

    @Test
    fun smartWithFilesOrOtherModesDoesNot() {
        assertEquals(false, GeoWarn.needed(RoutingMode.Smart, both))
        for (m in listOf(RoutingMode.Subscription, RoutingMode.Global, RoutingMode.Custom)) assertEquals(false, GeoWarn.needed(m, emptySet()))
    }
}
