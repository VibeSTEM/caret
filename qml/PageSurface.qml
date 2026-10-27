// 可滚动多页工作区；尺寸由 Rust 页面几何快照提供，缩放只改变视图映射。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas
import Caret.ViewModels

Item {
    id: surface

    signal documentPositionHit(var result)

    property real pageScale: DocumentUnits.percentToFactor(CanvasPolicy.defaultZoomPercent)
    property bool glyphRasterizationPending: false
    property int canvasGeometryRevision: 0

    function compositionPoint(pageIndex, x, y) {
        // C++ 坐标映射依赖 QML 无法直接读取的画布几何状态。
        const revision = canvasGeometryRevision;
        return pageCanvas.documentToContent(pageIndex, Qt.point(x, y));
    }

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
            const pageBottom = pageCanvas.documentToContent(middle, Qt.point(0, pageSizes[middle].height)).y;
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
            const pageBottom = pageCanvas.documentToContent(pageIndex, Qt.point(0, pageSizes[pageIndex].height)).y;
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
            keys.push([pageIndex, LayoutViewModel.documentGeneration, LayoutViewModel.fontGeneration, LayoutViewModel.layoutRevision, request.pxPerPt, phaseX, phaseY, Math.round(color.r * 255), Math.round(color.g * 255), Math.round(color.b * 255), Math.round(color.a * 255)].join(":"));
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
            surface.canvasGeometryRevision++;
            surface.lastGlyphRasterKey = "";
            surface.scheduleGlyphRasterization();
        }
        function onWidthChanged() {
            surface.canvasGeometryRevision++;
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
        onContentYChanged: {
            surface.scheduleGlyphRasterization();
            pageCanvas.updateInputMethodCursorRectangle();
        }
        onContentXChanged: {
            surface.scheduleGlyphRasterization();
            pageCanvas.updateInputMethodCursorRectangle();
        }
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
            caretBlinkEnabled: PreferencesViewModel.caretBlinkEnabled
            caretWidthCentipixels: Theme.caretWidthCentipixels
            pageSizesPt: LayoutViewModel.pageSizesPt
            pageMarginsPt: LayoutViewModel.pageMarginsPt
            caretGeometry: LayoutViewModel.caretGeometry
            selectionRects: pointerHandler.selectionDragActive ? [] : LayoutViewModel.selectionRects
            selectionColor: Theme.colorSelected
            inputMethodContext: LayoutViewModel.inputMethodContext
            onActiveFocusChanged: if (!activeFocus && preeditText.length > 0)
                cancelPreedit()

            onTextInputRequested: function (text) {
                const result = LayoutViewModel.insertText(text);
                if (!result.valid)
                    console.warn("Document text input failed:", result.error);
            }

            onInputMethodEventRequested: function (preedit, commit, replacementStart, replacementLength, cursorPositionUtf16) {
                const result = LayoutViewModel.inputMethodEvent(preedit, commit, replacementStart, replacementLength, cursorPositionUtf16);
                if (!result.valid)
                    console.warn("Document input method event failed:", result.error);
            }

            onVisualNavigationRequested: function (forward, extend) {
                const result = LayoutViewModel.moveVisualCaret(forward, extend);
                if (!result.valid)
                    console.warn("Document caret navigation failed:", result.error);
            }

            onVerticalNavigationRequested: function (lineDelta, extend) {
                const result = LayoutViewModel.moveVerticalCaret(lineDelta, extend);
                if (!result.valid)
                    console.warn("Document caret navigation failed:", result.error);
            }

            onLineEdgeNavigationRequested: function (end, extend) {
                const result = LayoutViewModel.moveToVisualLineEdge(end, extend);
                if (!result.valid)
                    console.warn("Document caret navigation failed:", result.error);
            }

            onClipboardCommandRequested: function (command) {
                const result = command === "copy" ? LayoutViewModel.copySelectionToClipboard() : LayoutViewModel.pasteFromClipboard();
                if (!result.valid)
                    console.warn("Document clipboard command failed:", result.error);
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
                id: pointerHandler
                anchors.fill: parent
                canvas: pageCanvas
                onDocumentPositionHit: function (result) {
                    surface.documentPositionHit(result);
                }
            }
        }

        Repeater {
            model: LayoutViewModel.compositionRects

            delegate: Rectangle {
                required property var modelData
                objectName: "compositionUnderline"

                readonly property point underlineStart: surface.compositionPoint(modelData.pageIndex, modelData.x0Pt, modelData.y0Pt)
                readonly property point underlineEnd: surface.compositionPoint(modelData.pageIndex, modelData.x1Pt, modelData.y1Pt)

                x: pageCanvas.x + underlineStart.x
                y: pageCanvas.y + underlineStart.y
                width: Math.max(0, underlineEnd.x - underlineStart.x)
                height: Math.max(1, underlineEnd.y - underlineStart.y)
                color: Theme.colorText
                visible: width > 0
                z: 1
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
