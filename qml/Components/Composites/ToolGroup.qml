// 标题工具分组：统一浅灰底色与齐边内容行；默认属性接收调用方的按钮。
import QtQuick

Rectangle {
    default property alias content: row.data

    implicitHeight: Theme.titlebarHeight
    implicitWidth: row.implicitWidth
    color: Theme.colorChrome
    border.width: Theme.borderWidth
    border.color: Theme.colorPanelLine

    Row {
        id: row
        objectName: "toolGroupContent"

        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        spacing: Theme.spacingXSmall
    }
}
