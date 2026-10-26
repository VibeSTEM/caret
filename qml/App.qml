// 文档窗口外壳。页面、字数和语言是占位，设置齿轮打开已有的设置对话框。
pragma ComponentBehavior: Bound

import QtQuick
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

    property string documentName: qsTr("Untitled")
    property url documentFileUrl: ""
    property url pendingSaveFileUrl: ""
    property url suggestedSaveFileUrl: ""
    property bool pendingCaretBlinkEnabled: PreferencesViewModel.caretBlinkEnabled

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
                onSettingsRequested: {
                    window.pendingCaretBlinkEnabled = PreferencesViewModel.caretBlinkEnabled;
                    settingsDialog.open();
                }
                documentName: window.documentName
                onSaveRequested: {
                    if (window.documentFileUrl.toString().length === 0)
                        window.suggestedSaveFileUrl = DocumentPathAdapter.defaultSaveFileUrl();
                    saveDialog.open();
                }
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

    SettingsDialog {
        id: settingsDialog
        objectName: "settingsDialog"
        ownerWindow: window
        pages: [{
                routeId: "preferences",
                title: qsTranslate("SettingsDialog", "Preferences"),
                iconName: "settings",
                pageComponent: preferencesSettingsPage
            }]
        onAcceptRequested: {
            if (window.pendingCaretBlinkEnabled === PreferencesViewModel.caretBlinkEnabled ||
                    PreferencesViewModel.setCaretBlinkEnabled(window.pendingCaretBlinkEnabled))
                settingsDialog.completeAccept();
        }
        onCancelled: window.pendingCaretBlinkEnabled = PreferencesViewModel.caretBlinkEnabled
    }

    Component {
        id: preferencesSettingsPage

        PreferencesSettingsPage {
            caretBlinkEnabled: window.pendingCaretBlinkEnabled
            onCaretBlinkEnabledEdited: enabled => window.pendingCaretBlinkEnabled = enabled
        }
    }

    FileDialog {
        id: saveDialog
        objectName: "saveDocumentDialog"
        title: qsTr("Save document")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("Caret documents (*.cdoc)")]
        defaultSuffix: "cdoc"
        selectedFile: window.pendingSaveFileUrl.toString().length > 0 ? window.pendingSaveFileUrl : window.suggestedSaveFileUrl
        acceptLabel: qsTr("Save")
        onAccepted: {
            window.pendingSaveFileUrl = selectedFile;
        }
    }
}
