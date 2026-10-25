// 新建文档窗口壳。草稿和校验留在窗口内，通过信号提交文档信息。
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts

DialogWindow {
    id: dialog

    objectName: "newDocumentDialog"
    property string documentName: ""
    property string location: ""
    property string errorCode: ""
    property bool submitting: false
    escapeEnabled: !submitting
    signal documentAccepted(string name, string location)

    width: Theme.newProjectDialogWidth
    height: Theme.newProjectDialogHeight
    minimumWidth: Theme.newProjectDialogMinimumWidth
    minimumHeight: Theme.newProjectDialogMinimumHeight
    title: qsTranslate("NewProjectDialog", "New Document")
    onClosing: event => {
        if (submitting)
            event.accepted = false;
    }

    function resetFields() {
        errorCode = "";
        documentName = "";
        location = "";
        projectNameField.text = "";
        locationField.text = "";
    }

    function open() {
        if (submitting)
            return;
        resetFields();
        present();
        projectNameField.forceActiveFocus();
    }

    function submit() {
        if (!visible || submitting)
            return;
        const name = projectNameField.text.trim();
        const folder = locationField.text.trim();
        documentName = name;
        location = folder;
        if (name.length === 0) {
            errorCode = "document.empty_name";
            projectNameField.forceActiveFocus();
            return;
        }
        if (name.includes("/") || name.includes("\\") || name.includes(":")) {
            errorCode = "document.invalid_name";
            projectNameField.forceActiveFocus();
            return;
        }
        const absolute = folder.startsWith("/") || /^[A-Za-z]:[\\/]/.test(folder);
        if (!absolute) {
            errorCode = "document.location_not_absolute";
            locationField.forceActiveFocus();
            return;
        }
        errorCode = "";
        submitting = true;
        documentAccepted(name, folder);
        submitting = false;
        close();
    }

    function errorMessage() {
        if (errorCode === "document.empty_name" || errorCode === "document.invalid_name")
            return qsTranslate("NewProjectDialog", "Enter a valid document name.");
        if (errorCode === "document.location_not_absolute")
            return qsTranslate("NewProjectDialog", "Choose an absolute folder.");
        return "";
    }

    DialogFrame {
        window: dialog
        closeEnabled: !dialog.submitting

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
                text: qsTranslate("NewProjectDialog", "Create a document in the selected folder.")
                textSize: Theme.fontSmall
                textColor: Theme.colorTextMuted
                wrapMode: Text.Wrap
            }

            GridLayout {
                Layout.fillWidth: true
                enabled: !dialog.submitting
                columns: 2
                columnSpacing: Theme.spacingMedium
                rowSpacing: Theme.spacingMedium

                ThemedLabel {
                    text: qsTranslate("NewProjectDialog", "Document name")
                    textSize: Theme.fontBody
                }
                ThemedTextField {
                    id: projectNameField

                    objectName: "documentNameField"
                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.controlHeight
                    placeholderText: qsTranslate("NewProjectDialog", "My Document")
                    Accessible.name: qsTranslate("NewProjectDialog", "Document name")
                    invalid: dialog.errorCode === "document.empty_name" || dialog.errorCode === "document.invalid_name"
                    onAccepted: dialog.submit()
                }

                ThemedLabel {
                    text: qsTranslate("NewProjectDialog", "Create in")
                    textSize: Theme.fontBody
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSmall

                    ThemedTextField {
                        id: locationField

                        objectName: "documentLocationField"
                        Layout.fillWidth: true
                        Layout.preferredHeight: Theme.controlHeight
                        placeholderText: qsTranslate("NewProjectDialog", "Choose a folder")
                        Accessible.name: qsTranslate("NewProjectDialog", "Document location")
                        selectByMouse: true
                        invalid: dialog.errorCode === "document.location_not_absolute"
                    }
                    ThemedButton {
                        text: qsTranslate("UiCommonNavigation", "Browse")
                        iconName: "open"
                        preserveIconColors: true
                        contentPadding: Theme.spacingSmall
                        hoverColor: Theme.colorHover
                        borderColor: Theme.colorPanelLine
                        onClicked: folderDialog.open()
                    }
                }
            }

            ThemedLabel {
                Layout.fillWidth: true
                visible: dialog.errorCode.length > 0
                text: dialog.errorMessage()
                textSize: Theme.fontSmall
                textColor: Theme.colorError
                wrapMode: Text.Wrap
            }

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
            }

            DialogButtonRow {
                Layout.fillWidth: true
                spacing: Theme.spacingSmall

                ThemedButton {
                    objectName: "newDocumentAccept"
                    enabled: !dialog.submitting
                    text: qsTranslate("DialogAction", "OK")
                    primaryAction: true
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    onClicked: dialog.submit()
                }
                ThemedButton {
                    objectName: "newDocumentCancel"
                    enabled: !dialog.submitting
                    text: qsTranslate("DialogAction", "Cancel")
                    contentColor: Theme.colorText
                    hoverColor: Theme.colorHover
                    borderColor: Theme.colorPanelLine
                    onClicked: dialog.close()
                }
            }
        }
    }

    FolderDialog {
        id: folderDialog

        title: qsTranslate("NewProjectDialog", "Choose Document Location")
        onAccepted: {
            if (dialog.submitting)
                return;
            const selectedPath = selectedFolder.toString().replace(/^file:\/\//, "");
            if (selectedPath.length > 0) {
                locationField.text = decodeURIComponent(selectedPath);
                dialog.errorCode = "";
            }
        }
    }
}
