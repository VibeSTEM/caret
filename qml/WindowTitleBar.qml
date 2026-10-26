// 自定义标题栏。高度锁在 Theme.titlebarHeight，控件在这一行里垂直居中。
// 文档名和搜索还没接命令。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import QtQuick.Layouts

Rectangle {
    id: titleBar

    required property Window window
    property real leftInset: 0
    property real rightInset: 0
    signal settingsRequested
    signal saveRequested
    property string documentName: qsTr("Untitled")

    readonly property int toolSide: Math.min(Theme.toolbarButtonHeight, Theme.titlebarHeight - Theme.spacingSmall)
    readonly property int fieldHeight: Math.min(Theme.searchHeight, Theme.titlebarHeight - Theme.spacingSmall)
    // 非 macOS 左侧没有系统按钮。内容边距与 Home 文案对齐，避免 AutoSave 贴住窗口边。
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
        anchors.fill: parent
        anchors.leftMargin: titleBar.leadingMargin
        anchors.rightMargin: titleBar.trailingMargin
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
            objectName: "saveButton"
            iconName: "save"
            accessibleName: qsTr("Save")
            onClicked: titleBar.saveRequested()
        }

        TitleToolButton {
            objectName: "undoButton"
            iconName: "undo"
            accessibleName: qsTr("Undo")
        }

        TitleToolButton {
            objectName: "redoButton"
            iconName: "redo"
            accessibleName: qsTr("Redo")
        }

        Item {
            id: captionArea
            objectName: "shellCaptionDragArea"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumWidth: Theme.spacingLarge
            Layout.maximumHeight: titleBar.height

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

        Rectangle {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: Theme.searchFieldWidth
            Layout.preferredHeight: titleBar.fieldHeight
            Layout.maximumHeight: titleBar.fieldHeight
            radius: Theme.radiusSmall
            clip: true
            color: Theme.colorPanel
            border.width: Theme.borderWidth
            border.color: searchField.activeFocus ? Theme.colorFocus : Theme.colorPanelLine

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.spacingXSmall
                anchors.rightMargin: Theme.spacingXSmall
                spacing: Theme.spacingXSmall

                ThemedIcon {
                    name: "search"
                    iconSize: Theme.iconSizeCompact
                    color: Theme.colorTextMuted
                }

                ThemedTextField {
                    id: searchField
                    objectName: "commandSearch"
                    Accessible.name: qsTr("Search commands and help")
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    placeholderText: qsTr("Search commands and help")
                    font.pixelSize: Theme.fontSmall
                    background: Rectangle {
                        color: Theme.colorTransparent
                        border.width: 0
                    }
                }
            }
        }

        TitleToolButton {
            objectName: "settingsButton"
            iconName: "settings"
            accessibleName: qsTr("Settings")
            onClicked: titleBar.settingsRequested()
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
