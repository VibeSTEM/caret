// 后台菜单行统一图标、可用态、键盘焦点和导航指示器。
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ItemDelegate {
    id: command

    property string iconName: ""
    property bool showChevron: true
    property bool preserveIconColors: false

    Accessible.name: text
    implicitHeight: Theme.applicationMenuCommandHeight
    leftPadding: Theme.spacingSmall
    rightPadding: Theme.spacingSmall
    topPadding: Theme.spacingXSmall
    bottomPadding: Theme.spacingXSmall
    spacing: Theme.spacingSmall
    hoverEnabled: true
    opacity: enabled ? 1 : Theme.disabledOpacity

    contentItem: RowLayout {
        spacing: Theme.spacingSmall

        ThemedIcon {
            Layout.preferredWidth: Theme.iconSizeSmall
            Layout.preferredHeight: Theme.iconSizeSmall
            name: command.iconName
            preserveSourceColors: command.preserveIconColors
            color: Theme.colorIcon
        }

        ThemedLabel {
            Layout.fillWidth: true
            text: command.text
            textColor: command.enabled ? Theme.colorText : Theme.colorTextMuted
            textSize: Theme.fontBody
            wrapMode: Text.WordWrap
            maximumLineCount: 2
        }

        ThemedIcon {
            Layout.preferredWidth: Theme.iconSizeCompact
            Layout.preferredHeight: Theme.iconSizeCompact
            visible: command.showChevron
            name: "right"
            color: command.enabled ? Theme.colorIcon : Theme.colorTextMuted
        }
    }

    background: Rectangle {
        color: command.enabled && (command.hovered || command.visualFocus) ? Theme.colorSelected : Theme.colorTransparent
    }
}
