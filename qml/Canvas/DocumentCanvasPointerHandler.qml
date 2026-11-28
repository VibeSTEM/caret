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
    property bool dragSelectionCommitInProgress: false
    property bool dragLayoutRefreshDuringCommit: false
    property bool dragRebasePending: false
    property bool dragPreeditCancelInProgress: false
    property var dragRebaseIdentity: null
    property var dragRebasePressPosition: null
    property var dragRebaseCurrentPosition: null
    property var dragRebaseAnchor: null
    property string dragStartDomainId: ""
    // 正文拖选直接把焦点变化提交给 Rust 会话，由同一选区投影绘制；公式文本岛仍用片段预览。
    property bool previewOnly: false

    signal documentPositionHit(var result)

    Connections {
        target: LayoutViewModel

        function onLayoutChanged() {
            if (inputHandler.selectionDragActive && !inputHandler.dragSnapshotIsCurrent()) {
                if (inputHandler.dragRebasePending && inputHandler.dragPreeditCancelInProgress)
                    return;
                if (inputHandler.dragRebasePending) {
                    if (!inputHandler.dragRebaseIdentityIsCurrent())
                        inputHandler.resetDragPreview();
                    return;
                }
                if (inputHandler.dragSelectionCommitInProgress) {
                    inputHandler.dragLayoutRefreshDuringCommit = true;
                    return;
                }
                inputHandler.resetDragPreview();
            }
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
                inputHandler.dragSnapshot = inputHandler.currentDragIdentity();
                inputHandler.updateDragPreview(centroid.pressPosition, true);
                inputHandler.updateDragPreview(centroid.position, false);
            } else {
                if (inputHandler.dragRebasePending) {
                    if (inputHandler.dragRebaseIdentityIsCurrent())
                        inputHandler.rebaseDragPreview(centroid.pressPosition, centroid.position, true);
                    else
                        inputHandler.resetDragPreview();
                }
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
        return dragSnapshot !== null && dragSnapshot.documentGeneration === LayoutViewModel.documentGeneration && dragSnapshot.fontGeneration === LayoutViewModel.fontGeneration && dragSnapshot.sessionId === LayoutViewModel.sessionId && dragSnapshot.layoutRevision === String(LayoutViewModel.layoutRevision);
    }

    function currentDragIdentity() {
        return ({
                documentGeneration: LayoutViewModel.documentGeneration,
                fontGeneration: LayoutViewModel.fontGeneration,
                sessionId: LayoutViewModel.sessionId,
                layoutRevision: String(LayoutViewModel.layoutRevision)
            });
    }

    function dragRebaseIdentityIsCurrent() {
        return dragRebaseIdentity !== null && dragRebaseIdentity.documentGeneration === LayoutViewModel.documentGeneration && dragRebaseIdentity.fontGeneration === LayoutViewModel.fontGeneration && dragRebaseIdentity.sessionId === LayoutViewModel.sessionId && dragRebaseIdentity.layoutRevision === String(LayoutViewModel.layoutRevision);
    }

    function resetDragPreview() {
        dragRebasePending = false;
        dragPreeditCancelInProgress = false;
        dragRebaseIdentity = null;
        dragRebasePressPosition = null;
        dragRebaseCurrentPosition = null;
        dragRebaseAnchor = null;
        dragStartDomainId = "";
        dragSnapshot = null;
        dragMaps = ({});
        dragAnchor = null;
        dragFocus = null;
        dragAnchorPage = -1;
        previewOnly = true;
        canvas.selectionPreviewRects = [];
    }

    function rebaseDragPreview(pressPosition, currentPosition, allowInactive) {
        if (!selectionDrag.active && allowInactive !== true)
            return;
        if (dragRebasePending && !dragRebaseIdentityIsCurrent()) {
            resetDragPreview();
            return;
        }
        const anchor = dragRebaseAnchor;
        const startDomainId = dragStartDomainId;
        resetDragPreview();
        dragSnapshot = currentDragIdentity();
        dragStartDomainId = startDomainId;
        // 预编辑取消可能改变文字位置，已有逻辑锚点时只重新命中活动端。
        if (anchor !== null) {
            dragAnchor = anchor;
            dragAnchorPage = anchor.pageIndex;
        } else {
            updateDragPreview(pressPosition, true);
        }
        updateDragPreview(currentPosition, false);
    }

    function updateDragPreview(point, start) {
        if (dragRebasePending) {
            dragRebaseCurrentPosition = point;
            return;
        }
        if (!dragSnapshotIsCurrent()) {
            resetDragPreview();
            return;
        }

        const pageHit = canvas.hitTestPage(point);
        if (pageHit.pageIndex < 0)
            return;

        let startDomainId = "";
        if (start) {
            if (dragStartDomainId.length > 0) {
                startDomainId = dragStartDomainId;
            } else {
                const hit = LayoutViewModel.dragStartDomain(pageHit.pageIndex, pageHit.xPt, pageHit.yPt);
                if (!hit.valid) {
                    console.warn("Document drag hit failed:", hit.error);
                    return;
                }
                startDomainId = hit.domainId;
            }
            if (startDomainId === "")
                return;
            dragStartDomainId = startDomainId;
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

        let focusCellId = "";
        let focusCellArea = Number.MAX_VALUE;
        const cellRegions = pageMap.cellHitRegions || [];
        for (let index = 0; index < cellRegions.length; ++index) {
            const region = cellRegions[index];
            if (pageHit.xPt < region.x0Pt || pageHit.xPt > region.x1Pt || pageHit.yPt < region.y0Pt || pageHit.yPt > region.y1Pt)
                continue;
            const area = (region.x1Pt - region.x0Pt) * (region.y1Pt - region.y0Pt);
            if (area < focusCellArea) {
                focusCellId = region.cellId;
                focusCellArea = area;
            }
        }

        const domainStops = [];
        const focusCellStops = [];
        const pageMaps = Object.values(dragMaps);
        const candidateDomainId = startDomainId.length > 0 ? startDomainId : dragAnchor.domainId;
        for (let mapIndex = 0; mapIndex < pageMaps.length; ++mapIndex) {
            const pageStops = pageMaps[mapIndex].stops || [];
            for (let stopIndex = 0; stopIndex < pageStops.length; ++stopIndex) {
                const stop = pageStops[stopIndex];
                if (stop.domainId !== candidateDomainId)
                    continue;
                domainStops.push(stop);
                if (candidateDomainId === "body" && focusCellId.length > 0 && stop.cellId === focusCellId)
                    focusCellStops.push(stop);
            }
        }
        const stops = focusCellStops.length > 0 ? focusCellStops : domainStops;
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
        const focusChanged = start || dragFocus !== best;
        if (start) {
            dragAnchor = best;
            dragAnchorPage = pageHit.pageIndex;
        }
        dragFocus = best;
        if (!focusChanged)
            return;
        if (dragAnchor.domainId === "body" && best.domainId === "body") {
            // 正文拖选：每次焦点变化都提交同一套 Rust 选区投影，公式块与块环境随
            // 松手后的范围一起显示；画布直接消费 LayoutViewModel.selectionRects。
            previewOnly = false;
            canvas.selectionPreviewRects = [];
            commitDragSelection();
        } else {
            previewOnly = true;
            canvas.selectionPreviewRects = previewRectangles(dragAnchor, dragFocus);
        }
    }

    function commitDragSelection() {
        dragSelectionCommitInProgress = true;
        dragLayoutRefreshDuringCommit = false;
        const result = LayoutViewModel.setSelection(dragAnchor, dragFocus);
        dragSelectionCommitInProgress = false;
        const refreshed = dragLayoutRefreshDuringCommit || !dragSnapshotIsCurrent();
        dragLayoutRefreshDuringCommit = false;
        if (!result.valid) {
            console.warn("Document drag selection failed:", result.error);
            if (refreshed)
                resetDragPreview();
            return;
        }
        if (result.compositionCancelled === true) {
            dragRebasePending = true;
            dragRebasePressPosition = selectionDrag.centroid.pressPosition;
            dragRebaseCurrentPosition = selectionDrag.centroid.position;
            dragRebaseAnchor = result.dragAnchor === undefined ? null : result.dragAnchor;
            dragPreeditCancelInProgress = true;
            canvas.cancelPreedit();
            dragPreeditCancelInProgress = false;
            dragRebaseIdentity = currentDragIdentity();
            Qt.callLater(function () {
                if (inputHandler.dragRebasePending && inputHandler.dragRebaseIdentityIsCurrent())
                    inputHandler.rebaseDragPreview(inputHandler.dragRebasePressPosition, inputHandler.dragRebaseCurrentPosition, false);
                else if (inputHandler.dragRebasePending)
                    inputHandler.resetDragPreview();
            });
        } else if (refreshed) {
            resetDragPreview();
        }
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
        if (left.kind !== "math" || right.kind !== "math")
            return 0;
        return left.orderOrdinal < right.orderOrdinal ? -1 : (left.orderOrdinal > right.orderOrdinal ? 1 : 0);
    }
}
