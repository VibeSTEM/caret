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
    readonly property var modes: ["editing", "review", "view"]

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
        focus: true

        onOpened: {
            const index = Math.max(0, selector.modes.indexOf(selector.mode));
            modeOptions.itemAt(index).forceActiveFocus();
        }
        onClosed: modeButton.forceActiveFocus()

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
                id: modeOptions
                model: selector.modes

                delegate: Item {
                    id: option
                    objectName: "documentModeOption-" + modelData

                    required property string modelData
                    required property int index

                    readonly property bool selected: selector.mode === modelData
                    readonly property string title: selector.labelFor(modelData)
                    readonly property string description: selector.descriptionFor(modelData)

                    width: options.width
                    height: selector.optionHeight
                    activeFocusOnTab: true
                    Accessible.name: title + ". " + description
                    Accessible.role: Accessible.MenuItem
                    Accessible.onPressAction: selector.choose(option.modelData)

                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Up || event.key === Qt.Key_Down) {
                            const offset = event.key === Qt.Key_Up ? -1 : 1;
                            const nextIndex = (option.index + selector.modes.length + offset) % selector.modes.length;
                            modeOptions.itemAt(nextIndex).forceActiveFocus();
                            event.accepted = true;
                        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
                            selector.choose(option.modelData);
                            event.accepted = true;
                        } else if (event.key === Qt.Key_Escape) {
                            modeMenu.close();
                            event.accepted = true;
                        }
                    }

                    Rectangle {
                        anchors.fill: parent
                        radius: Theme.radiusSmall
                        color: optionMouseArea.containsMouse || option.activeFocus ? Theme.colorHover : Theme.colorTransparent
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
