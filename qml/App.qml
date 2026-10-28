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
    property url suggestedRecoveryExportUrl: ""
    property string pendingRecoveryExportId: ""
    property bool allowClose: false
    property bool waitForCloseAfterFileOperation: false
    property bool closeAfterSaveRequested: false
    property bool closeAfterAutoSaveFlush: false
    property bool pendingAutoSaveEnable: false
    property bool pendingRecoveryRestore: false
    property bool pendingRecoveryScan: false

    onClosing: close => {
        if (allowClose)
            return;
        close.accepted = false;
        if (LayoutViewModel.documentFileBusy) {
            waitForCloseAfterFileOperation = true;
            return;
        }
        if (PreferencesViewModel.autoSaveEnabled && LayoutViewModel.documentPath.length > 0 && LayoutViewModel.documentEdited) {
            closeAfterSaveRequested = true;
            closeAfterAutoSaveFlush = true;
            Qt.callLater(window.saveDocument);
            return;
        }
        if (LayoutViewModel.documentEdited)
            closeConfirmation.open();
        else
            closeWindow();
    }

    Component.onCompleted: {
        WindowChromeMetrics.observeWindow(window);
        pendingRecoveryScan = true;
        const recovery = LayoutViewModel.refreshRecoveryEntries();
        if (!recovery.valid) {
            pendingRecoveryScan = false;
            console.warn("Recovery scan skipped:", recovery.error);
            Qt.callLater(window.requestStartupAutoSavePath);
        }
    }

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
                onAutoSaveRequested: enabled => window.requestAutoSave(enabled)
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
        id: recoveryExportDialog
        objectName: "recoveryExportDialog"
        title: qsTr("Save external backup copy")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("All files (*)")]
        selectedFile: window.suggestedRecoveryExportUrl
        acceptLabel: qsTr("Save copy")
        onAccepted: {
            const result = LayoutViewModel.exportRecovery(window.pendingRecoveryExportId, DocumentPathAdapter.localFilePath(selectedFile));
            if (!result.valid)
                documentOperationError.showError(result.error);
        }
        onRejected: window.pendingRecoveryExportId = ""
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
            if (!result.valid && !result.needsSaveAs) {
                window.closeAfterSaveRequested = false;
                documentOperationError.showError(result.error);
            }
        }
        onRejected: {
            window.closeAfterSaveRequested = false;
            if (window.pendingAutoSaveEnable)
                window.cancelAutoSaveEnable();
        }
    }

    UnsavedChangesDialog {
        id: closeConfirmation
        ownerWindow: window
        onSaveRequested: {
            window.closeAfterSaveRequested = true;
            Qt.callLater(window.saveDocument);
        }
        onDiscardRequested: {
            const result = LayoutViewModel.discardCurrentRecovery();
            if (!result.valid) {
                documentOperationError.showError(result.error);
                return;
            }
            Qt.callLater(window.closeWindow);
        }
    }

    SaveConflictDialog {
        id: saveConflictDialog
        ownerWindow: window
        conflictedFileName: LayoutViewModel.documentFileName
        replaceAllowed: LayoutViewModel.saveConflictCanReplace
        onSaveCopyRequested: window.requestSaveCopy()
        onReplaceRequested: {
            const result = LayoutViewModel.replaceExternalVersion();
            if (!result.valid)
                documentOperationError.showError(result.error);
        }
        onCancelRequested: {
            if (window.closeAfterSaveRequested) {
                window.closeAfterSaveRequested = false;
                window.closeAfterAutoSaveFlush = false;
                closeConfirmation.open();
            }
        }
    }

    RecoveryDialog {
        id: recoveryDialog
        entries: LayoutViewModel.recoveryEntries
        busy: LayoutViewModel.documentFileBusy
        onRestoreRequested: id => {
            window.pendingRecoveryRestore = true;
            const result = LayoutViewModel.restoreRecovery(id);
            if (!result.valid) {
                window.pendingRecoveryRestore = false;
                documentOperationError.showError(result.error);
            }
        }
        onDiscardRequested: id => {
            const result = LayoutViewModel.discardRecovery(id);
            if (!result.valid) {
                documentOperationError.showError(result.error);
                return;
            }
            if (LayoutViewModel.recoveryEntries.length === 0) {
                recoveryDialog.close();
                window.requestStartupAutoSavePath();
            }
        }
        onExportRequested: id => window.requestRecoveryExport(id)
        onDismissRequested: window.requestStartupAutoSavePath()
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
            if (window.pendingRecoveryScan) {
                window.pendingRecoveryScan = false;
                if (!succeeded) {
                    console.warn("Recovery scan failed:", error);
                    if (window.waitForCloseAfterFileOperation) {
                        window.resumeCloseAfterFileOperation();
                        return;
                    }
                    window.requestStartupAutoSavePath();
                    return;
                }
                if (window.waitForCloseAfterFileOperation) {
                    window.resumeCloseAfterFileOperation();
                    return;
                }
                if (LayoutViewModel.recoveryEntries.length > 0)
                    recoveryDialog.open();
                else
                    window.requestStartupAutoSavePath();
                return;
            }
            if (window.pendingRecoveryExportId.length > 0) {
                window.pendingRecoveryExportId = "";
                if (!succeeded)
                    documentOperationError.showError(error);
                window.resumeCloseAfterFileOperation();
                return;
            }
            if (window.pendingRecoveryRestore) {
                if (!succeeded) {
                    window.pendingRecoveryRestore = false;
                    documentOperationError.showError(error);
                    window.resumeCloseAfterFileOperation();
                    return;
                }
                window.pendingRecoveryRestore = false;
                recoveryDialog.close();
                if (window.waitForCloseAfterFileOperation) {
                    window.resumeCloseAfterFileOperation();
                    return;
                }
                window.requestStartupAutoSavePath();
                return;
            }
            if (!succeeded) {
                window.waitForCloseAfterFileOperation = false;
                if (window.pendingAutoSaveEnable)
                    window.cancelAutoSaveEnable();
                if (window.closeAfterAutoSaveFlush) {
                    window.closeAfterSaveRequested = false;
                    window.closeAfterAutoSaveFlush = false;
                    closeConfirmation.open();
                    return;
                }
                if (error.startsWith("document.external_conflict")) {
                    const recoveryFailureMarker = "; recovery_binding_failed: ";
                    const recoveryFailureOffset = error.indexOf(recoveryFailureMarker);
                    const recoveryWriteMarker = "document.external_conflict_recovery_failed: ";
                    saveConflictDialog.recoveryWriteFailed = error.startsWith(recoveryWriteMarker);
                    if (recoveryFailureOffset >= 0)
                        saveConflictDialog.recoveryErrorDetail = error.substring(recoveryFailureOffset + recoveryFailureMarker.length);
                    else if (saveConflictDialog.recoveryWriteFailed)
                        saveConflictDialog.recoveryErrorDetail = error.substring(recoveryWriteMarker.length);
                    else
                        saveConflictDialog.recoveryErrorDetail = "";
                    saveConflictDialog.open();
                    return;
                }
                window.closeAfterSaveRequested = false;
                documentOperationError.showError(error);
                return;
            }
            if (window.closeAfterSaveRequested && !LayoutViewModel.documentFileBusy) {
                window.closeAfterSaveRequested = false;
                window.closeAfterAutoSaveFlush = false;
                if (!LayoutViewModel.documentEdited)
                    window.closeWindow();
                else
                    closeConfirmation.open();
            }
            if (window.pendingAutoSaveEnable && !LayoutViewModel.documentFileBusy)
                window.commitAutoSaveEnable();
            else if (PreferencesViewModel.autoSaveEnabled && LayoutViewModel.documentPath.length > 0)
                LayoutViewModel.setAutoSaveEnabled(true);
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

    function requestAutoSave(enabled) {
        if (!enabled) {
            pendingAutoSaveEnable = false;
            if (!PreferencesViewModel.setAutoSaveEnabled(false)) {
                documentOperationError.showError(PreferencesViewModel.error);
            } else {
                LayoutViewModel.setAutoSaveEnabled(false);
            }
            return;
        }
        if (LayoutViewModel.documentPath.length > 0) {
            if (PreferencesViewModel.setAutoSaveEnabled(true))
                LayoutViewModel.setAutoSaveEnabled(true);
            else {
                LayoutViewModel.setAutoSaveEnabled(false);
                documentOperationError.showError(PreferencesViewModel.error);
            }
            return;
        }
        pendingAutoSaveEnable = true;
        requestSaveAs();
    }

    function requestStartupAutoSavePath() {
        if (!PreferencesViewModel.autoSaveEnabled)
            return;
        pendingAutoSaveEnable = true;
        requestSaveAs();
    }

    function commitAutoSaveEnable() {
        pendingAutoSaveEnable = false;
        if (PreferencesViewModel.setAutoSaveEnabled(true))
            LayoutViewModel.setAutoSaveEnabled(true);
        else {
            LayoutViewModel.setAutoSaveEnabled(false);
            documentOperationError.showError(PreferencesViewModel.error);
        }
    }

    function cancelAutoSaveEnable() {
        pendingAutoSaveEnable = false;
        LayoutViewModel.setAutoSaveEnabled(false);
        if (PreferencesViewModel.autoSaveEnabled && !PreferencesViewModel.setAutoSaveEnabled(false))
            documentOperationError.showError(PreferencesViewModel.error);
    }

    function requestSaveAs() {
        suggestedSaveFileUrl = LayoutViewModel.documentPath.length > 0 ? DocumentPathAdapter.localFileUrl(LayoutViewModel.documentPath) : DocumentPathAdapter.defaultSaveFileUrl();
        if (suggestedSaveFileUrl.toString().length === 0) {
            closeAfterSaveRequested = false;
            if (pendingAutoSaveEnable)
                cancelAutoSaveEnable();
            documentOperationError.showError(qsTr("The document folder is unavailable."));
            return;
        }
        saveDialog.open();
    }

    function requestSaveCopy() {
        suggestedSaveFileUrl = DocumentPathAdapter.defaultSaveFileUrl();
        if (suggestedSaveFileUrl.toString().length === 0) {
            documentOperationError.showError(qsTr("The document folder is unavailable."));
            return;
        }
        saveDialog.open();
    }

    function requestRecoveryExport(id) {
        const entry = LayoutViewModel.recoveryEntries.find(item => item.id === id);
        if (!entry)
            return;
        const defaultPath = DocumentPathAdapter.localFilePath(DocumentPathAdapter.defaultSaveFileUrl());
        const separator = Math.max(defaultPath.lastIndexOf("/"), defaultPath.lastIndexOf("\\"));
        if (separator < 0) {
            documentOperationError.showError(qsTr("The document folder is unavailable."));
            return;
        }
        const originalName = String(entry.originalPath).split(/[\\/]/).pop();
        const fileName = originalName.length > 0 ? originalName + ".backup" : "Caret.backup";
        suggestedRecoveryExportUrl = DocumentPathAdapter.localFileUrl(defaultPath.slice(0, separator + 1) + fileName);
        pendingRecoveryExportId = id;
        recoveryExportDialog.open();
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
