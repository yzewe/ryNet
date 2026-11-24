pragma Singleton
import QtQuick

QtObject {
    readonly property real fastK: 800
    readonly property real fastZ: 0.6
    readonly property real spatialK: 380
    readonly property real spatialZ: 0.8
    readonly property real slowK: 200
    readonly property real slowZ: 0.8
    readonly property real effectsFastK: 3800
    readonly property real effectsK: 1600
    readonly property real effectsSlowK: 800
    readonly property real bouncyK: 520
    readonly property real bouncyZ: 0.58
    readonly property real pressInK: 1800
    readonly property real pressOutK: 650
    readonly property real pressOutZ: 0.42

    readonly property int effectsMs: 180
    readonly property int effectsSlowMs: 280
    readonly property var decelerate: [0.05, 0.7, 0.1, 1, 1, 1]
    readonly property var accelerate: [0.3, 0, 0.8, 0.15, 1, 1]
    readonly property var standard: [0.2, 0, 0, 1, 1, 1]

    readonly property string font: Ui.fontFamily !== "" ? Ui.fontFamily : Qt.application.font.family
    readonly property string iconFont: "Material Symbols Rounded"

    readonly property var type: ({
        displayLarge: [57, 400], displayMedium: [45, 400], displaySmall: [36, 400], displaySmallE: [36, 600],
        headlineLarge: [32, 400], headlineMedium: [28, 400], headlineMediumE: [28, 600],
        headlineSmall: [24, 400], headlineSmallE: [24, 600],
        titleLarge: [22, 400], titleLargeE: [22, 600], titleMedium: [16, 500], titleMediumE: [16, 600],
        titleSmall: [14, 500], titleSmallE: [14, 600],
        bodyLarge: [16, 400], bodyMedium: [14, 400], bodySmall: [12, 400],
        labelLarge: [14, 500], labelLargeE: [14, 600], labelMedium: [12, 500], labelMediumE: [12, 600], labelSmall: [11, 500]
    })

    readonly property real xs: 4
    readonly property real sm: 8
    readonly property real md: 12
    readonly property real lg: 16
    readonly property real lgPlus: 20
    readonly property real xl: 28
    readonly property real xlPlus: 32
    readonly property real full: 999

    readonly property int minPressMs: 90
}
