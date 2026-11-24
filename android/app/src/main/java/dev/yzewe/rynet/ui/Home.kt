package dev.yzewe.rynet.ui

import androidx.compose.ui.graphics.ImageBitmap
import dev.yzewe.rynet.data.ExitIp
import dev.yzewe.rynet.data.ExpiryAlert
import dev.yzewe.rynet.data.Profile
import dev.yzewe.rynet.data.ServerInfo
import dev.yzewe.rynet.data.SubInfo
import dev.yzewe.rynet.data.VpnState

data class HomeData(
    val title: String,
    val icon: ImageBitmap? = null,
    val sub: SubInfo?,
    val vpn: VpnState,
    val profile: Profile?,
    val ping: Int?,
    val info: ServerInfo = ServerInfo.Description,
    val alert: ExpiryAlert? = null,
    val geoWarn: Boolean = false,
    val geoBusy: Boolean = false,
    val exit: ExitIp? = null,
    val via: Profile? = null,
    val chain: Boolean = false,
    val chainOn: Boolean = false,
    val pingWaiting: Boolean = false,
    val subIndex: Int = 0,
    val subCount: Int = 1,
    val quickServers: () -> List<SlideItem> = { emptyList() },
    val quickSubs: () -> List<SlideItem> = { emptyList() },
    val autoOn: Boolean = false,
    val autoSub: String? = null
)

class HomeActions(
    val onToggle: () -> Unit = {},
    val onServers: () -> Unit = {},
    val onSubs: () -> Unit = {},
    val onSettings: () -> Unit = {},
    val onSpeed: () -> Unit = {},
    val onChain: () -> Unit = {},
    val onChainSwitch: (Boolean) -> Unit = {},
    val onDismissAlert: (String) -> Unit = {},
    val onGeoDownload: () -> Unit = {},
    val onPickServer: (String) -> Unit = {},
    val onPickSub: (String) -> Unit = {},
    val onSubStep: (Int) -> Unit = {},
    val onReconnect: () -> Unit = {},
    val onAutoSwitch: (Boolean) -> Unit = {},
    val onPing: () -> Unit = {},
    val onAutoSettings: () -> Unit = {}
)
