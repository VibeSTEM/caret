// 自定义标题栏。高度锁在 Theme.titlebarHeight，控件在这一行里垂直居中。
// 文档文件操作由 ViewModel 执行，搜索仍为界面占位。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import Caret.ViewModels
import QtQuick.Layouts

Rectangle {
    id: titleBar

    required property Window window
    property real leftInset: 0
    property real rightInset: 0
    signal settingsRequested
    signal openRequested
    signal saveRequested
    signal undoRequested
    signal redoRequested
    property string documentName: qsTr("Untitled")

    readonly property int toolSide: Math.min(Theme.toolbarButtonHeight, Theme.titlebarHeight - Theme.spacingSmall)
    // 系统窗口控件占用的区域由 WindowChromeMetrics 提供，QML 只增加主题间距。
    readonly property real leadingMargin: leftInset > 0 ? leftInset + Theme.spacingSmall : Theme.spacingMedium + Theme.spacingLarge
    readonly property real trailingMargin: rightInset > 0 ? rightInset + Theme.spacingSmall : Theme.spacingMedium + Theme.spacingLarge

    implicitHeight: Theme.titlebarHeight
    clip: true
    color: Theme.colorChrome

    component TitleToolButton: ThemedToolButton {
        Layout.alignment: Qt.AlignVCenter
        Layout.preferredWidth: titleBar.toolSide
        Layout.preferredHeight: titleBar.toolSide
        Layout.maximumHeight: titleBar.toolSide
        controlHeight: titleBar.toolSide
        contentPadding: 0
        iconSize: Theme.iconSizeSmall
    }

    RowLayout {
        id: leftControls
        anchors.left: parent.left
        anchors.leftMargin: titleBar.leadingMargin
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.spacingSmall

        ThemedSwitch {
            id: autoSave
            objectName: "autoSaveSwitch"
            Layout.alignment: Qt.AlignVCenter
            Layout.fillHeight: false
            Layout.preferredHeight: autoSave.trackHeight
            Layout.maximumHeight: autoSave.trackHeight
            text: qsTr("AutoSave")
            checked: false
        }

        TitleToolButton {
            objectName: "openButton"
            iconName: "open"
            accessibleName: qsTr("Open")
            onClicked: titleBar.openRequested()
        }

        TitleToolButton {
            objectName: "saveButton"
            iconName: "save"
            accessibleName: qsTr("Save")
            onClicked: titleBar.saveRequested()
        }

        TitleToolButton {
            objectName: "undoButton"
            iconName: "undo"
            accessibleName: qsTr("Undo")
            enabled: LayoutViewModel.canUndo
            onClicked: titleBar.undoRequested()
        }

        TitleToolButton {
            objectName: "redoButton"
            iconName: "redo"
            accessibleName: qsTr("Redo")
            enabled: LayoutViewModel.canRedo
            onClicked: titleBar.redoRequested()
        }
    }

    Item {
        id: captionArea
        objectName: "shellCaptionDragArea"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(0, titleBar.width - 2 * Math.max(leftControls.implicitWidth + titleBar.leadingMargin, rightControls.implicitWidth + titleBar.trailingMargin))
        height: titleBar.height
        z: 0

        ThemedLabel {
            anchors.fill: parent
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            text: titleBar.documentName
            textColor: Theme.colorText
            textSize: Theme.fontBody
            font.weight: Font.DemiBold
        }

        // 扩展客户区盖住了系统标题栏。拖动和双击交给平台窗口管理器。
        DragHandler {
            target: null
            acceptedButtons: Qt.LeftButton
            onActiveChanged: {
                if (active && titleBar.window)
                    titleBar.window.startSystemMove();
            }
        }

        TapHandler {
            acceptedButtons: Qt.LeftButton
            exclusiveSignals: TapHandler.DoubleTap
            onDoubleTapped: (eventPoint, button) => {
                if (button === Qt.LeftButton)
                    WindowChromeMetrics.performTitlebarDoubleClickAction();
            }
        }
    }

    RowLayout {
        id: rightControls
        anchors.right: parent.right
        anchors.rightMargin: titleBar.trailingMargin
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.spacingSmall

        TitleToolButton {
            objectName: "commentsButton"
            iconName: "comments"
            accessibleName: qsTr("Comments")
        }

        DocumentModeSelector {
            id: documentModeSelector
            objectName: "documentModeSelector"
            Layout.alignment: Qt.AlignVCenter
            Layout.maximumHeight: titleBar.toolSide
        }

        ThemedToolButton {
            id: shareButton
            objectName: "shareButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.maximumHeight: titleBar.toolSide
            font.pixelSize: Theme.fontSmall
            contentPadding: Theme.spacingTiny
            iconName: "share"
            text: qsTr("Share")
            showCaret: true
            controlHeight: titleBar.toolSide
            contentColor: Theme.colorPanel
            hoverColor: Theme.colorTransparent
            pressedColor: Theme.colorTransparent
            background: Rectangle {
                radius: Theme.radiusSmall
                color: shareButton.hovered || shareButton.pressed ? Qt.darker(Theme.colorFocus, 1.08) : Theme.colorFocus
            }
        }

        TitleToolButton {
            objectName: "searchButton"
            iconName: "search"
            accessibleName: qsTr("Search commands and help")
            cornerRadius: titleBar.toolSide / 2
            borderColor: Theme.colorPanelLine
        }

        TitleToolButton {
            objectName: "settingsButton"
            iconName: "settings"
            accessibleName: qsTr("Settings")
            onClicked: titleBar.settingsRequested()
        }
    }
}
