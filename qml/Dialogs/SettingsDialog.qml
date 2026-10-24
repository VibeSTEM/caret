// 三端共享的设置窗口外壳；复用应用 DialogFrame 与按钮样式，页面由路由注册。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

DialogWindow {
    id: dialog

    objectName: "settingsPageDialog"
    property var pages: []
    property string activeRouteId: "general"
    property bool intentionalClose: false

    signal routeSelected(string routeId)
    signal acceptRequested
    signal accepted
    signal cancelled

    function completeAccept() {
        intentionalClose = true;
        accepted();
        close();
    }

    width: Theme.settingsDialogWidth
    height: Theme.settingsDialogHeight
    title: qsTranslate("SettingsDialog", "Settings")
    onClosing: {
        if (!intentionalClose)
            cancelled();
        intentionalClose = false;
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
                            onClicked: dialog.routeSelected(pageDelegate.modelData.routeId)

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
                    sourceComponent: dialog.pages.find(page => page.routeId === dialog.activeRouteId)?.pageComponent || unavailablePage
                }

                DialogButtonRow {
                    Layout.fillWidth: true
                    ThemedButton {
                        objectName: "settingsCancelButton"
                        text: qsTranslate("DialogAction", "Cancel")
                        onClicked: {
                            dialog.intentionalClose = true;
                            dialog.cancelled();
                            dialog.close();
                        }
                    }
                    ThemedButton {
                        objectName: "settingsOkButton"
                        text: qsTranslate("DialogAction", "OK")
                        primaryAction: true
                        onClicked: dialog.acceptRequested()
                    }
                }
            }
        }
    }

    Component {
        id: unavailablePage

        ColumnLayout {
            spacing: Theme.spacingMedium

            ThemedLabel {
                Layout.fillWidth: true
                text: qsTranslate("SettingsDialog", "Common application preferences are managed here.")
                textColor: Theme.colorTextMuted
                textSize: Theme.fontBody
                wrapMode: Text.WordWrap
            }

            PanelSurface {
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.applicationMenuHeight / 4

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: Theme.spacingMedium
                    spacing: Theme.spacingSmall

                    ThemedLabel {
                        Layout.fillWidth: true
                        text: qsTranslate("SettingsDialog", "Preferences")
                        textSize: Theme.fontBody
                    }
                    ThemedLabel {
                        Layout.fillWidth: true
                        text: qsTranslate("SettingsDialog", "Settings pages can be registered in this shared dialog on Windows, Linux, and macOS.")
                        textColor: Theme.colorTextMuted
                        textSize: Theme.fontSmall
                        wrapMode: Text.WordWrap
                    }
                }
            }
        }
    }
}
