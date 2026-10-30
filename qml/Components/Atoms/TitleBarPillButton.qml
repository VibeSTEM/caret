// 标题栏胶囊按钮：统一描边操作按钮与强调色分享按钮的形状、焦点和悬停反馈。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

import QtQuick

ThemedToolButton {
    id: button

    property bool accent: false

    font.pixelSize: Theme.fontSmall
    iconSize: Theme.iconSizeDefault
    contentPadding: Theme.spacingSmall
    cornerRadius: Theme.radiusSmall + Theme.spacingSmall
    contentColor: accent ? Theme.colorPanel : Theme.colorText
    hoverColor: Theme.colorHover

    contentItem: Item {
        id: pillContent

        readonly property real leadingAccessoryWidth: button.iconName !== "" ? mainIcon.width + button.spacing : 0
        readonly property real trailingAccessoryWidth: button.showCaret ? caretIcon.width + button.spacing : 0
        readonly property real textContentWidth: titleLabel.implicitWidth + (dimLabel.visible ? button.spacing + dimLabel.implicitWidth : 0)

        implicitWidth: leadingAccessoryWidth + textContentWidth + trailingAccessoryWidth
        implicitHeight: Math.max(mainIcon.height, textContent.implicitHeight, caretIcon.height)

        ThemedIcon {
            id: mainIcon
            objectName: "titleBarPillButtonIcon"
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            visible: button.iconName !== ""
            name: button.iconName
            iconSize: button.iconSize
            color: button.contentColor
            preserveSourceColors: button.preserveIconColors
        }

        Item {
            id: textArea
            anchors.left: mainIcon.visible ? mainIcon.right : parent.left
            anchors.leftMargin: mainIcon.visible ? button.spacing : 0
            anchors.right: caretIcon.visible ? caretIcon.left : parent.right
            anchors.rightMargin: caretIcon.visible ? button.spacing : 0
            anchors.verticalCenter: parent.verticalCenter
            height: textContent.implicitHeight

            Row {
                id: textContent
                anchors.centerIn: parent
                spacing: button.spacing

                ThemedLabel {
                    id: titleLabel
                    visible: button.text !== ""
                    text: button.text
                    textSize: button.font.pixelSize
                    font.weight: button.font.weight
                    textColor: button.contentColor
                }

                ThemedLabel {
                    id: dimLabel
                    visible: button.dimText !== ""
                    text: button.dimText
                    textSize: button.font.pixelSize
                    font.weight: button.font.weight
                    textColor: Theme.colorTextMuted
                }
            }
        }

        ThemedIcon {
            id: caretIcon
            objectName: "titleBarPillButtonCaret"
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: button.showCaret
            name: "caret"
            iconSize: Theme.iconSizeCompact
            color: button.contentColor
        }
    }

    background: Rectangle {
        objectName: "titleBarPillButtonBackground"
        radius: button.cornerRadius
        color: {
            if (button.accent)
                return button.hovered ? Qt.darker(Theme.colorFocus, 1.08) : Theme.colorFocus;
            if (button.pressed)
                return Theme.colorTransparent;
            if (button.hovered || button.visualFocus)
                return Theme.colorHover;
            return Theme.colorTransparent;
        }
        border.width: button.accent ? 0 : button.visualFocus ? Theme.focusBorderWidth : Theme.borderWidth
        border.color: button.visualFocus ? Theme.colorFocus : Theme.colorPanelLine
    }
}
