import QtQuick

Column {
    id: sec
    readonly property var s: App.settings

    ChoiceBlock {
        first: true
        title: Ui.tr("Отображение", "View")
        options: ["groups", "single"]
        labels: [Ui.tr("Группы", "Groups"), Ui.tr("Единый", "Single list")]
        selected: sec.s.view
        onPicked: (v) => App.set("view", v)
    }
    ChoiceBlock {
        title: Ui.tr("Сетка", "Grid")
        options: ["2x1", "1x1"]
        labels: ["2×1", "1×1"]
        selected: sec.s.grid
        onPicked: (v) => App.set("grid", v)
    }
    ChoiceBlock {
        title: Ui.tr("Сортировка", "Sorting")
        options: ["default", "ping", "name"]
        labels: [Ui.tr("Как есть", "Default"), Ui.tr("Пинг", "Ping"), Ui.tr("А–Я", "A–Z")]
        selected: sec.s.sort
        onPicked: (v) => App.set("sort", v)
    }
    ChoiceBlock {
        title: Ui.tr("Подпись в карточке", "Card caption")
        options: ["description", "protocol", "none"]
        labels: [Ui.tr("Описание", "Description"), Ui.tr("Протокол", "Protocol"), Ui.tr("Ничего", "None")]
        selected: sec.s.serverInfo
        onPicked: (v) => App.set("serverInfo", v)
    }
    Group {
        SettingRow {
            icon: "public"
            title: Ui.tr("Страна по IP выхода", "Country by exit IP")
            summary: Ui.tr("По стороннему сервису, бывает точнее названия", "From a third-party service, often more accurate than the name")
            checked: sec.s.exitFlags
            onActivated: App.set("exitFlags", !sec.s.exitFlags)
        }
    }
}
