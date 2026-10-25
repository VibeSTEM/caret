// 工具栏高度锁在 Theme.menubarHeight。右侧三个按钮还没有命令。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: commandBar

    readonly property int rowHeight: Theme.menubarHeight

    readonly property int chipHeight: Math.min(Theme.toolbarButtonHeight - Theme.spacingTiny, rowHeight - Theme.spacingSmall)

    implicitHeight: rowHeight
    clip: true
    color: Theme.colorChrome

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingMedium
        anchors.rightMargin: Theme.spacingLarge
        spacing: Theme.spacingSmall
        z: 1

        Item {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredHeight: commandBar.rowHeight
            Layout.maximumHeight: commandBar.rowHeight
            implicitWidth: homeButton.implicitWidth
            implicitHeight: commandBar.rowHeight

            MenuTabButton {
                id: homeButton
                objectName: "homeTab"
                anchors.fill: parent
                text: qsTr("Home")
                highlighted: false
                font.weight: Font.DemiBold
                controlHeight: commandBar.rowHeight
                contentColor: Theme.colorText
                hoverColor: Theme.colorHover
            }

            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.leftMargin: Theme.spacingMedium
                anchors.rightMargin: Theme.spacingMedium
                height: 2
                color: Theme.colorFocus
            }
        }

        Item {
            Layout.fillWidth: true
        }

        ThemedToolButton {
            objectName: "commentsButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.maximumHeight: commandBar.chipHeight
            font.pixelSize: Theme.fontSmall
            contentPadding: Theme.spacingTiny
            text: qsTr("Comments")
            controlHeight: commandBar.chipHeight
            contentColor: Theme.colorText
            hoverColor: Theme.colorHover
            borderColor: Theme.colorPanelLine
        }

        ThemedToolButton {
            objectName: "editingButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.maximumHeight: commandBar.chipHeight
            font.pixelSize: Theme.fontSmall
            contentPadding: Theme.spacingTiny
            text: qsTr("Editing")
            showCaret: true
            controlHeight: commandBar.chipHeight
            contentColor: Theme.colorText
            hoverColor: Theme.colorHover
            borderColor: Theme.colorPanelLine
        }

        ThemedToolButton {
            id: shareButton
            objectName: "shareButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.maximumHeight: commandBar.chipHeight
            font.pixelSize: Theme.fontSmall
            contentPadding: Theme.spacingTiny
            text: qsTr("Share")
            controlHeight: commandBar.chipHeight
            contentColor: Theme.colorPanel
            hoverColor: Theme.colorTransparent
            pressedColor: Theme.colorTransparent
            background: Rectangle {
                radius: Theme.radiusSmall
                color: shareButton.hovered || shareButton.pressed ? Qt.darker(Theme.colorFocus, 1.08) : Theme.colorFocus
            }
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Theme.borderWidth
        color: Theme.colorChromeLine
    }
}
