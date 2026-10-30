// 提供命令与字体候选弹窗共用的外框、定位和内容留白。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.ViewModels

Rectangle {
    id: root

    required property var commandLine
    required property real availableWidth
    required property real availableHeight
    required property real preferredWidth
    required property real preferredHeight
    property real anchorX: 0
    property real anchorY: 0
    property real anchorTopY: 0
    property bool popupEnabled: true
    default property alias contentData: contentHost.data

    x: Math.max(Theme.spacingSmall, Math.min(anchorX, Math.max(Theme.spacingSmall, availableWidth - width - Theme.spacingSmall)))
    y: anchorY + height <= availableHeight - Theme.spacingSmall ? anchorY : Math.max(Theme.spacingSmall, anchorTopY - height - Theme.spacingTiny)
    width: Math.max(0, Math.min(preferredWidth, availableWidth - Theme.spacingMedium * 2))
    height: Math.max(0, Math.min(preferredHeight, availableHeight - Theme.spacingSmall * 2))
    radius: Theme.radiusSmall
    border.width: Theme.borderWidth
    border.color: Theme.colorPanelLine
    color: Theme.colorPanel
    visible: popupEnabled && commandLine.active === true && PreferencesViewModel.commandSuggestionsEnabled

    Item {
        id: contentHost
        anchors.fill: parent
        anchors.margins: Theme.spacingTiny
    }
}
