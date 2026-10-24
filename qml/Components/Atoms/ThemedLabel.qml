// 主题文本；调用方可覆盖颜色与字号。
import QtQuick
import QtQuick.Controls

Label {
    property color textColor: Theme.colorText
    property int textSize: Theme.fontBody

    color: textColor
    font.pixelSize: textSize
}
