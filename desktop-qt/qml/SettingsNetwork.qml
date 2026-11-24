import QtQuick

Column {
    id: sec
    readonly property var s: App.settings
    readonly property string dnsChoice: Ui.dnsChoice(s.dnsServers || "")

    ChoiceBlock {
        first: true
        title: Ui.tr("Маршрутизация", "Routing")
        options: ["smart", "subscription", "global", "direct"]
        labels: [Ui.tr("Умный", "Smart"), Ui.tr("Подписка", "Subscription"), Ui.tr("Всё", "All"), Ui.tr("Напрямую", "Direct")]
        summaries: [Ui.tr("Российское и локальная сеть — напрямую", "Russian and local traffic goes direct"), Ui.tr("Правила из подписки", "Rules from the subscription"),
                    Ui.tr("Весь трафик через VPN", "All traffic through VPN"), Ui.tr("Весь трафик идёт напрямую, кроме правил", "All traffic goes direct, except your rules")]
        selected: sec.s.routingMode
        onPicked: (v) => App.setRouting(v)
    }
    Group {
        SettingRow {
            icon: "edit"
            title: Ui.tr("Правила", "Rules")
            summary: sec.s.ruleCount > 0 ? App.plural(sec.s.ruleCount, "правило", "правила", "правил", "rule")
                : sec.s.routingMode === "direct" ? Ui.tr("Какие сайты и приложения идут через VPN", "Which sites and apps use the VPN")
                : Ui.tr("Какие сайты и приложения идут мимо VPN", "Which sites and apps skip the VPN")
            onActivated: App.nav = "rules"
        }
    }

    Header { text: "DNS" }
    Group {
        DropdownRow {
            icon: "language"
            title: "DNS"
            options: Ui.dnsOptions()
            labels: Ui.dnsLabels()
            selected: sec.dnsChoice
            summary: Ui.dnsSummary(sec.s.dnsServers || "")
            onPicked: (v) => {
                if (v === "auto")
                    App.set("dnsServers", "", true)
                else if (v === "custom")
                    edit.ask("dns")
                else
                    App.set("dnsServers", v, true)
            }
            onReselected: (v) => { if (v === "custom") edit.ask("dns") }
        }
        DropdownRow {
            icon: "dns"
            title: Ui.tr("DNS серверов", "Server DNS")
            options: ["auto", "cloudflare", "google", "system", "custom"]
            labels: [Ui.tr("Авто", "Auto"), "Cloudflare", "Google", Ui.tr("Системный", "System"), Ui.tr("Свой", "Custom")]
            selected: sec.s.serverDns
            summary: sec.s.serverDns === "custom" && (sec.s.serverDnsCustom || "") !== "" ? sec.s.serverDnsCustom : labels[Math.max(0, options.indexOf(selected))]
            onPicked: (v) => v === "custom" ? edit.ask("serverDnsCustom") : App.set("serverDns", v, true)
            onReselected: (v) => { if (v === "custom") edit.ask("serverDnsCustom") }
        }
        SettingRow {
            icon: "bolt"
            title: "FakeDNS"
            summary: Ui.tr("Сайты открываются быстрее, без ожидания DNS", "Sites open faster, without waiting for DNS")
            checked: sec.s.fakeDns
            onActivated: App.set("fakeDns", !sec.s.fakeDns, true)
        }
    }

    Header { text: Ui.tr("Обход блокировок", "Anti-censorship") }
    Group {
        SettingRow {
            icon: "link"
            title: Ui.tr("Цепочка", "Chain")
            readonly property var entry: (App.rev < 0 ? undefined : App.chainServer())
            summary: sec.s.chain && entry.name ? entry.name
                : Ui.tr("Трафик идёт через два сервера: вход и выход", "Traffic goes through two servers: entry and exit")
            checked: sec.s.chain
            onActivated: {
                if (sec.s.chain) {
                    App.set("chain", false, true)
                } else {
                    App.set("chain", true)
                    App.openEntryPicker()
                }
            }
        }
        SettingRow {
            icon: "content_cut"
            title: Ui.tr("Фрагментация", "Fragmentation")
            summary: Ui.tr("Помогает обойти блокировки у части провайдеров", "Helps get past blocking at some providers")
            checked: sec.s.fragment
            onActivated: App.set("fragment", !sec.s.fragment, true)
        }
    }
    Fold {
        open: sec.s.fragment === true
        ChoiceBlock {
            title: Ui.tr("Сила фрагментации", "Fragmentation strength")
            current: sec.s.fragmentPreset < 0 ? sec.s.fragmentLength + " · " + sec.s.fragmentInterval : ""
            options: [0, 1, 2, -1]
            labels: [Ui.tr("Лёгкая", "Light"), Ui.tr("Средняя", "Medium"), Ui.tr("Сильная", "Strong"), Ui.tr("Своя", "Custom")]
            summaries: [Ui.tr("Крупные куски — почти без задержки · 100-200 байт, 10-20 мс", "Large chunks — almost no delay · 100-200 bytes, 10-20 ms"),
                        Ui.tr("Если с лёгкой всё ещё режут · 30-60 байт, 10-30 мс", "If light still gets blocked · 30-60 bytes, 10-30 ms"),
                        Ui.tr("Мелкие куски — дольше соединение · 5-20 байт, 20-50 мс", "Small chunks — slower to connect · 5-20 bytes, 20-50 ms"),
                        Ui.tr("Длина кусков и пауза между ними — «от-до»", "Chunk size and pause between chunks, as min-max")]
            selected: sec.s.fragmentPreset
            onPicked: (v) => {
                const presets = [["100-200", "10-20"], ["30-60", "10-30"], ["5-20", "20-50"]]
                if (v < 0) {
                    edit.ask("fragment")
                    return
                }
                App.set("fragmentLength", presets[v][0])
                App.set("fragmentInterval", presets[v][1], true)
            }
        }
    }

    ChoiceBlock {
        title: Ui.tr("Трафик", "Traffic")
        options: ["tun", "proxy"]
        labels: [Ui.tr("Весь трафик", "All traffic"), Ui.tr("Прокси", "Proxy")]
        summaries: [Ui.tr("Через VPN идут все программы", "Every program goes through the VPN"), Ui.tr("Через VPN идёт то, что настроено на прокси", "What is set to use the proxy goes through the VPN")]
        selected: sec.s.connMode
        onPicked: (v) => App.set("connMode", v, true)
    }
    Group {
        Fold {
            open: sec.s.connMode === "tun"
            SettingRow {
                visible: !App.tunCapable
                icon: "lock_open"
                title: Ui.tr("Разрешить режим «Весь трафик»", "Allow \"All traffic\" mode")
                summary: Ui.tr("Один раз попросит пароль администратора", "Asks for the administrator password once")
                onActivated: App.grantTun()
            }
            DropdownRow {
                icon: "straighten"
                title: "MTU"
                options: [0, 1500, 1400, 1280, -1]
                labels: [Ui.tr("Авто", "Auto"), "1500", "1400", "1280", Ui.tr("Своё", "Custom")]
                selected: options.indexOf(sec.s.mtu) >= 0 ? sec.s.mtu : -1
                summary: selected === -1 ? String(sec.s.mtu) : labels[options.indexOf(selected)]
                onPicked: (v) => v < 0 ? edit.ask("mtu") : App.set("mtu", v, true)
                onReselected: (v) => { if (v < 0) edit.ask("mtu") }
            }
            SettingRow {
                icon: "lock"
                title: Ui.tr("Защита от утечки трафика", "Kill switch")
                summary: Ui.tr("Если VPN оборвётся, интернет не пойдёт напрямую", "If the VPN drops, traffic won't go direct")
                checked: sec.s.killSwitch
                onActivated: App.set("killSwitch", !sec.s.killSwitch, true)
            }
            CardBlock {
                visible: App.windows
                ProcessList {}
            }
            CardBlock {
                PortList {}
            }
        }
        Fold {
            open: sec.s.connMode === "proxy"
            SettingRow {
                visible: App.sysProxyAvailable
                icon: "apps"
                title: Ui.tr("Системный прокси", "System proxy")
                summary: Ui.tr("Браузер и программы пойдут через прокси сами", "The browser and apps go through the proxy by themselves")
                checked: sec.s.systemProxy
                onActivated: App.setSystemProxy(!sec.s.systemProxy)
            }
            SettingRow {
                icon: "data_usage"
                title: Ui.tr("Порт", "Port")
                summary: "127.0.0.1:" + sec.s.port
                onActivated: edit.ask("port")
            }
        }
        SettingRow {
            icon: "language"
            title: Ui.tr("Только IPv4", "IPv4 only")
            summary: Ui.tr("Не использовать IPv6 для соединений и DNS", "Use IPv4 for connections and DNS")
            checked: sec.s.ipv4Only === true
            onActivated: App.set("ipv4Only", !sec.s.ipv4Only, true)
        }
    }

    EditDialog {
        id: edit
        function ask(w) {
            multiline = w === "dns" || w === "serverDnsCustom"
            allowEmpty = w === "dns" || w === "mtu"
            placeholder = w === "dns" ? "https://dns.example/dns-query" : w === "serverDnsCustom" ? "1.1.1.1\nhttps://dns.example/dns-query\ntls://dns.example"
                : w === "mtu" ? Ui.tr("Авто (9000)", "Auto (9000)") : w === "fragment" ? "10-30 10-20" : ""
            edit.edit(w === "dns" ? "DNS" : w === "serverDnsCustom" ? Ui.tr("DNS серверов", "Server DNS") : w === "mtu" ? "MTU" : w === "port" ? Ui.tr("Порт", "Port")
                      : Ui.tr("Куски, байт · пауза, мс", "Chunks, bytes · pause, ms"), w)
        }
    }
}
