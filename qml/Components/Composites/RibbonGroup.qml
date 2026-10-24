// Ribbon 分组拥有白底、竖分隔线和底部组名；工具内容由面板注入。
import QtQuick

Rectangle {
    id: group

    default property alias tools: toolsRow.data
    property string title: ""
    property bool showCaret: false

    implicitWidth: Math.max(toolsRow.implicitWidth, captionRow.implicitWidth) + 2 * Theme.spacingTiny + Theme.borderWidth
    implicitHeight: Theme.ribbonHeight - Theme.borderWidth
    color: Theme.colorPanel

    Row {
        id: toolsRow
        objectName: "ribbonTools"
        // 扣除右分隔线后居中；显式坐标保留半像素，避免居中锚点取整。
        x: (group.width - Theme.borderWidth - width) / 2
        y: (group.height - Theme.ribbonLabelHeight - height) / 2
        spacing: Theme.spacingTiny
    }

    Rectangle {
        anchors.right: parent.right
        width: Theme.borderWidth
        height: parent.height
        color: Theme.colorPanelLine
    }
    Item {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.rightMargin: Theme.borderWidth
        anchors.bottom: parent.bottom
        height: Theme.ribbonLabelHeight

        Rectangle {
            width: parent.width
            height: Theme.borderWidth
            color: Theme.colorPanelLine
        }
        Row {
            id: captionRow
            anchors.centerIn: parent
            spacing: Theme.ribbonContentSpacing
            ThemedLabel {
                anchors.verticalCenter: parent.verticalCenter
                text: group.title
                textSize: Theme.fontRibbon
                textColor: Theme.colorTextMuted
            }
            ThemedIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: group.showCaret
                name: "caret"
                iconSize: Theme.iconSizeCompact
                color: Theme.colorTextMuted
            }
        }
    }
}
