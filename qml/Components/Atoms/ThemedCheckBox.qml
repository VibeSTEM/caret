// 蓝底白勾的主题选择控件；保留 CheckBox 的键盘与可访问性语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
    indicator: Rectangle {
        x: control.leftPadding
        y: (control.height - height) / 2
        implicitWidth: Theme.selectionIndicatorSize
        implicitHeight: Theme.selectionIndicatorSize
        radius: Theme.radiusSmall
        color: control.checked ? Theme.colorFocus : Theme.colorPanel
        border.color: control.checked || control.visualFocus ? Theme.colorFocus : Theme.colorPanelLine
        border.width: control.visualFocus ? Theme.focusBorderWidth : Theme.borderWidth

        ThemedIcon {
            anchors.centerIn: parent
            name: "check"
            iconSize: Theme.iconSizeCompact
            color: Theme.colorPanel
            visible: control.checked
        }
    }
    contentItem: ThemedLabel {
        text: control.text
        textSize: control.font.pixelSize
        leftPadding: control.indicator.width + control.spacing
        verticalAlignment: Text.AlignVCenter
        wrapMode: control.wrapText ? Text.Wrap : Text.NoWrap
    }
}
