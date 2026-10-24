// 带标题的信息框；同一面板可按段落数显示提示内容。
import QtQuick
import QtQuick.Layouts

Item {
    id: informationPanel

    property string titleText: ""
    property var messages: []

    Rectangle {
        id: frame
        x: Theme.spacingSmall
        y: Theme.spacingMedium
        width: Math.max(0, informationPanel.width - 2 * x)
        height: Math.max(0, informationPanel.height - y - Theme.spacingSmall)
        color: Theme.colorTransparent
        border.color: Theme.colorPanelLine
        border.width: Theme.borderWidth
    }

    ThemedLabel {
        x: frame.x + Theme.spacingMedium
        y: frame.y - height / 2
        text: informationPanel.titleText
        textSize: Theme.fontSmall
        leftPadding: Theme.spacingXSmall
        rightPadding: Theme.spacingXSmall
        background: Rectangle {
            color: Theme.colorPanel
        }
    }

    ColumnLayout {
        x: frame.x + Theme.spacingMedium
        y: frame.y + Theme.spacingMedium
        width: Math.max(0, frame.width - 2 * Theme.spacingMedium)
        spacing: Theme.spacingLarge

        Repeater {
            model: informationPanel.messages

            delegate: ThemedLabel {
                required property string modelData

                Layout.fillWidth: true
                text: modelData
                textSize: Theme.fontBody
                wrapMode: Text.Wrap
            }
        }
    }
}
