// 将页面点击与拖选映射到 Rust 文档插入点和共享选区协议。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
    property var dragSnapshot: null
    readonly property var dragFocusCaretGeometry: dragFocus === null ? ({}) : ({
            pageIndex: dragFocus.pageIndex,
            xPt: dragFocus.xPt,
            y0Pt: dragFocus.y0Pt,
            y1Pt: dragFocus.y1Pt
        })
    property int dragAnchorPage: -1
    property bool selectionDragActive: false

    signal documentPositionHit(var result)

    Connections {
        target: LayoutViewModel

        function onLayoutChanged() {
            if (inputHandler.selectionDragActive && !inputHandler.dragSnapshotIsCurrent())
                inputHandler.resetDragPreview();
        }
    }

    TapHandler {
        onTapped: function (eventPoint) {
            inputHandler.canvas.forceActiveFocus(Qt.MouseFocusReason);
            const pageHit = inputHandler.canvas.hitTestPage(eventPoint.position);
            if (pageHit.pageIndex < 0) {
                const result = LayoutViewModel.clearSelection();
                if (!result.valid)
                    console.warn("Document selection clear failed:", result.error);
                else if (result.compositionCancelled === true)
                    inputHandler.canvas.cancelPreedit();
                return;
            }
            const result = LayoutViewModel.placeCaretAtPage(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
            if (result.valid) {
                if (result.compositionCancelled === true)
                    inputHandler.canvas.cancelPreedit();
                inputHandler.documentPositionHit(result);
            }
        }
    }

    DragHandler {
        id: selectionDrag
        objectName: "documentSelectionDrag"
        target: null
        acceptedButtons: Qt.LeftButton
        grabPermissions: PointerHandler.CanTakeOverFromItems | PointerHandler.CanTakeOverFromHandlersOfSameType

        onActiveChanged: {
            if (active) {
                inputHandler.canvas.forceActiveFocus(Qt.MouseFocusReason);
                inputHandler.selectionDragActive = true;
                inputHandler.resetDragPreview();
                inputHandler.dragSnapshot = ({
                        documentGeneration: LayoutViewModel.documentGeneration,
                        fontGeneration: LayoutViewModel.fontGeneration,
                        layoutRevision: String(LayoutViewModel.layoutRevision)
                    });
                inputHandler.updateDragPreview(centroid.pressPosition, true);
                inputHandler.updateDragPreview(centroid.position, false);
            } else {
                if (inputHandler.dragSnapshotIsCurrent() && inputHandler.dragAnchor !== null && inputHandler.dragFocus !== null) {
                    const result = LayoutViewModel.setSelection(inputHandler.dragAnchor, inputHandler.dragFocus);
                    if (!result.valid)
                        console.warn("Document selection failed:", result.error);
                    else if (result.compositionCancelled === true)
                        inputHandler.canvas.cancelPreedit();
                }
                inputHandler.selectionDragActive = false;
                inputHandler.resetDragPreview();
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

    function dragSnapshotIsCurrent() {
        return dragSnapshot !== null && dragSnapshot.documentGeneration === LayoutViewModel.documentGeneration && dragSnapshot.fontGeneration === LayoutViewModel.fontGeneration && dragSnapshot.layoutRevision === String(LayoutViewModel.layoutRevision);
    }

    function resetDragPreview() {
        dragSnapshot = null;
        dragMaps = ({});
        dragAnchor = null;
        dragFocus = null;
        dragAnchorPage = -1;
        canvas.selectionPreviewRects = [];
    }

    function updateDragPreview(point, start) {
        if (!dragSnapshotIsCurrent()) {
            resetDragPreview();
            return;
        }

        const pageHit = canvas.hitTestPage(point);
        if (pageHit.pageIndex < 0)
            return;

        let startDomainId = "";
        if (start) {
            const hit = LayoutViewModel.dragStartDomain(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
            if (!hit.valid) {
                console.warn("Document drag start hit failed:", hit.error);
                return;
            }
            if (hit.domainId === "")
                return;
            startDomainId = hit.domainId;
        } else if (dragAnchor === null) {
            return;
        }

        const key = String(pageHit.pageIndex);
        if (dragMaps[key] === undefined) {
            const anchorPage = dragAnchorPage < 0 ? pageHit.pageIndex : dragAnchorPage;
            if (!cacheDragMaps(Math.min(anchorPage, pageHit.pageIndex), Math.max(anchorPage, pageHit.pageIndex)))
                return;
        }
        const pageMap = dragMaps[key];
        if (pageMap === undefined)
            return;

        const stops = [];
        const pageMaps = Object.values(dragMaps);
        for (let mapIndex = 0; mapIndex < pageMaps.length; ++mapIndex) {
            const pageStops = pageMaps[mapIndex].stops || [];
            for (let stopIndex = 0; stopIndex < pageStops.length; ++stopIndex) {
                const stop = pageStops[stopIndex];
                if ((startDomainId.length > 0 && stop.domainId === startDomainId) || (startDomainId.length === 0 && dragAnchor !== null && stop.domainId === dragAnchor.domainId))
                    stops.push(stop);
            }
        }
        if (stops.length === 0)
            return;
        let best = null;
        let bestPageDistance = Number.MAX_VALUE;
        let bestVerticalDistance = Number.MAX_VALUE;
        let bestHorizontalDistance = Number.MAX_VALUE;
        for (let index = 0; index < stops.length; ++index) {
            const stop = stops[index];
            const pageDistance = Math.abs(stop.pageIndex - pageHit.pageIndex);
            const verticalDistance = distanceToLine(stop, pageHit.yPt);
            const horizontalDistance = Math.abs(stop.xPt - pageHit.xPt);
            if (pageDistance < bestPageDistance || (pageDistance === bestPageDistance && verticalDistance < bestVerticalDistance) || (pageDistance === bestPageDistance && verticalDistance === bestVerticalDistance && horizontalDistance < bestHorizontalDistance)) {
                best = stop;
                bestPageDistance = pageDistance;
                bestVerticalDistance = verticalDistance;
                bestHorizontalDistance = horizontalDistance;
            }
        }
        if (best === null)
            return;
        if (start) {
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
                if (segment.domainId !== anchor.domainId)
                    continue;
                const segmentStart = segment.startPosition;
                const segmentEnd = segment.endPosition;
                if (comparePosition(startPosition, segmentEnd) < 0 && comparePosition(endPosition, segmentStart) > 0)
                    result.push(segment);
            }
        }
        return result;
    }

    function comparePosition(left, right) {
        if (left.kind === "body" && right.kind === "body") {
            if (left.paragraphIndex !== right.paragraphIndex)
                return left.paragraphIndex < right.paragraphIndex ? -1 : 1;
            return compareDecimal(left.orderUtf8, right.orderUtf8);
        }
        if (left.kind === "math" && right.kind === "math")
            return compareDecimal(left.orderUtf8, right.orderUtf8);
        return 0;
    }

    function compareDecimal(left, right) {
        const leftValue = String(left);
        const rightValue = String(right);
        if (leftValue.length !== rightValue.length)
            return leftValue.length < rightValue.length ? -1 : 1;
        return leftValue < rightValue ? -1 : (leftValue > rightValue ? 1 : 0);
    }
}
