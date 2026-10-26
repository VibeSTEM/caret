// 可滚动多页工作区；尺寸由 Rust 页面几何快照提供，缩放只改变视图映射。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas
import Caret.ViewModels

Item {
    id: surface

    signal documentPositionHit(var result)

    property real pageScale: 1
    property bool glyphRasterizationPending: false

    function scheduleGlyphRasterization() {
        if (glyphRasterizationPending)
            return;
        glyphRasterizationPending = true;
        Qt.callLater(surface.rasterizeScheduledGlyphs);
    }

    function rasterizeScheduledGlyphs() {
        glyphRasterizationPending = false;
        rasterizeVisiblePages();
    }

    function rasterizeVisiblePages() {
        if (scroller.width <= 0 || scroller.height <= 0)
            return;
        const pageSizes = pageCanvas.pageSizesPt;
        const visibleTop = scroller.contentY;
        const visibleBottom = visibleTop + scroller.height;
        let low = 0;
        let high = pageSizes.length;
        while (low < high) {
            const middle = low + Math.floor((high - low) / 2);
            const pageTop = pageCanvas.pageOrigin(middle).y;
            const pageBottom = pageTop + pageSizes[middle].height * pageCanvas.zoom * 96 / 72;
            if (pageBottom <= visibleTop)
                low = middle + 1;
            else
                high = middle;
        }

        const frames = [];
        const keys = [];
        const requests = [];
        const color = Theme.colorText;
        for (let pageIndex = low; pageIndex < pageSizes.length; ++pageIndex) {
            const origin = pageCanvas.pageOrigin(pageIndex);
            if (origin.y >= visibleBottom)
                break;
            const pageBottom = origin.y + pageSizes[pageIndex].height * pageCanvas.zoom * 96 / 72;
            if (pageBottom <= visibleTop)
                continue;
            const request = pageCanvas.glyphRasterRequest(pageIndex);
            if (!request.valid) {
                pageCanvas.glyphFrames = [];
                lastGlyphRasterKey = "";
                return;
            }
            const phaseX = request.pageOriginDeviceXPx - Math.floor(request.pageOriginDeviceXPx);
            const phaseY = request.pageOriginDeviceYPx - Math.floor(request.pageOriginDeviceYPx);
            keys.push([pageIndex, LayoutViewModel.documentGeneration, LayoutViewModel.fontGeneration, request.pxPerPt, phaseX, phaseY, Math.round(color.r * 255), Math.round(color.g * 255), Math.round(color.b * 255), Math.round(color.a * 255)].join(":"));
            requests.push({
                pageIndex: pageIndex,
                raster: request
            });
        }
        const key = "visible:" + keys.join("|");
        if (key === lastGlyphRasterKey)
            return;
        for (let index = 0; index < requests.length; ++index) {
            const raster = requests[index].raster;
            frames.push(LayoutViewModel.rasterizePageGlyphs(requests[index].pageIndex, raster.pxPerPt, raster.pageOriginDeviceXPx, raster.pageOriginDeviceYPx, Math.round(color.r * 255), Math.round(color.g * 255), Math.round(color.b * 255), Math.round(color.a * 255)));
        }
        for (let index = 0; index < frames.length; ++index) {
            if (!frames[index].valid) {
                pageCanvas.glyphFrames = [];
                lastGlyphRasterKey = "";
                return;
            }
        }
        pageCanvas.glyphFrames = frames;
        lastGlyphRasterKey = key;
    }

    property string lastGlyphRasterKey: ""

    Connections {
        target: pageCanvas
        function onGeometryChanged() {
            surface.lastGlyphRasterKey = "";
            surface.scheduleGlyphRasterization();
        }
        function onGlyphRasterMetricsChanged() {
            surface.lastGlyphRasterKey = "";
            surface.scheduleGlyphRasterization();
        }
    }

    Connections {
        target: Theme
        function onColorTextChanged() {
            surface.lastGlyphRasterKey = "";
            surface.scheduleGlyphRasterization();
        }
    }

    Connections {
        target: LayoutViewModel
        function onLayoutChanged() {
            surface.lastGlyphRasterKey = "";
            surface.scheduleGlyphRasterization();
        }
    }

    Flickable {
        id: scroller
        objectName: "documentScroller"
        anchors.fill: parent
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentWidth: Math.max(width, pageCanvas.implicitWidth)
        contentHeight: Math.max(height, pageCanvas.implicitHeight)
        onContentYChanged: surface.scheduleGlyphRasterization()
        onContentXChanged: surface.scheduleGlyphRasterization()
        onWidthChanged: surface.scheduleGlyphRasterization()
        onHeightChanged: surface.scheduleGlyphRasterization()

        DocumentCanvas {
            id: pageCanvas
            objectName: "documentCanvas"
            width: scroller.contentWidth
            height: scroller.contentHeight
            zoom: surface.pageScale
            pageColor: Theme.colorPanel
            pageBorderColor: Theme.colorPanelLine
            caretColor: Theme.colorCaret
            caretWidthCentipixels: Theme.caretWidthCentipixels
            pageSizesPt: LayoutViewModel.pageSizesPt
            pageMarginsPt: LayoutViewModel.pageMarginsPt
            caretGeometry: LayoutViewModel.caretGeometry

            onTextInputRequested: function (text) {
                const result = LayoutViewModel.insertText(text);
                if (!result.valid)
                    console.warn("Document text input failed:", result.error);
            }

            onEditingCommandRequested: function (commandId) {
                let result;
                if (commandId === "text.split_paragraph")
                    result = LayoutViewModel.splitParagraph();
                else if (commandId === "text.soft_break")
                    result = LayoutViewModel.insertSoftBreak();
                else if (commandId === "text.delete_backward")
                    result = LayoutViewModel.deleteBackward();
                if (result && !result.valid)
                    console.warn("Document editing command failed:", result.error);
            }

            DocumentCanvasPointerHandler {
                anchors.fill: parent
                canvas: pageCanvas
                onDocumentPositionHit: function (result) {
                    surface.documentPositionHit(result);
                }
            }
        }
    }

    onPageScaleChanged: {
        lastGlyphRasterKey = "";
        scheduleGlyphRasterization();
    }
    Component.onCompleted: scheduleGlyphRasterization()

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
