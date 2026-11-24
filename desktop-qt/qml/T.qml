import QtQuick

Text {
    property string type: "bodyMedium"
    readonly property var spec: M.type[type] || M.type.bodyMedium

    color: Theme.onSurface
    font.family: M.font
    font.pixelSize: spec[0]
    font.weight: spec[1]
    elide: Text.ElideRight
    textFormat: Text.PlainText
}
