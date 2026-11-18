// 文档窗口外壳。页面、字数和语言是占位，设置齿轮打开已有的设置对话框。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import QtQuick.Layouts
import QtQuick.Dialogs
import Caret.Shell
import Caret.ViewModels

Window {
    id: rootWindow

    enum CloseFlowState {
        Ready,
        WaitingForFileOperation,
        ConfirmingClose,
        SavingForClose,
        SaveSucceededWaitingForActivity,
        CloseSaveFailed,
        AutoSaveFlushFailed,
        FlushingAutoSave,
        ChoosingSavePath,
        ResolvingConflict,
        Closing
    }

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

    readonly property string documentBaseName: LayoutViewModel.documentPath.length === 0 ? qsTranslate("WindowTitleBar", "Untitled") : LayoutViewModel.documentFileName
    readonly property string documentStatusLabel: LayoutViewModel.documentSaveStatus === "saving" ? qsTranslate("WindowTitleBar", "Saving") : LayoutViewModel.documentSaveStatus === "edited" ? qsTranslate("WindowTitleBar", "Edited") : qsTranslate("WindowTitleBar", "Saved")
    readonly property string documentName: documentBaseName + " — " + documentStatusLabel
    property url suggestedSaveFileUrl: ""
    property url suggestedRecoveryExportUrl: ""
    property string pendingRecoveryExportId: ""
    property int closeFlowState: App.Ready
    property bool pendingAutoSaveEnable: false
    property bool pendingRecoveryRestore: false
    property bool pendingRecoveryScan: false

    onClosing: close => {
        if (closeFlowState === App.Closing)
            return;
        if (closeFlowState !== App.Ready) {
            close.accepted = false;
            return;
        }
        if (LayoutViewModel.documentFileBusy) {
            close.accepted = false;
            closeFlowState = App.WaitingForFileOperation;
            return;
        }
        if (LayoutViewModel.documentMathSourceDraftPending) {
            close.accepted = false;
            closeFlowState = App.ConfirmingClose;
            closeConfirmation.open();
            return;
        }
        if (PreferencesViewModel.autoSaveEnabled && LayoutViewModel.documentPath.length > 0 && LayoutViewModel.documentEdited) {
            close.accepted = false;
            closeFlowState = App.FlushingAutoSave;
            Qt.callLater(rootWindow.saveDocument);
            return;
        }
        if (LayoutViewModel.documentEdited) {
            close.accepted = false;
            closeFlowState = App.ConfirmingClose;
            closeConfirmation.open();
        } else {
            closeFlowState = App.Closing;
        }
    }

    Component.onCompleted: {
        WindowChromeMetrics.observeWindow(rootWindow);
        pendingRecoveryScan = true;
        const recovery = LayoutViewModel.refreshRecoveryEntries();
        if (!recovery.valid) {
            pendingRecoveryScan = false;
            console.warn("Recovery scan skipped:", recovery.error);
            Qt.callLater(rootWindow.requestStartupAutoSavePath);
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
                window: rootWindow
                saveEnabled: rootWindow.closeFlowState === App.Ready
                autoSaveControlEnabled: rootWindow.closeFlowState === App.Ready
                leftInset: WindowChromeMetrics.leadingSystemControlsInset
                rightInset: WindowChromeMetrics.trailingSystemControlsInset
                onSettingsRequested: settingsController.open()
                onOpenRequested: openDialog.open()
                documentName: rootWindow.documentName
                onSaveRequested: {
                    if (rootWindow.closeFlowState === App.Ready)
                        rootWindow.saveDocument();
                }
                onAutoSaveRequested: enabled => rootWindow.requestAutoSave(enabled)
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
                id: windowStatusBar
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
        settings: SettingsViewModel
        onOpened: settingsDialog.open()
        onClosed: settingsDialog.close()
    }

    SettingsDialog {
        id: settingsDialog
        objectName: "settingsDialog"
        ownerWindow: rootWindow
        controller: settingsController
        pages: [
            {
                routeId: "preferences",
                title: qsTranslate("SettingsDialog", "Preferences"),
                iconName: "settings",
                pageComponent: preferencesSettingsPage
            },
            {
                routeId: "theme",
                title: qsTranslate("SettingsDialog", "Theme"),
                iconName: "settings",
                pageComponent: themeSettingsPage
            },
            {
                routeId: "language",
                title: qsTranslate("SettingsDialog", "Language"),
                iconName: "settings",
                pageComponent: languageSettingsPage
            }
        ]
    }

    Component {
        id: preferencesSettingsPage

        PreferencesSettingsPage {
            caretBlinkEnabled: settingsController.draftCaretBlinkEnabled
            commandSuggestionsEnabled: settingsController.draftCommandSuggestionsEnabled
            errorText: settingsController.error.length > 0 ? LayoutViewModel.documentOperationMessage(settingsController.error) : ""
            enabled: !settingsController.busy
            onCaretBlinkEnabledEdited: enabled => settingsController.editCaretBlinkEnabled(enabled)
            onCommandSuggestionsEnabledEdited: enabled => settingsController.editCommandSuggestionsEnabled(enabled)
        }
    }

    Component {
        id: themeSettingsPage

        ThemeSettingsPage {
            availableThemes: SettingsViewModel.availableThemes
            selectedTheme: settingsController.draftThemeId
            errorText: settingsController.error.length > 0 ? LayoutViewModel.documentOperationMessage(settingsController.error) : ""
            enabled: !settingsController.busy
            onThemeSelected: themeId => settingsController.editThemeId(themeId)
        }
    }

    Component {
        id: languageSettingsPage

        LanguageSettingsPage {
            availableLanguages: SettingsViewModel.availableLanguages
            selectedLanguage: settingsController.draftLanguage
            errorText: settingsController.error.length > 0 ? LayoutViewModel.documentOperationMessage(settingsController.error) : ""
            enabled: !settingsController.busy
            onLanguageSelected: locale => settingsController.editLanguage(locale)
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
        selectedFile: rootWindow.suggestedRecoveryExportUrl
        acceptLabel: qsTr("Save copy")
        onAccepted: {
            const result = LayoutViewModel.exportRecovery(rootWindow.pendingRecoveryExportId, DocumentPathAdapter.localFilePath(selectedFile));
            if (!result.valid)
                documentOperationError.showError(result.error);
        }
        onRejected: rootWindow.pendingRecoveryExportId = ""
    }

    FileDialog {
        id: saveDialog
        objectName: "saveDocumentDialog"
        title: qsTr("Save document as")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("Caret documents (*.cdoc)")]
        defaultSuffix: "cdoc"
        selectedFile: rootWindow.suggestedSaveFileUrl
        acceptLabel: qsTr("Save")
        onAccepted: {
            const result = LayoutViewModel.saveDocument(DocumentPathAdapter.localFilePath(selectedFile));
            if (!result.valid && !result.needsSaveAs) {
                if (rootWindow.closeFlowState === App.ChoosingSavePath || rootWindow.closeFlowState === App.CloseSaveFailed)
                    rootWindow.closeFlowState = App.Ready;
                documentOperationError.showError(result.error);
            }
        }
        onRejected: {
            if (rootWindow.closeFlowState === App.ChoosingSavePath)
                rootWindow.closeFlowState = App.Ready;
            if (rootWindow.pendingAutoSaveEnable)
                rootWindow.cancelAutoSaveEnable();
        }
    }

    UnsavedChangesDialog {
        id: closeConfirmation
        ownerWindow: rootWindow
        mathSourceDraftPending: LayoutViewModel.documentMathSourceDraftPending
        onSaveRequested: {
            rootWindow.closeFlowState = App.SavingForClose;
            Qt.callLater(rootWindow.saveDocument);
        }
        onDiscardRequested: {
            const result = LayoutViewModel.discardCurrentRecovery();
            if (!result.valid) {
                documentOperationError.showError(result.error);
                return;
            }
            rootWindow.closeFlowState = App.Closing;
            Qt.callLater(rootWindow.closeWindow);
        }
    }

    Connections {
        target: closeConfirmation

        function onVisibleChanged() {
            if (!closeConfirmation.visible && rootWindow.closeFlowState === App.ConfirmingClose)
                rootWindow.closeFlowState = App.Ready;
        }
    }

    SaveConflictDialog {
        id: saveConflictDialog
        ownerWindow: rootWindow
        conflictedFileName: LayoutViewModel.documentFileName
        replaceAllowed: LayoutViewModel.saveConflictCanReplace
        onSaveCopyRequested: rootWindow.requestSaveCopy()
        onReplaceRequested: {
            if (rootWindow.closeFlowState === App.ResolvingConflict)
                rootWindow.closeFlowState = App.SavingForClose;
            const result = LayoutViewModel.replaceExternalVersion();
            if (!result.valid) {
                if (rootWindow.closeFlowState === App.CloseSaveFailed || rootWindow.closeFlowState === App.SavingForClose)
                    rootWindow.closeFlowState = App.Ready;
                documentOperationError.showError(result.error);
            }
        }
        onCancelRequested: {
            if (rootWindow.closeFlowState === App.ResolvingConflict) {
                rootWindow.closeFlowState = App.ConfirmingClose;
                closeConfirmation.open();
            }
        }
    }

    RecoveryDialog {
        id: recoveryDialog
        entries: LayoutViewModel.recoveryEntries
        busy: LayoutViewModel.documentFileBusy
        onRestoreRequested: id => {
            rootWindow.pendingRecoveryRestore = true;
            const result = LayoutViewModel.restoreRecovery(id);
            if (!result.valid) {
                rootWindow.pendingRecoveryRestore = false;
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
                rootWindow.requestStartupAutoSavePath();
            }
        }
        onExportRequested: id => rootWindow.requestRecoveryExport(id)
        onDismissRequested: rootWindow.requestStartupAutoSavePath()
    }

    MessageDialog {
        id: documentOperationError
        objectName: "documentOperationError"
        title: qsTr("Document operation failed")
        buttons: MessageDialog.Ok
        property string operationError: ""
        text: operationError

        function showError(error) {
            operationError = LayoutViewModel.documentOperationMessage(error || "");
            open();
        }
    }

    Connections {
        target: LayoutViewModel

        function onDocumentFileActivityFinished() {
            rootWindow.resumeCloseAfterFileOperation();
        }

        function onDocumentSaveFinished(succeeded) {
            if (succeeded && (rootWindow.closeFlowState === App.SavingForClose || rootWindow.closeFlowState === App.FlushingAutoSave || rootWindow.closeFlowState === App.ChoosingSavePath)) {
                rootWindow.closeFlowState = App.SaveSucceededWaitingForActivity;
            } else if (!succeeded && rootWindow.closeFlowState === App.FlushingAutoSave) {
                rootWindow.closeFlowState = App.AutoSaveFlushFailed;
            } else if (!succeeded && (rootWindow.closeFlowState === App.SavingForClose || rootWindow.closeFlowState === App.ChoosingSavePath)) {
                rootWindow.closeFlowState = App.CloseSaveFailed;
            }
        }

        function onDocumentFileOperationFinished(succeeded, error, conflict, recoveryWriteFailed, recoveryError) {
            if (rootWindow.pendingRecoveryScan) {
                rootWindow.pendingRecoveryScan = false;
                if (!succeeded) {
                    console.warn("Recovery scan failed:", error);
                    if (rootWindow.closeFlowState === App.WaitingForFileOperation) {
                        rootWindow.resumeCloseAfterFileOperation();
                        return;
                    }
                    rootWindow.requestStartupAutoSavePath();
                    return;
                }
                if (rootWindow.closeFlowState === App.WaitingForFileOperation) {
                    rootWindow.resumeCloseAfterFileOperation();
                    return;
                }
                if (LayoutViewModel.recoveryEntries.length > 0)
                    recoveryDialog.open();
                else
                    rootWindow.requestStartupAutoSavePath();
                return;
            }
            if (rootWindow.pendingRecoveryExportId.length > 0) {
                rootWindow.pendingRecoveryExportId = "";
                if (!succeeded)
                    documentOperationError.showError(error);
                rootWindow.resumeCloseAfterFileOperation();
                return;
            }
            if (rootWindow.pendingRecoveryRestore) {
                if (!succeeded) {
                    rootWindow.pendingRecoveryRestore = false;
                    documentOperationError.showError(error);
                    rootWindow.resumeCloseAfterFileOperation();
                    return;
                }
                rootWindow.pendingRecoveryRestore = false;
                recoveryDialog.close();
                if (rootWindow.closeFlowState === App.WaitingForFileOperation) {
                    rootWindow.resumeCloseAfterFileOperation();
                    return;
                }
                rootWindow.requestStartupAutoSavePath();
                return;
            }
            if (!succeeded) {
                const wasWaitingForFileOperation = rootWindow.closeFlowState === App.WaitingForFileOperation;
                const closeSaveFailed = rootWindow.closeFlowState === App.CloseSaveFailed;
                const autoSaveFlushFailed = rootWindow.closeFlowState === App.AutoSaveFlushFailed;
                const closeSaveAlreadySucceeded = rootWindow.closeFlowState === App.SaveSucceededWaitingForActivity;
                if (wasWaitingForFileOperation)
                    rootWindow.closeFlowState = App.Ready;
                if (rootWindow.pendingAutoSaveEnable)
                    rootWindow.cancelAutoSaveEnable();
                if (autoSaveFlushFailed) {
                    rootWindow.closeFlowState = App.ConfirmingClose;
                    closeConfirmation.open();
                    return;
                }
                if (conflict && (closeSaveFailed || rootWindow.closeFlowState === App.Ready)) {
                    if (closeSaveFailed)
                        rootWindow.closeFlowState = App.ResolvingConflict;
                    saveConflictDialog.recoveryWriteFailed = recoveryWriteFailed;
                    saveConflictDialog.recoveryErrorDetail = recoveryError.length > 0 ? LayoutViewModel.documentOperationMessage(recoveryError) : "";
                    saveConflictDialog.open();
                    return;
                }
                if (closeSaveFailed || closeSaveAlreadySucceeded)
                    rootWindow.closeFlowState = App.Ready;
                documentOperationError.showError(error.length > 0 ? error : recoveryError);
                return;
            }
            if (rootWindow.pendingAutoSaveEnable && !LayoutViewModel.documentFileBusy)
                rootWindow.commitAutoSaveEnable();
            else if (PreferencesViewModel.autoSaveEnabled && LayoutViewModel.documentPath.length > 0)
                LayoutViewModel.setAutoSaveEnabled(true);
            rootWindow.resumeCloseAfterFileOperation();
        }
    }

    Shortcut {
        sequences: [StandardKey.Open]
        enabled: !LayoutViewModel.documentEdited && !LayoutViewModel.documentFileBusy
        onActivated: openDialog.open()
    }

    Shortcut {
        sequences: [StandardKey.Save]
        enabled: rootWindow.closeFlowState === App.Ready
        onActivated: rootWindow.saveDocument()
    }

    Shortcut {
        sequences: [StandardKey.SaveAs]
        enabled: rootWindow.closeFlowState === App.Ready
        onActivated: rootWindow.requestSaveAs()
    }

    Shortcut {
        sequences: [StandardKey.Undo]
        enabled: pageSurface.canvasHasActiveFocus && LayoutViewModel.canUndo
        onActivated: LayoutViewModel.handleEditorKey("mod-z")
    }

    Shortcut {
        sequences: [StandardKey.Redo]
        enabled: pageSurface.canvasHasActiveFocus && LayoutViewModel.canRedo
        onActivated: LayoutViewModel.handleEditorKey("mod-shift-z")
    }

    Shortcut {
        sequences: [StandardKey.ZoomIn, "Ctrl+="]
        onActivated: windowStatusBar.zoomIn()
    }

    Shortcut {
        sequences: [StandardKey.ZoomOut]
        onActivated: windowStatusBar.zoomOut()
    }

    function saveDocument() {
        const result = LayoutViewModel.saveDocument("");
        if (result.needsSaveAs) {
            requestSaveAs();
            return;
        }
        if (!result.valid) {
            if (closeFlowState === App.CloseSaveFailed)
                closeFlowState = App.Ready;
            else if (closeFlowState === App.AutoSaveFlushFailed) {
                closeFlowState = App.ConfirmingClose;
                closeConfirmation.open();
            }
            documentOperationError.showError(result.error);
        }
    }

    function requestAutoSave(enabled) {
        if (closeFlowState !== App.Ready)
            return;
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
        if (closeFlowState === App.SavingForClose || closeFlowState === App.FlushingAutoSave || closeFlowState === App.ResolvingConflict || closeFlowState === App.CloseSaveFailed)
            closeFlowState = App.ChoosingSavePath;
        suggestedSaveFileUrl = LayoutViewModel.documentPath.length > 0 ? DocumentPathAdapter.localFileUrl(LayoutViewModel.documentPath) : DocumentPathAdapter.defaultSaveFileUrl();
        if (suggestedSaveFileUrl.toString().length === 0) {
            if (closeFlowState === App.ChoosingSavePath)
                closeFlowState = App.Ready;
            if (pendingAutoSaveEnable)
                cancelAutoSaveEnable();
            documentOperationError.showError("document.folder_unavailable");
            return;
        }
        saveDialog.open();
    }

    function requestSaveCopy() {
        if (closeFlowState === App.ResolvingConflict)
            closeFlowState = App.ChoosingSavePath;
        suggestedSaveFileUrl = DocumentPathAdapter.defaultSaveFileUrl();
        if (suggestedSaveFileUrl.toString().length === 0) {
            if (closeFlowState === App.ChoosingSavePath)
                closeFlowState = App.Ready;
            documentOperationError.showError("document.folder_unavailable");
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
            documentOperationError.showError("document.folder_unavailable");
            return;
        }
        const originalName = String(entry.originalPath).split(/[\\/]/).pop();
        const fileName = originalName.length > 0 ? originalName + ".backup" : "Caret.backup";
        suggestedRecoveryExportUrl = DocumentPathAdapter.localFileUrl(defaultPath.slice(0, separator + 1) + fileName);
        pendingRecoveryExportId = id;
        recoveryExportDialog.open();
    }

    function closeWindow() {
        closeFlowState = App.Closing;
        close();
    }

    function resumeCloseAfterFileOperation() {
        if (LayoutViewModel.documentFileBusy)
            return;
        if (closeFlowState === App.CloseSaveFailed || closeFlowState === App.AutoSaveFlushFailed) {
            closeFlowState = App.Ready;
            return;
        }
        if (closeFlowState === App.SaveSucceededWaitingForActivity) {
            closeFlowState = App.Ready;
            if (!LayoutViewModel.documentEdited && !LayoutViewModel.documentMathSourceDraftPending)
                closeWindow();
            else {
                closeFlowState = App.ConfirmingClose;
                closeConfirmation.open();
            }
            return;
        }
        if (closeFlowState !== App.WaitingForFileOperation)
            return;
        closeFlowState = App.Ready;
        if (LayoutViewModel.documentEdited || LayoutViewModel.documentMathSourceDraftPending) {
            closeFlowState = App.ConfirmingClose;
            closeConfirmation.open();
        } else {
            closeWindow();
        }
    }
}
