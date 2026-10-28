// 外部程序改写文档后，明确选择另存副本、备份后替换或取消。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

DialogWindow {
    id: dialog

    objectName: "saveConflictDialog"
    signal saveCopyRequested
    signal replaceRequested
    signal cancelRequested
    property bool actionSelected: false

    required property string conflictedFileName
    property bool replaceAllowed: false
    property string recoveryErrorDetail: ""
    property bool recoveryWriteFailed: false

    width: Theme.dialogActionWidth * 3 + Theme.dialogActionSpacing * 2 + Theme.spacingLarge * 2
    height: Theme.titlebarHeight + messageLabel.implicitHeight + Theme.controlHeight + Theme.spacingLarge * 5
    title: qsTr("File changed outside Caret")

    function open() {
        actionSelected = false;
        present();
    }

    onVisibleChanged: {
        if (!visible && !actionSelected)
            cancelRequested();
        actionSelected = false;
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
            spacing: Theme.spacingLarge

            ThemedLabel {
                id: messageLabel
                Layout.fillWidth: true
                text: {
                    const message = qsTr("Caret cannot safely overwrite %1 because it changed, moved, or is unavailable. Save a copy, replace the current file after backing it up when possible, or cancel.").arg(dialog.conflictedFileName);
                    if (dialog.recoveryErrorDetail.length === 0)
                        return message;
                    const failure = dialog.recoveryWriteFailed ? qsTr("The recovery copy could not be written:") : qsTr("The recovery copy could not be linked to this document:");
                    return message + "\n\n" + failure + " " + dialog.recoveryErrorDetail;
                }
                textSize: Theme.fontBody
                wrapMode: Text.Wrap
            }

            DialogButtonRow {
                Layout.fillWidth: true

                ThemedButton {
                    objectName: "saveConflictCopyButton"
                    text: qsTr("Save a Copy")
                    primaryAction: true
                    onClicked: {
                        dialog.actionSelected = true;
                        dialog.saveCopyRequested();
                        dialog.close();
                    }
                }

                ThemedButton {
                    objectName: "saveConflictReplaceButton"
                    text: qsTr("Replace")
                    visible: dialog.replaceAllowed
                    onClicked: {
                        dialog.actionSelected = true;
                        dialog.replaceRequested();
                        dialog.close();
                    }
                }

                ThemedButton {
                    objectName: "saveConflictCancelButton"
                    text: qsTr("Cancel")
                    onClicked: {
                        dialog.actionSelected = true;
                        dialog.cancelRequested();
                        dialog.close();
                    }
                }
            }
        }
    }
}
