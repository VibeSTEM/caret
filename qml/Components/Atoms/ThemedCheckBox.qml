// 蓝底白勾的主题选择控件；保留 CheckBox 的键盘与可访问性语义。
import QtQuick
import QtQuick.Controls

CheckBox {
    id: control
    property bool wrapText: false
    implicitHeight: Theme.controlHeight
    padding: 0
    spacing: Theme.spacingSmall
    hoverEnabled: true
    font.pixelSize: Theme.fontBody
    opacity: enabled ? 1 : Theme.disabledOpacity
    indicator: CheckIndicator {
        x: control.leftPadding
        y: (control.height - height) / 2
        checked: control.checked
        focused: control.visualFocus
    }
    contentItem: ThemedLabel {
        text: control.text
        textSize: control.font.pixelSize
        leftPadding: control.indicator.width + control.spacing
        verticalAlignment: Text.AlignVCenter
        wrapMode: control.wrapText ? Text.Wrap : Text.NoWrap
    }
}
