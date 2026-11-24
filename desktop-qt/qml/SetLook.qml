pragma Singleton
import QtQuick

QtObject {
    readonly property var icons: ["grid_view", "language", "lock", "bolt", "smartphone", "campaign", "home", "work", "school",
        "account_balance", "shopping_cart", "mail", "chat", "movie", "music_note", "tv", "videogame_asset", "code", "cloud",
        "shield", "block", "flight", "star"]
    readonly property var shapes: ["Cookie9Sided", "Sunny", "Clover4Leaf", "Pill", "Gem", "Cookie4Sided"]

    function icon(i) { return icons[((i % icons.length) + icons.length) % icons.length] }
    function shape(i) { return shapes[((i % shapes.length) + shapes.length) % shapes.length] }
    function name(s) { return s && s.name ? s.name : Ui.tr("Набор", "Set") }
    function entries(n) { return App.plural(n, "запись", "записи", "записей", "entry", "entries") }
}
