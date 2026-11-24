import QtQuick

Text {
    property string name
    property real size: 24
    property real fill: 0
    property int weight: 400

    width: size
    height: size
    text: Icons.map[name] || ""
    color: Theme.onSurfaceVariant
    font.family: M.iconFont
    font.pixelSize: size
    font.variableAxes: ({ "FILL": fill, "wght": weight })
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    textFormat: Text.PlainText
    renderType: fill > 0 ? Text.CurveRendering : Text.QtRendering
}
