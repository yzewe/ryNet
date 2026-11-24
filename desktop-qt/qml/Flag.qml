import QtQuick

Item {
    id: f

    property string flag
    property real radius: 4
    property bool outline: true
    readonly property string code: Ui.flagCode(flag)
    readonly property bool picture: code !== "" && img.status !== Image.Error

    Image {
        id: img
        anchors.fill: parent
        visible: false
        source: f.code !== "" ? "qrc:/flags/" + f.code + ".png" : ""
        sourceSize: Qt.size(Math.ceil(f.width * 2), Math.ceil(f.height * 2))
        fillMode: Image.Stretch
        smooth: true
        mipmap: true
    }
    ShaderEffect {
        anchors.fill: parent
        visible: f.picture
        property var source: img
        property size size: Qt.size(width, height)
        property real radius: f.radius
        fragmentShader: "qrc:/round.frag.qsb"
    }
    Rectangle {
        anchors.fill: parent
        visible: f.picture && f.outline
        radius: f.radius
        color: "transparent"
        border.width: 1
        border.color: Theme.alpha(Theme.outlineVariant, 0.6)
    }
    Text {
        anchors.centerIn: parent
        visible: !f.picture
        text: f.flag !== "" ? f.flag : "🌐"
        font.pixelSize: Math.min(f.width, f.height) * 0.75
    }
}
