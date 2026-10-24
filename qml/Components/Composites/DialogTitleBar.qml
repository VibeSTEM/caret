// 无边框对话框标题栏：提供系统级移动尝试、键盘焦点和关闭语义。
import QtQuick
import QtQuick.Layouts

Rectangle {
    id: titleBar

    required property Window window
    property string caption: ""
    property bool closeEnabled: true
    signal closeRequested

    implicitHeight: Theme.titlebarHeight + Theme.spacingSmall
    color: Theme.colorChrome

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingMedium
        anchors.rightMargin: Theme.spacingXSmall
        spacing: Theme.spacingSmall

        ThemedLabel {
            Layout.fillWidth: true
            text: titleBar.caption
            textSize: Theme.fontTitle
            textColor: Theme.colorText
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            verticalAlignment: Text.AlignVCenter
        }

        ThemedToolButton {
            id: closeButton
            objectName: "dialogCloseButton"
            enabled: titleBar.closeEnabled
            Layout.alignment: Qt.AlignTop
            Layout.topMargin: Theme.spacingXSmall
            Layout.preferredWidth: Theme.paneCloseSize
            Layout.preferredHeight: Theme.paneCloseSize
            controlHeight: Theme.paneCloseSize
            contentPadding: 0
            iconName: "close"
            iconSize: Theme.iconSizeCompact
            accessibleName: qsTranslate("IconActionCloseDialog", "Close dialog")
            contentColor: hovered || visualFocus ? Theme.colorPanel : Theme.colorTextMuted
            hoverColor: Theme.colorCloseHover
            onClicked: titleBar.closeRequested()
        }
    }

    MouseArea {
        id: moveArea
        anchors.fill: parent
        anchors.rightMargin: closeButton.width + Theme.spacingSmall
        acceptedButtons: Qt.LeftButton
        hoverEnabled: true

        property bool manualMove: false
        property point pressPosition

        onPressed: mouse => {
            pressPosition = Qt.point(mouse.x, mouse.y);
            manualMove = !titleBar.window.startSystemMove();
            mouse.accepted = true;
        }
        onPositionChanged: mouse => {
            if (!manualMove || !pressed)
                return;
            titleBar.window.x += mouse.x - pressPosition.x;
            titleBar.window.y += mouse.y - pressPosition.y;
        }
        onReleased: manualMove = false
        onCanceled: manualMove = false
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Theme.borderWidth
        color: Theme.colorPanelLine
    }
}
