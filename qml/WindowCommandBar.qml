// 工具栏高度锁在 Theme.menubarHeight；当前显示 Home 标签。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: commandBar

    readonly property int rowHeight: Theme.menubarHeight

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
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Theme.borderWidth
        color: Theme.colorChromeLine
    }
}
