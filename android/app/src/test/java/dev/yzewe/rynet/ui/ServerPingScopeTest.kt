package dev.yzewe.rynet.ui

import androidx.compose.material3.Surface
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import dev.yzewe.rynet.data.Group
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.tr
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "ru-w360dp-h800dp-mdpi")
class ServerPingScopeTest {
    @get:Rule val rule = createComposeRule()

    @Test fun buttonUsesCurrentSearchAndKeepsEmptyResultsEmpty() {
        val query = mutableStateOf("герм")
        val calls = mutableListOf<List<String>?>()
        val profiles = listOf(
            Profile("de", "Германия", "", Group.Main, "REALITY", true, emptyList()),
            Profile("nl", "Нидерланды", "", Group.Main, "REALITY", true, emptyList()),
            Profile("old", "Германия old", "", Group.Main, "unknown", false, emptyList())
        )
        rule.mainClock.autoAdvance = false
        rule.setContent {
            RynetTheme(false) {
                Surface {
                    ServersContent(profiles, null, emptyMap(), false, ServerSort.Default, query = query.value, onPing = { calls += it }, onSelect = {})
                }
            }
        }
        rule.mainClock.advanceTimeBy(1600)
        val ping = rule.onNodeWithContentDescription(tr("Проверить пинг", "Check ping"))
        ping.performClick()
        rule.runOnIdle {
            assertEquals(listOf("de"), calls.last())
            query.value = "япония"
        }
        rule.mainClock.advanceTimeBy(500)
        ping.performClick()
        rule.runOnIdle {
            assertEquals(emptyList<String>(), calls.last())
            query.value = ""
        }
        rule.mainClock.advanceTimeBy(500)
        ping.performClick()
        rule.runOnIdle { assertEquals(null, calls.last()) }
    }
}
