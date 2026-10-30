// 命令候选行；显示别名、翻译名称与当前选择状态。
pragma ComponentBehavior: Bound

import QtQuick

Item {
    id: root

    required property var candidate
    required property string displayAlias
    required property string label
    required property bool selected
    required property real aliasColumnWidth
    required property real labelColumnWidth

    signal activated

    implicitHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)

    HighlightBackground {
        anchors.fill: parent
        color: root.candidate.available ? (root.selected ? Theme.colorCommandPrompt : Theme.colorPanel) : (root.selected ? Theme.colorChrome : Theme.colorPanel)
    }

    AutoScrollText {
        id: aliasContents
        x: Theme.spacingSmall
        y: 0
        width: Math.max(0, Math.min(root.aliasColumnWidth, parent.width - Theme.spacingSmall * 2))
        height: parent.height
        text: root.displayAlias
        textColor: root.candidate.available ? Theme.colorFocus : Theme.colorTextMuted
        fontPixelSize: Theme.fontSmall
        monospaced: !root.candidate.commandId.endsWith(".font_family")
        scrollActive: root.selected
    }

    AutoScrollText {
        id: labelContents
        x: aliasContents.x + aliasContents.width + Theme.spacingMedium
        y: 0
        width: Math.max(0, Math.min(root.labelColumnWidth, parent.width - x - Theme.spacingSmall))
        height: parent.height
        text: root.label
        textColor: root.candidate.available ? Theme.colorText : Theme.colorTextMuted
        fontPixelSize: Theme.fontBody
        scrollActive: root.selected
    }

    ThemedIcon {
        anchors.right: parent.right
        anchors.rightMargin: Theme.spacingSmall
        anchors.verticalCenter: parent.verticalCenter
        visible: root.candidate.hasChildren === true
        name: "right"
        iconSize: Theme.iconSizeSmall
        color: Theme.colorTextMuted
    }

    TapHandler {
        onTapped: root.activated()
    }
}
