// Ribbon 专用紧凑下拉框；禁用状态仍保持清晰的白底与文本层级。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

ComboBox {
    id: combo

    height: Theme.ribbonComboBoxHeight
    font.pixelSize: Theme.fontRibbon
    opacity: 1
    hoverEnabled: true

    contentItem: ThemedLabel {
        leftPadding: Theme.spacingSmall
        rightPadding: Theme.iconSizeCompact + Theme.spacingMedium
        text: combo.displayText
        textSize: Theme.fontRibbon
        textColor: Theme.colorText
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    indicator: ThemedIcon {
        anchors.right: parent.right
        anchors.rightMargin: Theme.spacingSmall
        anchors.verticalCenter: parent.verticalCenter
        name: "caret"
        iconSize: Theme.iconSizeCompact
        color: Theme.colorTextMuted
    }

    background: Rectangle {
        color: Theme.colorPanel
        border.width: Theme.borderWidth
        border.color: combo.visualFocus ? Theme.colorFocus : Theme.colorPanelLine
        radius: Theme.radiusSmall
    }

    delegate: ItemDelegate {
        id: optionDelegate
        required property int index
        width: combo.width
        height: Theme.ribbonComboBoxHeight
        highlighted: combo.highlightedIndex === index

        contentItem: ThemedLabel {
            leftPadding: Theme.spacingSmall
            text: combo.textAt(optionDelegate.index)
            textSize: Theme.fontRibbon
            textColor: Theme.colorText
            verticalAlignment: Text.AlignVCenter
        }

        background: Rectangle {
            color: optionDelegate.highlighted ? Theme.colorHover : Theme.colorPanel
        }
    }

    popup: Popup {
        y: combo.height
        width: combo.width
        implicitHeight: contentItem.implicitHeight + 2 * padding
        padding: Theme.borderWidth
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: combo.popup.visible ? combo.delegateModel : null
            currentIndex: combo.highlightedIndex
            boundsBehavior: Flickable.StopAtBounds
        }

        background: Rectangle {
            color: Theme.colorPanel
            border.width: Theme.borderWidth
            border.color: Theme.colorPanelLine
        }
    }
}
