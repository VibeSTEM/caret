// 文档窗口外壳。页面、字数和语言是占位，设置齿轮打开已有的设置对话框。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import QtQuick.Layouts
import QtQuick.Dialogs
import Caret.Shell
import Caret.ViewModels

Window {
    id: window

    objectName: "caretWindow"
    visible: true
    width: PreferencesViewModel.windowWidth
    height: PreferencesViewModel.windowHeight
    minimumWidth: PreferencesViewModel.minimumWindowWidth
    minimumHeight: PreferencesViewModel.minimumWindowHeight
    // Linux XCB 会把空标题换成应用名。零宽字符保留窗口按钮，同时不显示文字。
    title: Qt.platform.os === "linux" ? "\u200B" : ""
    color: Theme.colorChrome
    // Windows 隐藏系统标题但保留原生按钮。Linux 显式保留空标题和系统按钮提示。
    flags: Qt.Window | Qt.ExpandedClientAreaHint | Qt.NoTitleBarBackgroundHint | (Qt.platform.os === "windows" ? Qt.CustomizeWindowHint | Qt.WindowMinimizeButtonHint | Qt.WindowMaximizeButtonHint | Qt.WindowCloseButtonHint : Qt.platform.os === "linux" ? Qt.CustomizeWindowHint | Qt.WindowTitleHint | Qt.WindowMinimizeButtonHint | Qt.WindowMaximizeButtonHint | Qt.WindowCloseButtonHint : 0)

    readonly property string documentName: LayoutViewModel.documentFileName + (LayoutViewModel.documentSaveStatus === "saving" ? " — Saving" : LayoutViewModel.documentSaveStatus === "edited" ? " — Edited" : " — Saved")
    property url suggestedSaveFileUrl: ""
    property bool allowClose: false
    property bool waitForCloseAfterFileOperation: false
    property bool closeAfterSaveRequested: false

    onClosing: close => {
        if (allowClose)
            return;
        close.accepted = false;
        if (LayoutViewModel.documentFileBusy) {
            waitForCloseAfterFileOperation = true;
            return;
        }
        if (LayoutViewModel.documentEdited)
            closeConfirmation.open();
        else
            closeWindow();
    }

    Component.onCompleted: WindowChromeMetrics.observeWindow(window)

    Item {
        id: root
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            WindowTitleBar {
                Layout.fillWidth: true
                Layout.minimumHeight: Theme.titlebarHeight
                Layout.preferredHeight: Theme.titlebarHeight
                Layout.maximumHeight: Theme.titlebarHeight
                window: window
                leftInset: WindowChromeMetrics.leadingSystemControlsInset
                rightInset: WindowChromeMetrics.trailingSystemControlsInset
                onSettingsRequested: settingsController.open()
                onOpenRequested: openDialog.open()
                documentName: window.documentName
                onSaveRequested: window.saveDocument()
                onUndoRequested: LayoutViewModel.undo()
                onRedoRequested: LayoutViewModel.redo()
            }

            WindowCommandBar {
                Layout.fillWidth: true
                Layout.minimumHeight: Theme.menubarHeight
                Layout.preferredHeight: Theme.menubarHeight
                Layout.maximumHeight: Theme.menubarHeight
            }

            PageSurface {
                id: pageSurface
                Layout.fillWidth: true
                Layout.fillHeight: true
                enabled: !LayoutViewModel.documentOpening
            }

            WindowStatusBar {
                Layout.fillWidth: true
                Layout.minimumHeight: Theme.statusbarHeight
                Layout.preferredHeight: Theme.statusbarHeight
                Layout.maximumHeight: Theme.statusbarHeight
                onZoomEdited: value => pageSurface.setZoomAtViewportCenter(value)
            }
        }
    }

    SettingsDialogController {
        id: settingsController
        preferences: PreferencesViewModel
        onOpened: settingsDialog.open()
        onClosed: settingsDialog.close()
    }

    SettingsDialog {
        id: settingsDialog
        objectName: "settingsDialog"
        ownerWindow: window
        controller: settingsController
        pages: [
            {
                routeId: "preferences",
                title: qsTranslate("SettingsDialog", "Preferences"),
                iconName: "settings",
                pageComponent: preferencesSettingsPage
            }
        ]
    }

    Component {
        id: preferencesSettingsPage

        PreferencesSettingsPage {
            caretBlinkEnabled: settingsController.draftCaretBlinkEnabled
            errorText: settingsController.error
            enabled: !settingsController.busy
            onCaretBlinkEnabledEdited: enabled => settingsController.editCaretBlinkEnabled(enabled)
        }
    }

    FileDialog {
        id: openDialog
        objectName: "openDocumentDialog"
        title: qsTr("Open document")
        fileMode: FileDialog.OpenFile
        nameFilters: [qsTr("Caret documents (*.cdoc)")]
        acceptLabel: qsTr("Open")
        onAccepted: {
            const result = LayoutViewModel.openDocument(DocumentPathAdapter.localFilePath(selectedFile));
            if (!result.valid)
                documentOperationError.showError(result.error);
        }
    }

    FileDialog {
        id: saveDialog
        objectName: "saveDocumentDialog"
        title: qsTr("Save document as")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("Caret documents (*.cdoc)")]
        defaultSuffix: "cdoc"
        selectedFile: window.suggestedSaveFileUrl
        acceptLabel: qsTr("Save")
        onAccepted: {
            const result = LayoutViewModel.saveDocument(DocumentPathAdapter.localFilePath(selectedFile));
            if (!result.valid && !result.needsSaveAs)
                documentOperationError.showError(result.error);
        }
        onRejected: window.closeAfterSaveRequested = false
    }

    MessageDialog {
        id: closeConfirmation
        objectName: "closeDocumentConfirmation"
        title: qsTr("Unsaved changes")
        text: qsTr("Do you want to save your changes before closing?")
        buttons: MessageDialog.Save | MessageDialog.Discard | MessageDialog.Cancel
        onButtonClicked: (button, role) => {
            if (button === MessageDialog.Save) {
                window.closeAfterSaveRequested = true;
                window.saveDocument();
            } else if (button === MessageDialog.Discard) {
                window.closeWindow();
            }
        }
    }

    MessageDialog {
        id: documentOperationError
        title: qsTr("Document operation failed")
        buttons: MessageDialog.Ok
        property string operationError: ""
        text: operationError

        function showError(error) {
            operationError = error || qsTr("An unknown error occurred.");
            open();
        }
    }

    Connections {
        target: LayoutViewModel

        function onDocumentFileOperationFinished(succeeded, error) {
            if (!succeeded) {
                window.closeAfterSaveRequested = false;
                documentOperationError.showError(error);
                window.resumeCloseAfterFileOperation();
                return;
            }
            if (window.closeAfterSaveRequested && !LayoutViewModel.documentFileBusy) {
                window.closeAfterSaveRequested = false;
                if (!LayoutViewModel.documentEdited)
                    window.closeWindow();
            }
            window.resumeCloseAfterFileOperation();
        }
    }

    Shortcut {
        sequences: [StandardKey.Open]
        enabled: !LayoutViewModel.documentEdited && !LayoutViewModel.documentFileBusy
        onActivated: openDialog.open()
    }

    Shortcut {
        sequences: [StandardKey.Save]
        onActivated: window.saveDocument()
    }

    Shortcut {
        sequences: [StandardKey.SaveAs]
        onActivated: window.requestSaveAs()
    }

    Shortcut {
        sequences: [StandardKey.Undo]
        enabled: LayoutViewModel.canUndo
        onActivated: LayoutViewModel.undo()
    }

    Shortcut {
        sequences: [StandardKey.Redo]
        enabled: LayoutViewModel.canRedo
        onActivated: LayoutViewModel.redo()
    }

    function saveDocument() {
        const result = LayoutViewModel.saveDocument("");
        if (result.needsSaveAs) {
            requestSaveAs();
            return;
        }
        if (!result.valid) {
            closeAfterSaveRequested = false;
            documentOperationError.showError(result.error);
        }
    }

    function requestSaveAs() {
        suggestedSaveFileUrl = LayoutViewModel.documentPath.length > 0 ? DocumentPathAdapter.localFileUrl(LayoutViewModel.documentPath) : DocumentPathAdapter.defaultSaveFileUrl();
        if (suggestedSaveFileUrl.toString().length === 0) {
            documentOperationError.showError(qsTr("The document folder is unavailable."));
            return;
        }
        saveDialog.open();
    }

    function closeWindow() {
        allowClose = true;
        close();
    }

    function resumeCloseAfterFileOperation() {
        if (!waitForCloseAfterFileOperation || LayoutViewModel.documentFileBusy)
            return;
        waitForCloseAfterFileOperation = false;
        if (LayoutViewModel.documentEdited)
            closeConfirmation.open();
        else
            closeWindow();
    }
}
