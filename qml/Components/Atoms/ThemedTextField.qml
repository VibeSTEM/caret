// 主题文本输入原子：保留 TextField 的键盘、焦点和可访问性行为。
import QtQuick
import QtQuick.Controls

TextField {
    id: field

    property color fieldColor: Theme.colorText
    property color placeholderColor: Theme.colorTextMuted
    property color fieldBorderColor: Theme.colorPanelLine
    property color focusBorderColor: Theme.colorIcon
    property bool invalid: false

    hoverEnabled: true
    color: field.fieldColor
    placeholderTextColor: field.placeholderColor
    font.pixelSize: Theme.fontBody
    topPadding: 0
    bottomPadding: 0
    leftPadding: Theme.spacingSmall
    rightPadding: Theme.spacingSmall

    background: Rectangle {
        color: Theme.colorPanel
        border.width: Theme.borderWidth
        border.color: field.invalid ? Theme.colorError : field.activeFocus ? field.focusBorderColor : field.fieldBorderColor
        radius: Theme.radiusSmall
    }
}
