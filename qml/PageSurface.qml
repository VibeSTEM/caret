// 页面占位。缩放只改变这一页的显示大小，不读文档。页面在编辑区里居中。
pragma ComponentBehavior: Bound

import QtQuick

Item {
    id: surface

    property real pageScale: 1

    readonly property real pageMargin: Theme.spacingLarge * 2
    readonly property real fittedWidth: Math.min(680, Math.max(240, width - pageMargin * 2))
    readonly property real fittedHeight: fittedWidth * 11 / 8.5
    readonly property real fit: Math.min(1, Math.max(0.35, (height - pageMargin * 2) / Math.max(1, fittedHeight)))
    readonly property real pageWidth: fittedWidth * fit * pageScale
    readonly property real pageHeight: fittedHeight * fit * pageScale

    Flickable {
        id: scroller
        anchors.fill: parent
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentWidth: Math.max(width, surface.pageWidth + surface.pageMargin * 2)
        contentHeight: Math.max(height, surface.pageHeight + surface.pageMargin * 2)

        Rectangle {
            id: pageFrame
            objectName: "documentPage"
            width: surface.pageWidth
            height: surface.pageHeight
            x: Math.max(surface.pageMargin, (scroller.contentWidth - width) / 2)
            y: Math.max(surface.pageMargin, (scroller.contentHeight - height) / 2)
            color: Theme.colorPanel
            border.width: Theme.borderWidth
            border.color: Theme.colorPanelLine
        }
    }
}
