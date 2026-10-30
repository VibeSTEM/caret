// 显示崩溃或写入冲突留下的本机文档恢复项。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

DialogWindow {
    id: dialog

    objectName: "documentRecoveryDialog"
    signal restoreRequested(string id)
    signal exportRequested(string id)
    signal discardRequested(string id)
    signal dismissRequested

    property var entries: []
    property string selectedId: ""
    property bool busy: false
    readonly property var selectedEntry: entries.find(entry => entry.id === selectedId) || null
    escapeEnabled: false

    onEntriesChanged: {
        if (selectedId.length > 0 && !selectedEntry)
            selectedId = "";
    }

    width: Theme.dialogActionWidth * 3 + Theme.dialogActionSpacing * 2 + Theme.spacingLarge * 2
    height: Theme.titlebarHeight + Theme.controlHeight * 5 + Theme.spacingLarge * 8
    title: qsTr("Recover documents")

    function open() {
        present();
    }

    DialogFrame {
        window: dialog
        closeButtonVisible: false

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: Theme.spacingLarge
            Layout.rightMargin: Theme.spacingLarge
            Layout.topMargin: Theme.spacingMedium
            Layout.bottomMargin: Theme.spacingMedium
            spacing: Theme.spacingMedium

            ThemedLabel {
                Layout.fillWidth: true
                text: qsTr("Caret found document recovery files. Choose one to restore, or discard it.")
                textSize: Theme.fontBody
                wrapMode: Text.Wrap
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.selectedEntry && dialog.selectedEntry.kind === "external_version"
                text: qsTr("This is an external file backup. Save a copy to restore it outside Caret.")
                textSize: Theme.fontSmall
                color: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            ListView {
                id: recoveryList
                objectName: "recoveryEntryList"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: dialog.entries
                currentIndex: -1
                delegate: Rectangle {
                    id: delegateRoot
                    required property var modelData
                    required property int index

                    width: ListView.view.width
                    height: Theme.controlHeight * 1.5
                    radius: Theme.radiusSmall
                    color: dialog.selectedId === modelData.id ? Theme.colorSelected : Theme.colorPanel
                    border.width: 1
                    border.color: Theme.colorPanelLine

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: Theme.spacingSmall
                        spacing: Theme.spacingSmall / 2

                        ThemedLabel {
                            Layout.fillWidth: true
                            text: String(delegateRoot.modelData.originalPath).split(/[\\/]/).pop()
                            textSize: Theme.fontBody
                            elide: Text.ElideMiddle
                        }
                        ThemedLabel {
                            Layout.fillWidth: true
                            text: delegateRoot.modelData.originalPath
                            textSize: Theme.fontSmall
                            color: Theme.colorTextMuted
                            elide: Text.ElideMiddle
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: dialog.selectedId = delegateRoot.modelData.id
                    }
                }
            }

            DialogButtonRow {
                Layout.fillWidth: true

                ThemedButton {
                    objectName: "recoveryRestoreButton"
                    text: dialog.selectedEntry && dialog.selectedEntry.kind === "external_version" ? qsTr("Save backup copy") : qsTr("Restore")
                    primaryAction: true
                    enabled: dialog.selectedEntry !== null && !dialog.busy
                    onClicked: {
                        if (dialog.selectedEntry.kind === "external_version")
                            dialog.exportRequested(dialog.selectedId);
                        else
                            dialog.restoreRequested(dialog.selectedId);
                    }
                }
                ThemedButton {
                    objectName: "recoveryDiscardButton"
                    text: qsTr("Discard")
                    enabled: dialog.selectedEntry !== null && !dialog.busy
                    onClicked: dialog.discardRequested(dialog.selectedId)
                }
                ThemedButton {
                    objectName: "recoveryLaterButton"
                    text: qsTr("Later")
                    enabled: !dialog.busy
                    onClicked: {
                        dialog.dismissRequested();
                        dialog.close();
                    }
                }
            }
        }
    }
}
