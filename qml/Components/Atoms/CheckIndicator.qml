// 方框选择指示；由 CheckBox 或互斥 RadioButton 提供状态。
import QtQuick

Rectangle {
    property bool checked: false
    property bool focused: false
    implicitWidth: Theme.selectionIndicatorSize
    implicitHeight: Theme.selectionIndicatorSize
    radius: Theme.radiusSmall
    color: checked ? Theme.colorFocus : Theme.colorPanel
    border.color: checked || focused ? Theme.colorFocus : Theme.colorPanelLine
    border.width: focused ? Theme.focusBorderWidth : Theme.borderWidth
    ThemedIcon {
        anchors.centerIn: parent
        name: "check"
        iconSize: Theme.iconSizeCompact
        color: Theme.colorPanel
        visible: parent.checked
    }
}
