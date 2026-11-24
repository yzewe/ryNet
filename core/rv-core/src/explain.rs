use std::io;

use crate::Error;
use crate::lang::t;

pub fn explain(e: &Error) -> String {
    match e {
        Error::Config(m) => return format!("{}: {}", t("Неправильная конфигурация", "Invalid configuration"), short(m)),
        Error::Unsupported(m) if m == "tcp ping of udp protocol" => {
            return t("TCP-пинг не подходит для UDP-протокола", "TCP ping doesn't work for a UDP protocol").into();
        }
        Error::Unsupported(m) => return format!("{}: {}", t("Не поддерживается", "Not supported"), short(m)),
        Error::Io(io) => {
            if let Some(s) = by_kind(io) {
                return s.to_string();
            }
        }
        Error::Tls(_) => {}
    }
    let text = e.to_string();
    by_text(&text.to_ascii_lowercase()).map(str::to_string).unwrap_or_else(|| format!("{}: {}", t("Ошибка", "Error"), short(&text)))
}

fn by_kind(e: &io::Error) -> Option<&'static str> {
    use io::ErrorKind::*;
    Some(match e.kind() {
        TimedOut => t("Таймаут: сервер не ответил вовремя", "Timeout: the server did not answer in time"),
        ConnectionRefused => t("Сервер отклонил соединение: порт закрыт", "The server refused the connection: port closed"),
        ConnectionReset => t("Соединение сброшено: похоже на блокировку", "Connection reset: looks like blocking"),
        ConnectionAborted => t("Соединение прервано", "Connection aborted"),
        HostUnreachable | NetworkUnreachable => t("Сервер недоступен из этой сети", "The server is unreachable from this network"),
        NetworkDown => t("Нет сети", "No network"),
        AddrNotAvailable => t("Адрес сервера недоступен", "Server address is not available"),
        PermissionDenied => t("Нет доступа к сети", "No network access"),
        UnexpectedEof => t("Сервер закрыл соединение без ответа", "The server closed the connection without answering"),
        _ => return None,
    })
}

fn by_text(text: &str) -> Option<&'static str> {
    let has = |s: &str| text.contains(s);
    Some(if has(crate::ping::SLOW) {
        t("Слишком медленно: данные почти не проходят", "Too slow: data barely gets through")
    } else if has("reality authentication failed") {
        t(
            "Неверный ключ REALITY: сервер не принял publicKey, shortId или SNI",
            "Wrong REALITY key: the server rejected publicKey, shortId or SNI",
        )
    } else if has("certificate verification failed") || has("badcertificate") || has("unknownissuer") {
        t("Сертификат сервера не прошёл проверку", "Server certificate verification failed")
    } else if has("tls alert") {
        t("Сервер отказал в TLS: неверный SNI или отпечаток", "The server rejected TLS: wrong SNI or fingerprint")
    } else if has("record too long") || has("unexpected record") || has("bad record") {
        t("Сервер ответил не по TLS: неверный порт, протокол или SNI", "The server did not answer with TLS: wrong port, protocol or SNI")
    } else if has("closed during handshake") || has("protocol violation") {
        t(
            "Разрыв TLS: соединение оборвалось на рукопожатии (часто — блокировка)",
            "TLS dropped: the connection broke during the handshake (often blocking)",
        )
    } else if has("decrypt failed") {
        t(
            "Неверный пароль: Shadowsocks не расшифровал ответ (пароль или метод)",
            "Wrong password: Shadowsocks could not decrypt the reply (password or method)",
        )
    } else if has("hysteria: auth failed") {
        t("Неверный пароль: сервер Hysteria его не принял", "Wrong password: the Hysteria server rejected it")
    } else if has("no response header") || has("no http response") {
        t(
            "Сервер закрыл соединение без ответа: неверный UUID/пароль или сервер не выпускает в интернет",
            "The server closed the connection without answering: wrong UUID/password or the server has no internet access",
        )
    } else if has("not select h2") {
        t("Сервер не поддерживает HTTP/2 (XHTTP/gRPC)", "The server does not support HTTP/2 (XHTTP/gRPC)")
    } else if has("websocket: bad handshake") || has("httpupgrade: unrecognized reply") || has("http ") && has(" 404") {
        t("Сервер ответил ошибкой HTTP: неверный путь или хост", "The server answered with an HTTP error: wrong path or host")
    } else if has("failed to lookup") || has("no address") || has("resolved to no addresses") || has("no usable address") {
        t("Не удалось найти адрес сервера (DNS)", "Could not find the server address (DNS)")
    } else if has("timed out") || has("timeout") {
        t("Таймаут: сервер не ответил вовремя", "Timeout: the server did not answer in time")
    } else if has("connection refused") {
        t("Сервер отклонил соединение: порт закрыт", "The server refused the connection: port closed")
    } else if has("connection reset") || has("reset by peer") {
        t("Соединение сброшено: похоже на блокировку", "Connection reset: looks like blocking")
    } else if has("network is unreachable") || has("no route to host") {
        t("Сервер недоступен из этой сети", "The server is unreachable from this network")
    } else if has("broken pipe") || has("unexpected eof") || has("early eof") {
        t("Сервер закрыл соединение без ответа", "The server closed the connection without answering")
    } else if has("stream error") || has("internal error") {
        t("Сервер оборвал поток (ошибка на стороне сервера)", "The server dropped the stream (server-side error)")
    } else if has("unexpected http status") || has("bad status") {
        t("Сервер ответил не так, как ждали: проверка не прошла", "Unexpected server reply: the check failed")
    } else {
        return None;
    })
}

fn short(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() > 90 { format!("{}…", line.chars().take(90).collect::<String>()) } else { line.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases() {
        let io = |k| Error::Io(io::Error::from(k));
        assert!(explain(&io(io::ErrorKind::TimedOut)).starts_with("Таймаут"));
        assert!(explain(&io(io::ErrorKind::ConnectionRefused)).contains("отклонил"));
        let wrapped = Error::Io(io::Error::other("tls: REALITY authentication failed (server is not our xray, or wrong keys)"));
        assert!(explain(&wrapped).starts_with("Неверный ключ REALITY"));
        let hs = Error::Io(io::Error::other("tls: protocol violation: connection closed during handshake"));
        assert!(explain(&hs).starts_with("Разрыв TLS"));
        assert!(explain(&Error::Config("vless: no users".into())).starts_with("Неправильная конфигурация"));
        assert!(explain(&Error::Io(io::Error::other("weird thing"))).starts_with("Ошибка: "));
        let slow = Error::Io(io::Error::other(format!("{}: 9000 of 32768 bytes in 3000 ms", crate::ping::SLOW)));
        assert!(explain(&slow).starts_with("Слишком медленно"));
        let udp = Error::Unsupported("tcp ping of udp protocol".into());
        assert_eq!(explain(&udp), "TCP-пинг не подходит для UDP-протокола");
        crate::lang::set_english(true);
        assert!(explain(&io(io::ErrorKind::TimedOut)).starts_with("Timeout"));
        assert!(explain(&wrapped).starts_with("Wrong REALITY key"));
        assert!(explain(&slow).starts_with("Too slow"));
        assert_eq!(explain(&udp), "TCP ping doesn't work for a UDP protocol");
        crate::lang::set_english(false);
    }
}
