// 文档工作模式选择器：展示 Edit、Review、View 三种模式及其当前选择。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: selector

    property string mode: "editing"
    signal modeSelected(string mode)
    onModeSelected: value => selector.mode = value

    property int buttonWidth: naturalButtonWidth
    property int menuWidth: buttonWidth * 2 + Theme.spacingSmall
    readonly property int menuGap: Math.ceil(Theme.spacingSmall / 2)
    readonly property var choices: [
        {
            id: "editing",
            label: qsTr("Edit"),
            description: qsTr("Make changes to the document"),
            icon: "mode-edit"
        },
        {
            id: "review",
            label: qsTr("Review"),
            description: qsTr("Suggest changes and add comments"),
            icon: "mode-review"
        },
        {
            id: "view",
            label: qsTr("View"),
            description: qsTr("View the document without editing"),
            icon: "mode-view"
        }
    ]

    implicitWidth: buttonWidth
    implicitHeight: modeButton.implicitHeight

    TextMetrics {
        id: editLabelMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTr("Edit")
    }

    TextMetrics {
        id: reviewLabelMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTr("Review")
    }

    TextMetrics {
        id: viewLabelMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTr("View")
    }

    readonly property int naturalButtonWidth: Math.ceil(Math.max(editLabelMetrics.width, reviewLabelMetrics.width, viewLabelMetrics.width)) + Theme.iconSizeDefault + Theme.iconSizeCompact + Theme.spacingXSmall * 2 + Theme.spacingSmall * 2 + Theme.borderWidth * 2

    function choiceFor(value) {
        return choices.find(choice => choice.id === value) ?? choices[0];
    }

    function choose(value) {
        modeSelected(value);
        modeMenu.close();
    }

    function positionMenu() {
        const overlay = modeMenu.parent;
        if (!overlay)
            return;

        const anchor = modeButton.mapToItem(overlay, 0, modeButton.height);
        const margin = Theme.spacingSmall;
        const rightLimit = Math.max(margin, overlay.width - modeMenu.width - margin);
        let menuY = anchor.y + menuGap;

        if (menuY + modeMenu.height > overlay.height - margin)
            menuY = modeButton.mapToItem(overlay, 0, 0).y - modeMenu.height - menuGap;

        modeMenu.x = Math.max(margin, Math.min(anchor.x, rightLimit));
        modeMenu.y = Math.max(margin, Math.min(menuY, overlay.height - modeMenu.height - margin));
    }

    function toggleMenu() {
        if (modeMenu.visible)
            modeMenu.close();
        else
            modeMenu.open();
    }

    TitleBarPillButton {
        id: modeButton
        objectName: "documentModeButton"
        anchors.fill: parent
        implicitWidth: selector.buttonWidth
        text: selector.choiceFor(selector.mode).label
        iconName: selector.choiceFor(selector.mode).icon
        showCaret: true
        onClicked: selector.toggleMenu()
    }

    Popup {
        id: modeMenu
        objectName: "documentModeMenu"
        parent: Overlay.overlay
        width: selector.menuWidth
        padding: Theme.spacingSmall
        // 指针在触发按钮上时由按钮切换菜单，避免 Popup 与 clicked 重复处理同一次点击。
        closePolicy: Popup.CloseOnEscape | (modeButton.hovered ? Popup.NoAutoClose : Popup.CloseOnPressOutside)
        focus: true

        onAboutToShow: selector.positionMenu()
        onOpened: {
            const index = Math.max(0, selector.choices.findIndex(choice => choice.id === selector.mode));
            modeOptions.itemAt(index).forceActiveFocus();
        }
        onClosed: modeButton.forceActiveFocus()

        background: Rectangle {
            radius: Theme.radiusSmall + Theme.spacingSmall
            color: Qt.rgba(Theme.colorPanel.r, Theme.colorPanel.g, Theme.colorPanel.b, 0.96)
            border.width: Theme.borderWidth
            border.color: Theme.colorPanelLine
        }

        contentItem: Column {
            id: options
            spacing: Theme.spacingTiny

            Repeater {
                id: modeOptions
                model: selector.choices

                delegate: Item {
                    id: option
                    objectName: "documentModeOption-" + modelData.id

                    required property var modelData
                    required property int index

                    readonly property bool selected: selector.mode === modelData.id
                    readonly property string title: modelData.label
                    readonly property string description: modelData.description

                    width: options.width
                    height: optionContent.implicitHeight + Theme.spacingSmall * 2
                    activeFocusOnTab: true
                    Accessible.name: title + ". " + description
                    Accessible.role: Accessible.MenuItem
                    Accessible.onPressAction: selector.choose(option.modelData.id)

                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Up || event.key === Qt.Key_Down) {
                            const offset = event.key === Qt.Key_Up ? -1 : 1;
                            const nextIndex = (option.index + selector.choices.length + offset) % selector.choices.length;
                            modeOptions.itemAt(nextIndex).forceActiveFocus();
                            event.accepted = true;
                        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
                            selector.choose(option.modelData.id);
                            event.accepted = true;
                        }
                    }

                    Rectangle {
                        anchors.fill: parent
                        radius: Theme.radiusSmall + Theme.spacingTiny
                        color: optionMouseArea.containsMouse || option.activeFocus ? Theme.colorHover : Theme.colorTransparent
                    }

                    RowLayout {
                        id: optionContent
                        anchors.fill: parent
                        anchors.leftMargin: Theme.spacingSmall
                        anchors.rightMargin: Theme.spacingSmall
                        anchors.topMargin: Theme.spacingSmall
                        anchors.bottomMargin: Theme.spacingSmall
                        spacing: Theme.spacingSmall

                        ThemedIcon {
                            Layout.alignment: Qt.AlignVCenter
                            name: option.selected ? "check" : ""
                            iconSize: Theme.iconSizeCompact
                            color: Theme.colorText
                        }

                        ThemedIcon {
                            Layout.alignment: Qt.AlignVCenter
                            name: option.modelData.icon
                            iconSize: Theme.iconSizeDefault
                            color: Theme.colorIcon
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacingTiny

                            ThemedLabel {
                                Layout.fillWidth: true
                                text: option.title
                                textSize: Theme.fontBody
                                textColor: Theme.colorText
                                font.weight: Font.DemiBold
                            }

                            ThemedLabel {
                                Layout.fillWidth: true
                                text: option.description
                                textSize: Theme.fontSmall
                                textColor: Theme.colorTextMuted
                                wrapMode: Text.WordWrap
                                maximumLineCount: 2
                                elide: Text.ElideNone
                            }
                        }
                    }

                    MouseArea {
                        id: optionMouseArea
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: selector.choose(option.modelData.id)
                    }
                }
            }
        }
    }
}
