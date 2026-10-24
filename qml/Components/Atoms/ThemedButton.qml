// 普通操作按钮；业务命令和按钮排列由调用方负责。
import QtQuick

ThemedToolButton {
    implicitWidth: Theme.dialogActionWidth
    contentColor: Theme.colorText
    borderColor: primaryAction ? Theme.colorDialogPrimaryBorder : Theme.colorPanelLine
    Keys.onReturnPressed: event => {
        clicked();
        event.accepted = true;
    }
}
