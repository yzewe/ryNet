package dev.yzewe.rynet.ui

import android.graphics.Bitmap
import androidx.compose.material3.Surface
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.swipe
import dev.yzewe.rynet.data.Group
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.ServerSort
import dev.yzewe.rynet.data.ServerView
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.tr
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w960dp-h540dp-land-xhdpi")
class RemoteTest {
    @get:Rule val rule = createComposeRule()

    private fun capture(name: String) {
        val out = File(System.getenv("RYNET_SCREEN_DIR") ?: "build/screens/settings-circles").apply { mkdirs() }
        File(out, "$name.png").outputStream().use {
            rule.onRoot().captureToImage().asAndroidBitmap().compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    @Test fun backMovesIntoSettingsAndSelectsCategory() {
        var opened: SettingsPage? = null
        rule.mainClock.autoAdvance = false
        rule.setContent {
            LocalInputModeManager.current.requestInputMode(InputMode.Keyboard)
            RynetTheme(false) { Surface { SettingsScreen("1", onPage = { opened = it }, onDiag = {}, onBack = {}) } }
        }
        rule.mainClock.advanceTimeBy(1600)
        rule.onNodeWithContentDescription(tr("Назад", "Back")).performSemanticsAction(SemanticsActions.RequestFocus)
        rule.onRoot().performKeyInput { pressKey(Key.DirectionDown) }
        rule.onNodeWithText(SettingsPage.Network.title).assertIsFocused()
        rule.mainClock.advanceTimeBy(500)
        capture("remote_settings_focused")
        rule.onRoot().performKeyInput { pressKey(Key.DirectionCenter) }
        rule.runOnIdle { assertEquals(SettingsPage.Network, opened) }
    }

    @Test fun backMovesIntoSettingsPage() {
        var settings = Settings()
        rule.mainClock.autoAdvance = false
        rule.setContent {
            LocalInputModeManager.current.requestInputMode(InputMode.Keyboard)
            RynetTheme(false) { Surface { SettingsPageScreen(SettingsPage.Servers, settings, "", { settings = it }, {}, {}) } }
        }
        rule.mainClock.advanceTimeBy(1600)
        rule.onNodeWithContentDescription(tr("Назад", "Back")).performSemanticsAction(SemanticsActions.RequestFocus)
        rule.onRoot().performKeyInput { pressKey(Key.DirectionDown) }
        rule.onNodeWithContentDescription(tr("Назад", "Back")).assertIsNotFocused()
        rule.mainClock.advanceTimeBy(500)
        capture("remote_choice_focused")
        rule.onRoot().performKeyInput {
            pressKey(Key.DirectionRight)
            pressKey(Key.DirectionCenter)
        }
        rule.runOnIdle { assertEquals(ServerView.entries[1], settings.view) }
    }

    @Test fun choiceRetainsTouchAndDrag() {
        val selected = mutableStateOf(ServerView.entries[0])
        rule.setContent {
            RynetTheme(false) {
                Surface {
                    Choice(ServerView.entries, selected.value, { it.title }, { selected.value = it }, Modifier.testTag("choices"))
                }
            }
        }
        rule.onNodeWithText(ServerView.entries[1].title).performTouchInput { click() }
        rule.runOnIdle { assertEquals(ServerView.entries[1], selected.value) }
        rule.onNodeWithTag("choices").performTouchInput { swipe(centerRight, centerLeft) }
        rule.runOnIdle { assertEquals(ServerView.entries[0], selected.value) }
    }

    @Config(qualifiers = "w320dp-h480dp-xxhdpi")
    @Test
    fun scrollsServersAndActivatesWithRemote() {
        var selected = ""
        val profiles = (1..20).map { Profile("Server $it", "Server $it", "", Group.Main, "REALITY", true, emptyList()) }
        rule.setContent {
            LocalInputModeManager.current.requestInputMode(InputMode.Keyboard)
            RynetTheme(false) {
                Surface {
                    ServersContent(profiles, null, emptyMap(), false, ServerSort.Default, columns = 1, onPing = {}, onSelect = { selected = it.name })
                }
            }
        }
        rule.onNodeWithText("Server 1").performSemanticsAction(SemanticsActions.RequestFocus)
        repeat(19) { rule.onRoot().performKeyInput { pressKey(Key.DirectionDown) } }
        rule.onNodeWithText("Server 20").assertIsFocused()
        capture("remote_server_focused")
        rule.onRoot().performKeyInput { pressKey(Key.DirectionCenter) }
        rule.runOnIdle { assertEquals("Server 20", selected) }
    }
}
