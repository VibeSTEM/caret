// 分组 Ribbon 工具：最小宽度统一，长文案按内容扩宽；保留焦点和禁用语义。
import QtQuick
import QtQuick.Controls

ToolButton {
    id: tile
    hoverEnabled: true

    property string iconName: ""
    property int iconSize: Theme.iconSizeRibbon
    property bool showCaret: false

    opacity: enabled ? 1 : Theme.disabledOpacity
    Accessible.name: text.replace(/\n/g, " ")

    leftPadding: Theme.ribbonContentSpacing
    rightPadding: Theme.ribbonContentSpacing
    topPadding: Theme.ribbonContentSpacing
    bottomPadding: Theme.ribbonContentSpacing
    spacing: Theme.ribbonContentSpacing

    implicitWidth: Math.max(Theme.ribbonToolMinimumWidth, contentItem.implicitWidth + leftPadding + rightPadding)
    implicitHeight: Math.max(Theme.ribbonToolHeight, contentItem.implicitHeight + topPadding + bottomPadding)

    background: Rectangle {
        color: tile.enabled && (tile.hovered || tile.visualFocus) ? Theme.colorHover : Theme.colorTransparent
        radius: Theme.radiusSmall
        border.width: tile.visualFocus ? Theme.focusBorderWidth : 0
        border.color: Theme.colorFocus
    }

    contentItem: Item {
        implicitWidth: content.implicitWidth
        implicitHeight: content.implicitHeight

        Column {
            id: content
            objectName: "ribbonTileContent"
            anchors.centerIn: parent
            spacing: tile.spacing

            ThemedIcon {
                objectName: "ribbonTileIcon"
                anchors.horizontalCenter: parent.horizontalCenter
                name: tile.iconName
                iconSize: tile.iconSize
                preserveSourceColors: true
            }
            ThemedLabel {
                objectName: "ribbonTileLabel"
                anchors.horizontalCenter: parent.horizontalCenter
                // 单行与双行共用文字区高度，保持各工具图标和文字首行对齐。
                height: Math.max(implicitHeight, Theme.ribbonTextLines * Theme.ribbonTextLineHeight)
                text: tile.text
                textSize: Theme.fontRibbon
                textColor: Theme.colorText
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignTop
                lineHeightMode: Text.FixedHeight
                lineHeight: Theme.ribbonTextLineHeight
            }
            Item {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Theme.iconSizeCompact
                // 没有下拉时也占位，避免整列重新居中导致图标上下漂移。
                height: Theme.ribbonCaretHeight
                ThemedIcon {
                    anchors.centerIn: parent
                    visible: tile.showCaret
                    name: "caret"
                    iconSize: Theme.iconSizeCompact
                }
            }
        }
    }
}
