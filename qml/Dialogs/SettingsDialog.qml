// 三端共享的设置窗口外壳；复用应用 DialogFrame 与按钮样式，页面由路由注册。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Caret.ViewModels

DialogWindow {
    id: dialog

    objectName: "settingsPageDialog"
    property var pages: []
    property string activeRouteId: "preferences"
    required property SettingsDialogController controller

    signal routeSelected(string routeId)
    width: Theme.settingsDialogWidth
    height: Theme.settingsDialogHeight
    title: qsTranslate("SettingsDialog", "Settings")
    escapeEnabled: !controller.busy
    onClosing: close => {
        if (controller.state !== SettingsDialogController.Closed) {
            close.accepted = false;
            controller.cancel();
        }
    }

    function open() {
        present();
    }

    DialogFrame {
        window: dialog

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: Theme.spacingMedium
            spacing: Theme.spacingMedium

            Rectangle {
                Layout.preferredWidth: Theme.settingsDialogSidebarWidth
                Layout.fillHeight: true
                color: Theme.colorChrome

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: Theme.spacingSmall
                    spacing: Theme.spacingXSmall

                    Repeater {
                        model: dialog.pages
                        delegate: ItemDelegate {
                            id: pageDelegate
                            required property var modelData
                            Layout.fillWidth: true
                            Accessible.name: pageDelegate.modelData.title
                            objectName: "settingsPageButton_" + pageDelegate.modelData.routeId
                            highlighted: pageDelegate.modelData.routeId === dialog.activeRouteId
                            focus: highlighted
                            text: pageDelegate.modelData.title
                            onClicked: {
                                dialog.activeRouteId = pageDelegate.modelData.routeId;
                                dialog.routeSelected(pageDelegate.modelData.routeId);
                            }

                            contentItem: RowLayout {
                                spacing: Theme.spacingSmall
                                ThemedIcon {
                                    Layout.preferredWidth: Theme.iconSizeSmall
                                    Layout.preferredHeight: Theme.iconSizeSmall
                                    name: pageDelegate.modelData.iconName
                                    preserveSourceColors: pageDelegate.modelData.preserveSourceColors ?? false
                                }
                                ThemedLabel {
                                    Layout.fillWidth: true
                                    text: pageDelegate.modelData.title
                                    textSize: Theme.fontBody
                                }
                            }
                            background: Rectangle {
                                color: pageDelegate.highlighted ? Theme.colorSelected : pageDelegate.hovered ? Theme.colorHover : Theme.colorTransparent
                            }
                        }
                    }

                    Item {
                        Layout.fillHeight: true
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: Theme.spacingMedium

                ThemedLabel {
                    objectName: "settingsPageTitle"
                    text: dialog.pages.find(page => page.routeId === dialog.activeRouteId)?.title || qsTranslate("SettingsDialog", "Settings")
                    textSize: Theme.fontTitle + Theme.spacingMedium
                }

                Loader {
                    objectName: "settingsPageLoader"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    sourceComponent: dialog.pages.find(page => page.routeId === dialog.activeRouteId)?.pageComponent || null
                }

                DialogButtonRow {
                    Layout.fillWidth: true
                    ThemedButton {
                        objectName: "settingsCancelButton"
                        text: qsTranslate("DialogAction", "Cancel")
                        enabled: !dialog.controller.busy
                        onClicked: dialog.controller.cancel()
                    }
                    ThemedButton {
                        objectName: "settingsOkButton"
                        text: qsTranslate("DialogAction", "OK")
                        primaryAction: true
                        enabled: !dialog.controller.busy
                        onClicked: dialog.controller.apply()
                    }
                }
            }
        }
    }
}
