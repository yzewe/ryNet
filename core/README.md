# ryNet core

Ядро ryNet на Rust. Подписки и поведение протоколов совместимы с Xray-core; работает в одном
потоке tokio и в простое не просыпается.

| Крейт | Что делает |
|---|---|
| `rv-config` | Xray JSON, sing-box и ссылки `vless://`, `vmess://`, `trojan://`, `hysteria2://`, `ss://`, `socks://`, `http://`, base64-подписки, `finalmask` |
| `rv-tls` | клиент TLS 1.3 с ClientHello байт в байт как uTLS (Chrome, Firefox, Safari, iOS, Edge), REALITY, Vision |
| `rv-net` | диалер: happy eyeballs, sockopt, `protect()` на Android, фрагментация ClientHello |
| `rv-vless` | VLESS, XTLS Vision, XUDP |
| `rv-xhttp` | XHTTP (stream-one, stream-up, packet-up), xmux, заголовки как у браузера |
| `rv-rules` | маршрутизация Xray и правила в синтаксисе mihomo |
| `rv-tun` | TUN: свой TCP-стек в userspace без таймеров в простое, UDP и DNS |
| `rv-core` | протоколы и транспорты, VLESS Encryption, движок, DNS, пинг и автовыбор |
| `rv-stats` | счётчики трафика в общей памяти для приложения |
| `rv-android` | JNI для приложения на Android |
| `rv-desktop` | ядро для Linux и Windows (`rynet-core`) и клиент для терминала (`rynet-cli`) |
| `rv-cli` | стенд для разработки: `describe`, `ping`, `socks` |

## Протоколы

| | TCP | UDP |
|---|---|---|
| VLESS + REALITY / TLS + Vision | да | XUDP |
| VLESS + XHTTP / gRPC / WebSocket / HTTPUpgrade | да | XUDP |
| VMess AEAD | да | XUDP |
| Trojan | да | да |
| Hysteria 2 (salamander, BBR, Brutal, port hopping) | да | да |
| Shadowsocks (AEAD и 2022) | да | да |
| SOCKS5, SOCKS4/4a, HTTP(S) CONNECT | да | SOCKS5 |

## Разработка

```bash
cargo test --workspace --locked
cargo build --release -p rv-cli
./target/release/rv-cli describe sub.json
./target/release/rv-cli ping sub.json proxy
./target/release/rv-cli socks sub.json "Финляндия" --listen 127.0.0.1:10808
```

Fuzz-цели лежат в `fuzz/` (`cargo fuzz run config_parse`).
