// 文档关闭前询问如何处理未保存的修改。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

DialogWindow {
    id: dialog

    objectName: "closeDocumentConfirmation"
    signal saveRequested
    signal discardRequested

    width: Theme.dialogActionWidth * 3 + Theme.dialogActionSpacing * 2 + Theme.spacingLarge * 2
    height: frame.headerImplicitHeight + Theme.spacingMedium * 2 + promptLabel.implicitHeight + Theme.spacingLarge + actionsRow.implicitHeight
    title: qsTr("Unsaved changes")

    function open() {
        present();
    }

    DialogFrame {
        id: frame

        window: dialog
        closeButtonVisible: false

        ColumnLayout {
            objectName: "closeDialogContent"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: Theme.spacingLarge
            Layout.rightMargin: Theme.spacingLarge
            Layout.topMargin: Theme.spacingMedium
            Layout.bottomMargin: Theme.spacingMedium
            spacing: Theme.spacingLarge

            ThemedLabel {
                id: promptLabel
                objectName: "closePrompt"
                Layout.fillWidth: true
                text: qsTr("Save your changes before closing this document?")
                textSize: Theme.fontBody
                wrapMode: Text.Wrap
            }

            DialogButtonRow {
                id: actionsRow
                objectName: "closeDialogActions"
                Layout.fillWidth: true

                ThemedButton {
                    objectName: "closeSaveButton"
                    text: qsTr("Save")
                    primaryAction: true
                    onClicked: {
                        dialog.saveRequested();
                        dialog.close();
                    }
                }

                ThemedButton {
                    objectName: "closeDiscardButton"
                    text: qsTr("Don't Save")
                    onClicked: {
                        dialog.discardRequested();
                        dialog.close();
                    }
                }

                ThemedButton {
                    objectName: "closeCancelButton"
                    text: qsTr("Cancel")
                    onClicked: dialog.close()
                }
            }
        }
    }
}
