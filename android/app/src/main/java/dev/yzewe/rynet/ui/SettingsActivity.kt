package dev.yzewe.rynet.ui

import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import dev.yzewe.rynet.data.Hwid
import dev.yzewe.rynet.data.Settings
import dev.yzewe.rynet.data.Status
import dev.yzewe.rynet.data.VpnState

class SettingsActivity : ComponentActivity() {
    private var settings by mutableStateOf(Settings())

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        settings = Settings.load(this)
        val route = intent.getStringExtra(EXTRA_ROUTE) ?: ROOT
        setContent {
            AppFrame(settings) { Route(route) }
        }
    }

    override fun onStart() {
        super.onStart()
        settings = Settings.load(this)
    }

    private fun change(new: Settings) {
        Settings.change(this, settings, new)
        settings = new
    }

    private fun open(route: String) = startActivity(intent(this, route))

    @androidx.compose.runtime.Composable
    private fun Route(route: String) {
        when {
            route == ROOT -> {
                val version = remember { runCatching { packageManager.getPackageInfo(packageName, 0).versionName }.getOrNull().orEmpty() }
                SettingsScreen(
                    version = version,
                    onPage = { open(PAGE + it.name) },
                    onDiag = { open(DIAG) },
                    onBack = ::finish
                )
            }

            route.startsWith(PAGE) -> {
                val page = SettingsPage.entries.firstOrNull { it.name == route.removePrefix(PAGE) } ?: SettingsPage.Network
                SettingsPageScreen(
                    page,
                    settings,
                    defaultHwid = remember { Hwid.default(this) },
                    onChange = ::change,
                    onOpen = { if (it == Screen.Rules) open(RULES) },
                    onBack = ::finish
                )
            }

            route == RULES -> RulesScreen(
                settings,
                onChange = ::change,
                onRaw = { open(RULES_TEXT) },
                onSet = { open(SET + it.orEmpty()) },
                onBack = ::finish,
                incoming = remember { intent.getStringExtra(EXTRA_SET_LINK) }
            )

            route.startsWith(SET) -> RuleSetScreen(route.removePrefix(SET).ifEmpty { null }, settings, onChange = ::change, onBack = ::finish)

            route == RULES_TEXT -> RulesTextScreen(
                settings.userRules,
                onSave = {
                    change(settings.copy(userRules = it))
                    finish()
                },
                onBack = ::finish
            )

            route == APPEARANCE -> AppearanceScreen(settings, onChange = ::change, onBack = ::finish)

            route == DIAG -> DiagScreen(settings, VpnState.load(this).status == Status.On, onChange = ::change, onBack = ::finish)
        }
    }

    companion object {
        private const val EXTRA_ROUTE = "route"
        private const val ROOT = "root"
        private const val PAGE = "page:"
        private const val RULES = "rules"
        private const val RULES_TEXT = "rules-text"
        private const val SET = "set:"
        private const val EXTRA_SET_LINK = "set-link"
        private const val DIAG = "diag"
        private const val APPEARANCE = "appearance"

        fun page(p: SettingsPage): String = PAGE + p.name

        fun intent(ctx: Context, route: String = ROOT): Intent = Intent(ctx, SettingsActivity::class.java).putExtra(EXTRA_ROUTE, route)

        fun appearance(ctx: Context): Intent = intent(ctx, APPEARANCE)

        fun diag(ctx: Context): Intent = intent(ctx, DIAG)

        fun importSet(ctx: Context, link: String): Intent = intent(ctx, RULES).putExtra(EXTRA_SET_LINK, link)
    }
}
