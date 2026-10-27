// 将页面点击转换为 Rust 文档中的插入点。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas
import Caret.ViewModels

Item {
    id: inputHandler
    objectName: "documentCanvasPointerHandler"

    required property DocumentCanvas canvas

    property var dragMaps: ({})
    property var dragAnchor: null
    property var dragFocus: null
    property int dragAnchorPage: -1
    property bool selectionDragActive: false
    property string dragDocumentGeneration: ""
    property string dragFontGeneration: ""
    property string dragLayoutRevision: ""

    signal documentPositionHit(var result)

    TapHandler {
        onTapped: function (eventPoint) {
            inputHandler.canvas.forceActiveFocus(Qt.MouseFocusReason);
            const pageHit = inputHandler.canvas.hitTestPage(eventPoint.position);
            if (pageHit.pageIndex < 0) {
                const result = LayoutViewModel.clearSelection();
                if (!result.valid)
                    console.warn("Document selection clear failed:", result.error);
                return;
            }
            const result = LayoutViewModel.placeCaretAtPage(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
            if (result.valid)
                inputHandler.documentPositionHit(result);
        }
    }

    DragHandler {
        id: selectionDrag
        objectName: "documentSelectionDrag"
        target: null
        acceptedButtons: Qt.LeftButton
        property point dragStartPosition: Qt.point(0, 0)
        grabPermissions: PointerHandler.CanTakeOverFromItems | PointerHandler.CanTakeOverFromHandlersOfSameType

        onActiveChanged: {
            if (active) {
                inputHandler.selectionDragActive = true;
                inputHandler.dragMaps = ({});
                inputHandler.dragDocumentGeneration = LayoutViewModel.documentGeneration;
                inputHandler.dragFontGeneration = LayoutViewModel.fontGeneration;
                inputHandler.dragLayoutRevision = String(LayoutViewModel.layoutRevision);
                inputHandler.dragAnchor = null;
                inputHandler.dragFocus = null;
                inputHandler.dragAnchorPage = -1;
                dragStartPosition = centroid.pressPosition;
                inputHandler.updateDragPreview(dragStartPosition, true);
                inputHandler.updateDragPreview(centroid.position, false);
            } else {
                const hitMapIsCurrent = inputHandler.dragDocumentGeneration === LayoutViewModel.documentGeneration && inputHandler.dragFontGeneration === LayoutViewModel.fontGeneration && inputHandler.dragLayoutRevision === String(LayoutViewModel.layoutRevision);
                if (hitMapIsCurrent && inputHandler.dragAnchor !== null && inputHandler.dragFocus !== null) {
                    const result = LayoutViewModel.setSelection(inputHandler.dragAnchor, inputHandler.dragFocus);
                    if (!result.valid)
                        console.warn("Document selection failed:", result.error);
                }
                inputHandler.selectionDragActive = false;
                inputHandler.canvas.selectionPreviewRects = [];
                inputHandler.dragMaps = ({});
                inputHandler.dragAnchor = null;
                inputHandler.dragFocus = null;
                inputHandler.dragAnchorPage = -1;
            }
        }

        onTranslationChanged: if (active)
            inputHandler.updateDragPreview(centroid.position, false)
    }

    function cacheDragMaps(firstPage, lastPage) {
        const maps = Object.assign({}, dragMaps);
        for (let pageIndex = firstPage; pageIndex <= lastPage; ++pageIndex) {
            if (maps[String(pageIndex)] !== undefined)
                continue;
            const pageMap = LayoutViewModel.dragHitMap(pageIndex);
            if (!pageMap.valid) {
                console.warn("Document drag hit map failed:", pageMap.error);
                return false;
            }
            maps[String(pageIndex)] = pageMap;
        }
        dragMaps = maps;
        return true;
    }

    function updateDragPreview(point, start) {
        if (dragDocumentGeneration !== LayoutViewModel.documentGeneration || dragFontGeneration !== LayoutViewModel.fontGeneration || dragLayoutRevision !== String(LayoutViewModel.layoutRevision)) {
            dragMaps = ({});
            dragAnchor = null;
            dragFocus = null;
            dragAnchorPage = -1;
            canvas.selectionPreviewRects = [];
            return;
        }

        const pageHit = canvas.hitTestPage(point);
        if (pageHit.pageIndex < 0)
            return;

        const key = String(pageHit.pageIndex);
        if (dragMaps[key] === undefined) {
            const anchorPage = dragAnchorPage < 0 ? pageHit.pageIndex : dragAnchorPage;
            if (!cacheDragMaps(Math.min(anchorPage, pageHit.pageIndex), Math.max(anchorPage, pageHit.pageIndex)))
                return;
        }
        const pageMap = dragMaps[key];
        if (pageMap === undefined)
            return;

        const stops = pageMap.stops;
        if (!stops || stops.length === 0)
            return;
        let best = null;
        let bestVerticalDistance = Number.MAX_VALUE;
        let bestHorizontalDistance = Number.MAX_VALUE;
        for (let index = 0; index < stops.length; ++index) {
            const stop = stops[index];
            const verticalDistance = distanceToLine(stop, pageHit.yPt);
            const horizontalDistance = Math.abs(stop.xPt - pageHit.xPt);
            if (verticalDistance < bestVerticalDistance || (verticalDistance === bestVerticalDistance && horizontalDistance < bestHorizontalDistance)) {
                best = stop;
                bestVerticalDistance = verticalDistance;
                bestHorizontalDistance = horizontalDistance;
            }
        }
        if (best === null)
            return;
        if (start || dragAnchor === null) {
            dragAnchor = best;
            dragAnchorPage = pageHit.pageIndex;
        }
        dragFocus = best;
        canvas.selectionPreviewRects = previewRectangles(dragAnchor, dragFocus);
    }

    function distanceToLine(stop, yPt) {
        if (yPt < stop.y0Pt)
            return stop.y0Pt - yPt;
        if (yPt > stop.y1Pt)
            return yPt - stop.y1Pt;
        return 0;
    }

    function previewRectangles(anchor, focus) {
        const anchorPosition = anchor.position;
        const focusPosition = focus.position;
        let startPosition = anchorPosition;
        let endPosition = focusPosition;
        if (comparePosition(startPosition, endPosition) > 0) {
            startPosition = focusPosition;
            endPosition = anchorPosition;
        }

        const result = [];
        for (const pageMap of Object.values(dragMaps)) {
            for (let index = 0; index < pageMap.segments.length; ++index) {
                const segment = pageMap.segments[index];
                const segmentStart = {
                    paragraphIndex: segment.paragraphIndex,
                    offsetUtf8: segment.startUtf8
                };
                const segmentEnd = {
                    paragraphIndex: segment.paragraphIndex,
                    offsetUtf8: segment.endUtf8
                };
                if (comparePosition(startPosition, segmentEnd) < 0 && comparePosition(endPosition, segmentStart) > 0)
                    result.push(segment);
            }
        }
        return result;
    }

    function comparePosition(left, right) {
        if (left.paragraphIndex !== right.paragraphIndex)
            return left.paragraphIndex < right.paragraphIndex ? -1 : 1;
        const leftOffset = Number(left.offsetUtf8);
        const rightOffset = Number(right.offsetUtf8);
        return leftOffset < rightOffset ? -1 : (leftOffset > rightOffset ? 1 : 0);
    }
}
