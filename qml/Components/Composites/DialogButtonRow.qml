// 右侧操作按钮区；按钮内容与业务响应由具体弹窗提供。
import QtQuick
import QtQuick.Layouts

RowLayout {
    id: row
    default property alias actions: buttons.data
    spacing: Theme.dialogActionSpacing
    Item {
        Layout.fillWidth: true
    }
    RowLayout {
        id: buttons
        spacing: row.spacing
    }
}
