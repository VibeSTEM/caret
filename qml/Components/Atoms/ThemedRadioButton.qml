// 圆形模式选择控件；互斥分组和当前模式由调用方提供。
import QtQuick
import QtQuick.Controls

RadioButton {
    id: control
    implicitHeight: Theme.controlHeight
    padding: 0
    spacing: Theme.spacingSmall
    font.pixelSize: Theme.fontBody
    indicator: Rectangle {
        implicitWidth: Theme.selectionIndicatorSize
        implicitHeight: Theme.selectionIndicatorSize
        y: (control.height - height) / 2
        radius: width / 2
        color: Theme.colorPanel
        border.width: control.visualFocus ? Theme.focusBorderWidth : Theme.borderWidth
        border.color: control.checked || control.visualFocus ? Theme.colorFocus : Theme.colorPanelLine
        Rectangle {
            anchors.centerIn: parent
            width: Theme.selectionIndicatorSize / 2
            height: width
            radius: width / 2
            color: Theme.colorFocus
            visible: control.checked
        }
    }
    contentItem: ThemedLabel {
        text: control.text
        leftPadding: control.indicator.width + control.spacing
        verticalAlignment: Text.AlignVCenter
    }
}
