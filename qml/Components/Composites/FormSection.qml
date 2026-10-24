// 工艺表单的分组布局。
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

GroupBox {
    id: section
    Layout.fillWidth: true
    padding: Theme.spacingLarge
    topPadding: label.implicitHeight + Theme.spacingLarge
    label: ThemedLabel {
        text: section.title
        font.weight: Font.DemiBold
        leftPadding: Theme.spacingSmall
        rightPadding: Theme.spacingSmall
        background: Rectangle {
            color: Theme.colorPanel
        }
    }
    background: Rectangle {
        y: section.label.implicitHeight / 2
        height: section.height - y
        color: Theme.colorPanel
        border.color: Theme.colorPanelLine
        border.width: Theme.borderWidth
        radius: Theme.radiusSmall
    }
}
