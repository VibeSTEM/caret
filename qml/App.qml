// 文档窗口外壳。页面、字数和语言是占位，设置齿轮打开已有的设置对话框。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Dialogs
import Caret.Shell

Window {
    id: window

    objectName: "caretWindow"
    visible: true
    width: 1120
    height: 760
    minimumWidth: 880
    minimumHeight: 560
    // Linux XCB 会把空标题换成应用名。零宽字符保留窗口按钮，同时不显示文字。
    title: Qt.platform.os === "linux" ? "\u200B" : ""
    color: Theme.colorChrome
    // Windows 隐藏系统标题但保留原生按钮。Linux 显式保留空标题和系统按钮提示。
    flags: Qt.Window | Qt.ExpandedClientAreaHint | Qt.NoTitleBarBackgroundHint | (Qt.platform.os === "windows" ? Qt.CustomizeWindowHint | Qt.WindowMinimizeButtonHint | Qt.WindowMaximizeButtonHint | Qt.WindowCloseButtonHint : Qt.platform.os === "linux" ? Qt.CustomizeWindowHint | Qt.WindowTitleHint | Qt.WindowMinimizeButtonHint | Qt.WindowMaximizeButtonHint | Qt.WindowCloseButtonHint : 0)

    // 只缩放占位页，不表示文档缩放比。
    property real pageScale: 1
    property string documentName: qsTr("Untitled")
    property url documentFileUrl: ""
    property url pendingSaveFileUrl: ""
    property url suggestedSaveFileUrl: ""

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
                onSettingsRequested: settingsDialog.open()
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
                Layout.fillWidth: true
                Layout.fillHeight: true
                pageScale: window.pageScale
            }

            WindowStatusBar {
                Layout.fillWidth: true
                Layout.minimumHeight: Theme.statusbarHeight
                Layout.preferredHeight: Theme.statusbarHeight
                Layout.maximumHeight: Theme.statusbarHeight
                onZoomEdited: value => window.pageScale = value
            }
        }
    }

    SettingsDialog {
        id: settingsDialog
        objectName: "settingsDialog"
        ownerWindow: window
        onAcceptRequested: settingsDialog.completeAccept()
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
