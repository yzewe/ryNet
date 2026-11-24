package dev.yzewe.rynet.ui

import androidx.compose.material3.Surface
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Store
import dev.yzewe.rynet.data.SubInfo
import dev.yzewe.rynet.data.Subscription
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "ru-w360dp-h800dp-mdpi")
class SubscriptionRulesUiTest {
    @get:Rule val rule = createComposeRule()

    @Test fun selectedSubscriptionIsPinnedAndOpensItsRules() {
        val ctx = ApplicationProvider.getApplicationContext<android.content.Context>()
        val info = SubInfo(
            "https://sub.example/a", "Example", 0, 0, 0, 0, 0, 1,
            routing = """{"Name":"Imported rules","ProxySites":["domain:video.example"],"DirectIp":["geoip:ru"]}"""
        )
        Store.write(ctx, "subs.json", JSONArray().put(JSONObject().put("id", "sample").put("info", Subscription.toJson(info))).toString())
        Store.write(ctx, "selection.json", JSONObject().put("sub", "sample").put("profile", "server").toString())
        rule.mainClock.autoAdvance = false
        rule.setContent { RynetTheme(false) { Surface { RulesScreen(Settings(), {}, {}, {}, {}) } } }
        rule.mainClock.advanceTimeBy(1600)
        rule.onNodeWithText("Imported rules").assertIsDisplayed().performClick()
        rule.waitForIdle()
        rule.mainClock.advanceTimeBy(1600)
        rule.waitForIdle()
        rule.mainClock.advanceTimeBy(1600)
        rule.onNodeWithText("domain:video.example").assertIsDisplayed()
        rule.onNodeWithText("geoip:ru").assertIsDisplayed()
    }
}
