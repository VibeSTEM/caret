// 显示命令输入光标、查询文本与可滚动候选列表。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Canvas

Item {
    id: root

    required property var canvas
    required property var commandLine
    required property int geometryRevision
    readonly property int minimumCommandFontPixelSize: 1
    readonly property int commandFontSizeStepCentipoints: 100

    readonly property var caretPoint: {
        const revision = geometryRevision;
        if (revision < 0 || !commandLine.active || !canvas.caretGeometry || canvas.caretGeometry.pageIndex < 0)
            return Qt.point(0, 0);
        const geometry = canvas.caretGeometry;
        const documentPoint = canvas.documentToContent(geometry.pageIndex, Qt.point(geometry.xPt, geometry.y0Pt));
        return canvas.mapToItem(root, documentPoint.x, documentPoint.y);
    }
    readonly property var caretDisplayRect: {
        const revision = geometryRevision;
        if (revision < 0 || !commandLine.active || !canvas.caretVisualRect)
            return Qt.rect(0, 0, 0, 0);
        const rect = canvas.caretVisualRect;
        const topLeft = canvas.mapToItem(root, rect.x, rect.y);
        return Qt.rect(topLeft.x, topLeft.y, rect.width, rect.height);
    }
    readonly property real queryCaretX: queryScroll.x + queryBox.x + queryCursorMetrics.advanceWidth - queryScroll.contentX
    readonly property real commandFontPixelSize: {
        const fontSizeCentipoints = Number(canvas.caretGeometry.fontSizeCentipoints);
        if (!Number.isFinite(fontSizeCentipoints) || fontSizeCentipoints <= 0)
            return Math.max(root.minimumCommandFontPixelSize, Theme.fontSmall * canvas.zoom);
        const smallerFontSizeCentipoints = Math.max(
            Math.min(fontSizeCentipoints, root.commandFontSizeStepCentipoints),
            fontSizeCentipoints - root.commandFontSizeStepCentipoints
        );
        const zoomPercent = DocumentUnits.factorToPercent(canvas.zoom);
        const centipixels = DocumentUnits.centipointsToLogicalCentipixels(smallerFontSizeCentipoints, zoomPercent);
        return Math.max(root.minimumCommandFontPixelSize, DocumentUnits.centipixelsToLogicalPixels(centipixels));
    }

    visible: commandLine.active === true
    z: 10

    Item {
        id: queryBox
        x: Math.max(0, Math.min(root.caretPoint.x, root.width - width))
        y: root.caretDisplayRect.y + (root.caretDisplayRect.height - height) / 2
        width: Math.max(0, Math.min(queryText.implicitWidth + Theme.spacingSmall * 2, root.width))
        height: Math.max(1, root.caretDisplayRect.height)

        HighlightBackground {
            anchors.fill: parent
            color: Theme.colorCommandPrompt
        }

        Flickable {
            id: queryScroll
            x: Theme.spacingSmall
            y: 0
            width: Math.max(0, parent.width - Theme.spacingSmall * 2)
            height: parent.height
            contentWidth: queryText.implicitWidth
            contentHeight: height
            contentX: Math.max(0, Math.min(
                queryCursorMetrics.advanceWidth - width + root.caretDisplayRect.width,
                contentWidth - width
            ))
            clip: true
            interactive: false

            Text {
                id: queryText
                height: queryScroll.height
                color: Theme.colorText
                font.pixelSize: root.commandFontPixelSize
                verticalAlignment: Text.AlignVCenter
                wrapMode: Text.NoWrap
                text: "\\" + (root.commandLine.query || "")
            }
        }

        TextMetrics {
            id: queryCursorMetrics
            font: queryText.font
            text: queryText.text.slice(0, Number(root.commandLine.cursor || 0) + 1)
        }
    }

    Rectangle {
        x: root.queryCaretX
        y: queryBox.y + (queryBox.height - queryText.implicitHeight) / 2
        width: root.caretDisplayRect.width
        height: queryText.implicitHeight
        color: Theme.colorCaret
        visible: root.commandLine.active === true && root.canvas.caretVisible
        z: 1
    }

    CommandSuggestionPopup {
        commandLine: root.commandLine
        availableWidth: root.width
        availableHeight: root.height
        anchorX: queryBox.x
        anchorY: queryBox.y + queryBox.height + Theme.spacingTiny
        anchorTopY: queryBox.y
    }
}
