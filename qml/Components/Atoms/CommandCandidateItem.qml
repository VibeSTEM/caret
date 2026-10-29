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

    ScrollableRow {
        id: aliasContents
        x: Theme.spacingSmall
        y: 0
        width: Math.max(0, Math.min(root.aliasColumnWidth, parent.width - Theme.spacingSmall * 2))
        height: parent.height
        spacing: 0

        Text {
            height: aliasContents.height
            color: Theme.colorFocus
            font.family: "monospace"
            font.pixelSize: Theme.fontSmall
            text: root.candidate.alias
            verticalAlignment: Text.AlignVCenter
        }
    }

    ScrollableRow {
        id: labelContents
        x: aliasContents.x + aliasContents.width + Theme.spacingMedium
        y: 0
        width: Math.max(0, Math.min(root.labelColumnWidth, parent.width - x - Theme.spacingSmall))
        height: parent.height
        spacing: 0

        Text {
            height: labelContents.height
            color: Theme.colorText
            font.pixelSize: Theme.fontBody
            text: root.label
            verticalAlignment: Text.AlignVCenter
        }
    }

    TapHandler {
        onTapped: root.activated()
    }
}
