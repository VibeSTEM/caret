// 菜单页签预留强调字重的空间，选中时不改变相邻项位置。
import QtQuick

ThemedToolButton {
    id: button
    controlHeight: Theme.menubarHeight
    cornerRadius: 0
    contentPadding: Theme.spacingLarge
    contentColor: highlighted ? Theme.colorText : Theme.colorMenubarText
    hoverColor: Theme.colorMenubarHover
    font.weight: highlighted ? Font.DemiBold : Font.Normal
    implicitWidth: Math.ceil(Math.max(normalText.advanceWidth, emphasizedText.advanceWidth)) + 2 * contentPadding
    TextMetrics {
        id: normalText
        text: button.text
        font.family: button.font.family
        font.pixelSize: button.font.pixelSize
        font.weight: Font.Normal
        font.letterSpacing: button.font.letterSpacing
        font.wordSpacing: button.font.wordSpacing
    }
    TextMetrics {
        id: emphasizedText
        text: button.text
        font.family: button.font.family
        font.pixelSize: button.font.pixelSize
        font.weight: Font.DemiBold
        font.letterSpacing: button.font.letterSpacing
        font.wordSpacing: button.font.wordSpacing
    }
}
