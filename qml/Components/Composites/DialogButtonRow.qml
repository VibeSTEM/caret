// 右侧操作按钮区；按钮内容与业务响应由具体弹窗提供。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
