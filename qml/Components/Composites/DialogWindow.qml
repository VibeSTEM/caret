// 对话框窗口约束与定位；打开条件和关闭守卫由具体业务窗口处理。
import QtQuick

Window {
    id: baseDialog

    property Window ownerWindow
    property bool escapeEnabled: true
    color: Theme.colorTransparent
    modality: Qt.ApplicationModal
    flags: Qt.Dialog | Qt.FramelessWindowHint
    transientParent: ownerWindow

    onVisibleChanged: {
        if (!visible && ownerWindow)
            ownerWindow.requestActivate();
    }

    Shortcut {
        sequence: "Escape"
        enabled: baseDialog.visible && baseDialog.escapeEnabled
        onActivated: baseDialog.close()
    }

    function present() {
        centerOnOwner();
        show();
        requestActivate();
    }

    function centerOnOwner() {
        if (ownerWindow) {
            x = ownerWindow.x + Math.round((ownerWindow.width - width) / 2);
            y = ownerWindow.y + Math.round((ownerWindow.height - height) / 2);
        }
    }
}
