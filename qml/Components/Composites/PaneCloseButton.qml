// 面板右上角关闭入口；悬停/键盘焦点时红底白图标，由宿主处理关闭信号。
import QtQuick

ThemedToolButton {
    id: closeButton

    signal closeRequested

    anchors.top: parent.top
    anchors.right: parent.right
    anchors.topMargin: Theme.spacingXSmall
    anchors.rightMargin: Theme.spacingXSmall
    z: 2

    accessibleName: qsTranslate("IconActionClosePanel", "Close panel")
    iconName: "close"
    width: Theme.paneCloseSize
    controlHeight: Theme.paneCloseSize
    contentColor: hovered || visualFocus ? Theme.colorPanel : Theme.colorTextMuted
    iconSize: Theme.iconSizePaneClose
    contentPadding: 0
    hoverColor: Theme.colorCloseHover

    onClicked: closeButton.closeRequested()
}
