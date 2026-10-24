// 共用分段页签：紧凑等宽的移动选中块；TabBar 保留键盘导航和互斥状态。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

TabBar {
    id: bar
    objectName: "panelTabs"

    property alias tabs: repeater.model
    property int edge: Qt.TopEdge
    signal tabActivated(int index)

    implicitWidth: count * Theme.tabSegmentWidth + leftPadding + rightPadding
    implicitHeight: Theme.panelToolbarHeight
    padding: Theme.spacingTiny
    spacing: 0

    background: Rectangle {
        color: Theme.colorChrome

        Rectangle {
            anchors.top: bar.edge === Qt.BottomEdge ? parent.top : undefined
            anchors.bottom: bar.edge === Qt.TopEdge ? parent.bottom : undefined
            width: parent.width
            height: Theme.borderWidth
            color: Theme.colorPanelLine
        }
    }

    contentItem: ListView {
        id: tabList
        objectName: "segmentedTabList"
        implicitWidth: contentWidth
        implicitHeight: Theme.panelToolbarHeight - 2 * Theme.spacingTiny
        model: bar.contentModel
        currentIndex: bar.currentIndex
        orientation: ListView.Horizontal
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        highlightMoveDuration: Theme.tabSlideDuration
        highlightResizeDuration: 0
        highlight: Item {
            Rectangle {
                objectName: "tabSelectedSurface"
                anchors.fill: parent
                anchors.margins: Theme.spacingTiny
                radius: height / 2
                color: Theme.colorPanel
            }
        }
    }

    Repeater {
        id: repeater

        delegate: TabButton {
            id: tab
            hoverEnabled: true
            required property int index
            required property string modelData

            text: modelData
            onClicked: bar.tabActivated(index)
            width: Math.min(Theme.tabSegmentWidth, bar.availableWidth / Math.max(1, bar.count))
            // 与 ListView 内容高度同源，避免 TabBar 按旧隐式高度居中而产生负 y。
            implicitHeight: Theme.panelToolbarHeight - 2 * Theme.spacingTiny
            height: bar.availableHeight
            leftPadding: Theme.spacingMedium
            rightPadding: Theme.spacingMedium
            topPadding: 0
            bottomPadding: 0
            z: 1
            background: Item {
                Rectangle {
                    objectName: "tabHoverSurface"
                    anchors.fill: parent
                    anchors.margins: Theme.spacingTiny
                    radius: height / 2
                    color: !tab.checked && tab.hovered ? Theme.colorHover : Theme.colorTransparent
                    border.width: tab.visualFocus ? Theme.focusBorderWidth : 0
                    border.color: Theme.colorFocus
                }
            }
            contentItem: ThemedLabel {
                text: tab.text
                elide: Text.ElideRight
                textSize: Theme.fontSmall
                textColor: tab.checked ? Theme.colorText : Theme.colorTextMuted
                font.weight: tab.checked ? Font.DemiBold : Font.Normal
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
        }
    }
}
