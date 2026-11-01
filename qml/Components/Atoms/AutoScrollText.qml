// 仅在内容溢出且所属候选高亮时循环展示单行文本。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters

Flickable {
    id: root

    required property string text
    required property color textColor
    required property int fontPixelSize
    property bool monospaced: false
    property bool scrollActive: true

    readonly property int pauseDurationMilliseconds: 1000
    readonly property int millisecondsPerLogicalPixel: 24
    readonly property real naturalTextWidth: textMetrics.advanceWidth
    readonly property real overflowDistance: Math.max(0, naturalTextWidth - width)
    readonly property bool shouldScroll: scrollActive && visible && width > 0 && overflowDistance > 0
    readonly property int scrollDurationMilliseconds: Math.max(1, Math.ceil(overflowDistance * millisecondsPerLogicalPixel))

    implicitHeight: fontPixelSize
    contentWidth: naturalTextWidth
    contentHeight: height
    flickableDirection: Flickable.HorizontalFlick
    boundsBehavior: Flickable.StopAtBounds
    interactive: false
    clip: true

    function restartScrollAnimation() {
        contentX = 0;
        if (scrollAnimation.running)
            scrollAnimation.restart();
    }

    onTextChanged: restartScrollAnimation()
    onWidthChanged: restartScrollAnimation()
    onFontPixelSizeChanged: restartScrollAnimation()
    onMonospacedChanged: restartScrollAnimation()
    onOverflowDistanceChanged: restartScrollAnimation()

    TextMetrics {
        id: textMetrics
        font: displayText.font
        text: root.text
    }

    Text {
        id: displayText
        height: root.height
        color: root.textColor
        font.pixelSize: root.fontPixelSize
        text: root.text
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignLeft
        wrapMode: Text.NoWrap

        states: State {
            when: root.monospaced

            PropertyChanges {
                displayText.font: Qt.font({
                    family: PlatformFonts.codeFamily,
                    pixelSize: root.fontPixelSize
                })
            }
        }
    }

    SequentialAnimation {
        id: scrollAnimation
        running: root.shouldScroll
        loops: Animation.Infinite

        onRunningChanged: {
            if (!running)
                root.contentX = 0;
        }

        NumberAnimation {
            target: root
            property: "contentX"
            from: 0
            to: root.overflowDistance
            duration: root.scrollDurationMilliseconds
            easing.type: Easing.Linear
        }

        PauseAnimation {
            duration: root.pauseDurationMilliseconds
        }

        ScriptAction {
            script: root.contentX = 0
        }

        PauseAnimation {
            duration: root.pauseDurationMilliseconds
        }
    }
}
