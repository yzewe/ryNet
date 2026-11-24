@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package dev.yzewe.rynet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Warning
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import dev.yzewe.rynet.tr

internal data class ErrorReason(val title: String, val hint: String, val details: String?)

internal fun errorReason(message: String?): ErrorReason {
    val m = message.orEmpty().trim()
    val update = tr("Обновите подписку или выберите другой сервер", "Update the subscription or choose another server")
    val other = tr("Выберите другой сервер", "Choose another server")
    val net = tr("Проверьте интернет или выберите другой сервер", "Check the internet or choose another server")
    val failed = tr("Не удалось подключиться", "Could not connect")
    fun r(title: String, hint: String) = ErrorReason(title, hint, m.takeIf { it.isNotEmpty() && it != title })
    fun has(vararg s: String) = s.any { m.contains(it, ignoreCase = true) }
    return when {
        m.isEmpty() -> ErrorReason(failed, net, null)

        has("разрешения на VPN", "VPN permission") -> ErrorReason(tr("VPN занят другим приложением", "Another app holds the VPN"), tr("Отключите другой VPN и повторите", "Turn off the other VPN and retry"), null)

        has("Не выбран сервер", "No server selected") -> ErrorReason(tr("Сервер не выбран", "No server selected"), tr("Выберите сервер в списке", "Choose a server in the list"), null)

        has("ключ REALITY", "REALITY key") -> r(tr("Сервер отклонил ключ", "The server rejected the key"), update)

        has("Неверный пароль", "неверный UUID", "Wrong password", "wrong UUID") -> r(tr("Сервер не принял пароль", "The server rejected the password"), update)

        has("Сертификат", "certificate") -> r(tr("Сертификат не прошёл проверку", "Certificate check failed"), update)

        has("отказал в TLS", "не по TLS", "ошибкой HTTP", "HTTP/2", "не так, как ждали", "rejected TLS", "answer with TLS", "HTTP error", "Unexpected server reply") ->
            r(tr("Сервер не принял настройки", "The server rejected the settings"), update)

        has("Разрыв TLS", "блокировк", "TLS dropped", "blocking") -> r(tr("Похоже на блокировку", "Looks like blocking"), tr("Попробуйте другой сервер или протокол", "Try another server or protocol"))

        has("Таймаут", "Timeout") -> r(tr("Сервер не отвечает", "The server does not respond"), net)

        has("отклонил соединение", "refused the connection") -> r(tr("Сервер не принимает соединения", "The server refuses connections"), other)

        has("закрыл соединение", "оборвал поток", "Соединение прервано", "closed the connection", "dropped the stream", "Connection aborted") ->
            r(tr("Сервер оборвал соединение", "The server dropped the connection"), update)

        has("Нет сети", "Нет доступа к сети", "No network") -> r(tr("Нет сети", "No network"), tr("Проверьте подключение к интернету", "Check your internet connection"))

        has("недоступен", "unreachable", "not available") -> r(tr("Нет связи с сервером", "Cannot reach the server"), net)

        has("(DNS)") -> r(tr("Адрес сервера не найден", "Server address not found"), tr("Проверьте интернет или обновите подписку", "Check the internet or update the subscription"))

        has("Неправильная конфигурация", "Invalid configuration") -> r(tr("Сервер настроен неверно", "The server is misconfigured"), update)

        has("Не поддерживается", "not supported") -> r(tr("Сервер не поддерживается", "The server is not supported"), other)

        has("Ядро остановилось", "Core stopped") -> r(tr("Подключение прервалось", "The connection was interrupted"), tr("Попробуйте ещё раз", "Try again"))

        else -> {
            val head = m.substringBefore(": ").removePrefix("Ошибка").removePrefix("Error").trim().ifEmpty { failed }
            ErrorReason(head.replaceFirstChar { it.uppercase() }, net, m.takeIf { it != head })
        }
    }
}

@Composable
internal fun ConnectError(message: String?, onRetry: () -> Unit, onServers: () -> Unit, modifier: Modifier = Modifier, align: Alignment.Horizontal = Alignment.CenterHorizontally) {
    val reason = remember(message) { errorReason(message) }
    Column(modifier.fillMaxWidth(), horizontalAlignment = align) {
        ReasonCard(reason)
        Spacer(Modifier.height(16.dp))
        RetryActions(onRetry, onServers)
    }
}

@Composable
internal fun RetryActions(onRetry: () -> Unit, onServers: () -> Unit, modifier: Modifier = Modifier) {
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        PillButton(tr("Повторить", "Retry"), onRetry, haptic = Haptic.Confirm, leading = { Icon(Icons.Rounded.Refresh, null, modifier = Modifier.size(22.dp)) })
        PillButton(tr("Другой сервер", "Another server"), onServers, accent = false)
    }
}

@Composable
private fun ReasonCard(reason: ErrorReason) {
    val c = MaterialTheme.colorScheme
    var open by rememberSaveable(reason) { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open) 180f else 0f, MaterialTheme.motionScheme.fastSpatialSpec(), label = "more")
    Column(
        Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.extraLarge)
            .background(c.errorContainer)
            .padding(16.dp)
    ) {
        Row(verticalAlignment = Alignment.Top) {
            Icon(Icons.Rounded.Warning, null, tint = c.onErrorContainer, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(reason.title, style = MaterialTheme.typography.titleSmall, color = c.onErrorContainer)
                Text(reason.hint, style = MaterialTheme.typography.bodyMedium, color = c.onErrorContainer)
            }
        }
        val details = reason.details ?: return@Column
        AnimatedVisibility(open, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Text(
                details,
                style = MaterialTheme.typography.bodySmall,
                color = c.onErrorContainer,
                modifier = Modifier.padding(start = 32.dp, top = 8.dp)
            )
        }
        Row(
            Modifier
                .padding(start = 20.dp, top = 4.dp)
                .clip(MaterialTheme.shapes.small)
                .clickable(role = Role.Button, onClickLabel = if (open) tr("Скрыть", "Hide") else tr("Подробнее", "Details")) { open = !open }
                .padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            Text(if (open) tr("Скрыть", "Hide") else tr("Подробнее", "Details"), style = MaterialTheme.typography.labelLarge, color = c.onErrorContainer)
            Icon(Icons.Rounded.KeyboardArrowDown, null, tint = c.onErrorContainer, modifier = Modifier.size(18.dp).rotate(turn))
        }
    }
}
