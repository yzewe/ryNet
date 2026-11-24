# ryNet для Linux и Windows

Окно на Qt Quick (C++ и QML) и Rust-ядро `rynet-core` из `core/rv-desktop`. Окно запускает ядро, которое
лежит рядом с ним (`RYNET_CORE` — другой путь), и общается с ним JSON-строками.

## Сборка для себя

Нужны Qt 6.8+ (Core, Gui, Network, Qml, Quick, ShaderTools, на Linux ещё DBus), CMake, Ninja, zlib и libqrencode.

```sh
cd core && cargo build --release -p rv-desktop
cd ../desktop-qt
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release && ninja -C build
./build/rynet          # ядро найдётся в core/target/release
```

## Выпуск

Пакеты собирают скрипты в `packaging/`:

- **Linux** — `packaging/linux/build.sh`: окно собирается в корне Ubuntu 22.04 (bwrap) со своим Qt без ICU,
  ядро — `cargo zigbuild` под glibc 2.28. Пакеты идут на Ubuntu 22.04+, Debian 12+, Fedora 36+ и Arch:
  deb, rpm, pkg.tar.zst, AppImage и tar.gz с `install.sh`. Всё ставится в `/opt/rynet` (окно, ядро и нужная
  часть Qt), `/usr/bin/rynet` — ссылка; после установки ядру выдаются права на TUN (`setcap`).
  Туда же ставится правило polkit (`packaging/linux/polkit`): `resolvectl` для `rynet0` проходит без пароля; пакет удаляет его при удалении.
  У AppImage и tar.gz права выдаёт само окно (`Platform::grantTun`): root кладёт сверенную по SHA-256 копию ядра в
  `/usr/local/lib/rynet` и даёт `setcap` ей, а не файлу в каталоге пользователя; окно запускает эту копию, пока она совпадает с ядром из пакета.
  Окружение (`packaging/linux/buildenv.sh`) готовится один раз: `buildenv.sh root` от root, затем
  `buildenv.sh` — CMake, Qt и libqrencode в `~/env/linux`; повторные сборки идут через ccache.
- **Windows** — `packaging/windows/build.cmd` на Windows: окно собирается MinGW 13.1 и Qt для MinGW, ядро
  (`rynet-core.exe`) — `cargo zigbuild --target x86_64-pc-windows-gnu`, `windeployqt` кладёт только нужное.
  Получаются MSI (на месте прежнего, тот же UpgradeCode, `%LOCALAPPDATA%\rynet`), установщик Inno Setup
  и портативный zip (настройки — в `data`). Статические zlib и libqrencode собирает `packaging/windows/deps.cmd`.

Значки — `packaging/icons.py`, картинки установщика — `packaging/windows/wizard.py`.
