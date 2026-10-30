// 分享入口：封装标题栏强调色按钮，并通过信号交给窗口处理分享流程。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

import QtQuick

TitleBarPillButton {
    id: button

    signal shareRequested

    objectName: "shareButton"
    accent: true
    iconName: "share"
    text: qsTr("Share")
    showCaret: true
    accessibleName: text
    onClicked: shareRequested()
}
