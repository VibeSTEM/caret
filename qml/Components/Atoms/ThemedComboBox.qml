// 共用 Import 的 Basic 下拉外观；高度和实心三角由原子层统一。
import QtQuick
import QtQuick.Controls

ComboBox {
    implicitHeight: Theme.comboBoxHeight
    font.pixelSize: Theme.fontBody
    indicator: ThemedIcon {
        anchors.right: parent.right
        anchors.rightMargin: Theme.spacingSmall
        anchors.verticalCenter: parent.verticalCenter
        name: "caret"
        iconSize: Theme.iconSizeSmall
        color: Theme.colorIcon
    }
}
