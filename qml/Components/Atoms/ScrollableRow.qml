// 内容超出可视宽度时可横向滚动的单行布局。
pragma ComponentBehavior: Bound

import QtQuick

Flickable {
    id: root

    default property alias contentData: contentRow.data
    property alias spacing: contentRow.spacing

    readonly property real naturalContentWidth: contentRow.implicitWidth

    contentWidth: Math.max(width, naturalContentWidth)
    contentHeight: height
    flickableDirection: Flickable.HorizontalFlick
    boundsBehavior: Flickable.StopAtBounds
    clip: true
    interactive: contentWidth > width

    Row {
        id: contentRow
        height: root.height
    }
}
