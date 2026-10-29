// 命令候选行；显示别名、翻译名称与当前选择状态。
pragma ComponentBehavior: Bound

import QtQuick

Item {
    id: root

    required property var candidate
    required property string label
    required property bool selected
    required property real aliasColumnWidth
    required property real labelColumnWidth

    signal activated

    implicitHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)

    HighlightBackground {
        anchors.fill: parent
        color: root.selected ? Theme.colorCommandPrompt : Theme.colorPanel
        opacity: root.candidate.available ? 1 : Theme.disabledOpacity
    }

    AutoScrollText {
        id: aliasContents
        x: Theme.spacingSmall
        y: 0
        width: Math.max(0, Math.min(root.aliasColumnWidth, parent.width - Theme.spacingSmall * 2))
        height: parent.height
        text: root.candidate.alias
        textColor: Theme.colorFocus
        fontPixelSize: Theme.fontSmall
        monospaced: true
        scrollActive: root.selected
    }

    AutoScrollText {
        id: labelContents
        x: aliasContents.x + aliasContents.width + Theme.spacingMedium
        y: 0
        width: Math.max(0, Math.min(root.labelColumnWidth, parent.width - x - Theme.spacingSmall))
        height: parent.height
        text: root.label
        textColor: Theme.colorText
        fontPixelSize: Theme.fontBody
        scrollActive: root.selected
    }

    TapHandler {
        onTapped: root.activated()
    }
}
