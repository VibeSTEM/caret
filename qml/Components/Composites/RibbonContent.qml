// 统一分组与工具渲染，只报告工具 key，不解释页签或业务命令。
pragma ComponentBehavior: Bound

import QtQuick

Row {
    id: content

    required property var groups
    signal actionRequested(string key)

    Repeater {
        model: content.groups
        delegate: RibbonGroup {
            id: group
            required property var modelData
            title: modelData.title
            showCaret: modelData.caret === true

            Repeater {
                model: group.modelData.tools
                delegate: RibbonTile {
                    required property var modelData
                    objectName: "ribbon-" + modelData.key
                    text: modelData.label
                    iconName: modelData.icon
                    showCaret: modelData.caret === true
                    enabled: modelData.enabled !== false
                    onClicked: content.actionRequested(modelData.key)
                }
            }
        }
    }
    Item {
        width: Theme.ribbonTrailingWidth
        height: Theme.ribbonHeight - Theme.borderWidth
    }
}
