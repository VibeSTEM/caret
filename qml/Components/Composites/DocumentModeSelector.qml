// 文档工作模式选择器：展示 Editing、Review、View 三种模式及其当前选择。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: selector

    property string mode: "editing"
    signal modeSelected(string mode)
    onModeSelected: value => selector.mode = value

    readonly property int menuWidth: Theme.searchFieldWidth + Theme.spacingLarge * 2
    readonly property int optionHeight: Theme.toolbarButtonHeight + Theme.spacingLarge

    implicitWidth: modeButton.implicitWidth
    implicitHeight: modeButton.implicitHeight

    function labelFor(value) {
        if (value === "review")
            return qsTr("Review");
        if (value === "view")
            return qsTr("View");
        return qsTr("Editing");
    }

    function descriptionFor(value) {
        if (value === "review")
            return qsTr("Suggest changes and add comments");
        if (value === "view")
            return qsTr("View the document without editing");
        return qsTr("Make changes to the document");
    }

    function iconFor(value) {
        if (value === "review")
            return "mode-review";
        if (value === "view")
            return "mode-view";
        return "mode-edit";
    }

    function choose(value) {
        modeSelected(value);
        modeMenu.close();
    }

    ThemedToolButton {
        id: modeButton
        objectName: "editingButton"
        anchors.fill: parent
        text: selector.labelFor(selector.mode)
        iconName: selector.iconFor(selector.mode)
        showCaret: true
        font.pixelSize: Theme.fontSmall
        contentPadding: Theme.spacingTiny
        contentColor: Theme.colorText
        hoverColor: Theme.colorHover
        borderColor: Theme.colorPanelLine
        onClicked: modeMenu.open()
    }

    Popup {
        id: modeMenu
        objectName: "documentModeMenu"
        parent: Overlay.overlay
        x: Overlay.overlay ? modeButton.mapToItem(Overlay.overlay, 0, modeButton.height).x : 0
        y: Overlay.overlay ? modeButton.mapToItem(Overlay.overlay, 0, modeButton.height).y : 0
        width: selector.menuWidth
        padding: Theme.spacingXSmall
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        background: Rectangle {
            radius: Theme.radiusSmall
            color: Theme.colorPanel
            border.width: Theme.borderWidth
            border.color: Theme.colorPanelLine
        }

        contentItem: Column {
            id: options
            spacing: Theme.spacingTiny

            Repeater {
                model: ["editing", "review", "view"]

                delegate: Item {
                    id: option
                    objectName: "documentModeOption-" + modelData

                    required property string modelData

                    readonly property bool selected: selector.mode === modelData
                    readonly property string title: selector.labelFor(modelData)
                    readonly property string description: selector.descriptionFor(modelData)

                    width: options.width
                    height: selector.optionHeight
                    Accessible.name: title + ". " + description
                    Accessible.role: Accessible.Button
                    Accessible.onPressAction: selector.choose(option.modelData)

                    Rectangle {
                        anchors.fill: parent
                        radius: Theme.radiusSmall
                        color: optionMouseArea.containsMouse ? Theme.colorHover : Theme.colorTransparent
                    }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Theme.spacingSmall
                        anchors.rightMargin: Theme.spacingSmall
                        spacing: Theme.spacingSmall

                        ThemedIcon {
                            Layout.alignment: Qt.AlignVCenter
                            name: option.selected ? "check" : ""
                            iconSize: Theme.iconSizeCompact
                            color: Theme.colorText
                        }

                        ThemedIcon {
                            Layout.alignment: Qt.AlignVCenter
                            name: selector.iconFor(option.modelData)
                            iconSize: Theme.iconSizeDefault
                            color: Theme.colorIcon
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            ThemedLabel {
                                Layout.fillWidth: true
                                text: option.title
                                textSize: Theme.fontBody
                                textColor: Theme.colorText
                                font.weight: option.selected ? Font.DemiBold : Font.Normal
                            }

                            ThemedLabel {
                                Layout.fillWidth: true
                                text: option.description
                                textSize: Theme.fontSmall
                                textColor: Theme.colorTextMuted
                                elide: Text.ElideRight
                            }
                        }
                    }

                    MouseArea {
                        id: optionMouseArea
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: selector.choose(option.modelData)
                    }
                }
            }
        }
    }
}
