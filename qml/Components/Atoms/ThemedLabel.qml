// 主题文本；调用方可覆盖颜色与字号。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

import QtQuick
import QtQuick.Controls

Label {
    property color textColor: Theme.colorText
    property int textSize: Theme.fontBody

    color: textColor
    font.pixelSize: textSize
}
