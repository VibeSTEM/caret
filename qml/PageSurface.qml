// 可滚动多页工作区；尺寸由 Rust 页面几何快照提供，缩放只改变视图映射。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas
import Caret.ViewModels

Item {
    id: surface

    signal documentPositionHit(var result)

    property real pageScale: 1

    Flickable {
        id: scroller
        objectName: "documentScroller"
        anchors.fill: parent
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentWidth: Math.max(width, pageCanvas.implicitWidth)
        contentHeight: Math.max(height, pageCanvas.implicitHeight)

        DocumentCanvas {
            id: pageCanvas
            objectName: "documentCanvas"
            width: scroller.contentWidth
            height: scroller.contentHeight
            zoom: surface.pageScale
            pageColor: Theme.colorPanel
            pageBorderColor: Theme.colorPanelLine
            pageSizesPt: LayoutViewModel.pageSizesPt
            pageMarginsPt: LayoutViewModel.pageMarginsPt

            TapHandler {
                onTapped: function (eventPoint) {
                    const pageHit = pageCanvas.hitTestPage(eventPoint.position);
                    if (pageHit.pageIndex < 0)
                        return;
                    const result = LayoutViewModel.hitTestDocument(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
                    if (result.valid)
                        surface.documentPositionHit(result);
                }
            }
        }
    }

    function setZoomAtViewportCenter(value) {
        const center = Qt.point(scroller.width / 2, scroller.height / 2);
        const contentPoint = scroller.mapToItem(pageCanvas, center.x, center.y);
        const hit = pageCanvas.hitTestPage(contentPoint);
        const documentPoint = hit.pageIndex >= 0 ? Qt.point(hit.xPt, hit.yPt) : Qt.point(0, 0);
        pageScale = value;
        if (hit.pageIndex < 0)
            return;
        Qt.callLater(function () {
            const moved = pageCanvas.documentToContent(hit.pageIndex, documentPoint);
            scroller.contentX = Math.min(Math.max(0, moved.x - center.x), Math.max(0, scroller.contentWidth - scroller.width));
            scroller.contentY = Math.min(Math.max(0, moved.y - center.y), Math.max(0, scroller.contentHeight - scroller.height));
        });
    }
}
