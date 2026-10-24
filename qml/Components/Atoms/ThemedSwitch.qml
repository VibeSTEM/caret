// 紧凑开关。默认关闭，轨道为灰色；打开后轨道使用强调色。
import QtQuick
import QtQuick.Controls

Switch {
    id: control

    checked: false
    padding: 0
    spacing: Theme.spacingXSmall
    hoverEnabled: true
    font.pixelSize: Theme.fontSmall

    readonly property int trackHeight: Theme.iconSizeSmall
    readonly property int knobSize: Theme.iconSizeCompact
    readonly property int trackWidth: trackHeight + knobSize

    implicitHeight: trackHeight
    opacity: enabled ? 1 : Theme.disabledOpacity

    Accessible.name: text

    indicator: Item {
        implicitWidth: control.trackWidth
        implicitHeight: control.trackHeight
        x: control.leftPadding
        y: (control.height - height) / 2

        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: control.checked ? (control.hovered ? Qt.darker(Theme.colorFocus, 1.08) : Theme.colorFocus) : (control.hovered ? Theme.colorChromePressed : Theme.colorPanelLine)
            border.width: control.visualFocus ? Theme.focusBorderWidth : 0
            border.color: Theme.colorFocus

            Rectangle {
                width: control.knobSize
                height: control.knobSize
                radius: width / 2
                anchors.verticalCenter: parent.verticalCenter
                x: control.checked ? parent.width - width - Theme.spacingTiny : Theme.spacingTiny
                color: Theme.colorPanel
            }
        }
    }

    contentItem: ThemedLabel {
        text: control.text
        textSize: control.font.pixelSize
        textColor: control.checked ? Theme.colorText : Theme.colorTextMuted
        verticalAlignment: Text.AlignVCenter
        leftPadding: control.indicator.width + control.spacing
    }
}
