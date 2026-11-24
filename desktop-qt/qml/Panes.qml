import QtQuick

Item {
    property bool bare: false
    anchors.fill: parent
    anchors.topMargin: bare || Ui.compact ? 0 : 24
    anchors.rightMargin: bare || Ui.compact ? 0 : 24
    anchors.bottomMargin: bare || Ui.compact ? 0 : 24
}
