// 显示命令候选；宽度按候选内容伸缩，受可视区域限制。
pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import Caret.ViewModels

Rectangle {
    id: root

    required property var commandLine
    required property real availableWidth
    required property real availableHeight
    property real anchorX: 0
    property real anchorY: 0
    property real anchorTopY: 0
    readonly property real rowHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)
    readonly property real verticalPadding: Theme.spacingTiny * 2
    readonly property int maximumRows: 4
    // 列宽随当前候选收紧，单列上限固定；选中项超出时由文本 Atom 循环展示。
    readonly property real maximumCandidateColumnWidth: 105
    readonly property real candidateColumnSpacing: Theme.spacingMedium
    readonly property real maximumEstimatedWidth: 240
    readonly property real aliasColumnWidth: Math.min(maximumCandidateColumnWidth, widestTextWidth(aliasMeasurements))
    readonly property real labelColumnWidth: Math.min(maximumCandidateColumnWidth, widestTextWidth(labelMeasurements))
    readonly property real preferredWidth: candidateList.count > 0 ? Math.min(maximumEstimatedWidth, aliasColumnWidth + candidateColumnSpacing + labelColumnWidth + Theme.spacingSmall * 2 + Theme.spacingTiny * 2 + Theme.borderWidth * 2) : Math.min(maximumEstimatedWidth, emptyMetrics.advanceWidth + Theme.spacingTiny * 2 + Theme.borderWidth * 2)
    readonly property real preferredHeight: candidateList.count > 0 ? Math.min(maximumRows, candidateList.count) * rowHeight + verticalPadding : emptyMetrics.height + verticalPadding

    function widestTextWidth(measurements) {
        let widest = 0;
        for (let index = 0; index < measurements.count; ++index) {
            const textItem = measurements.itemAt(index);
            if (textItem)
                widest = Math.max(widest, textItem.implicitWidth);
        }
        return Math.ceil(widest);
    }

    function labelFor(candidate) {
        switch (candidate.alias) {
        case "\\chapter":
            return qsTranslate("CommandCompletion", "Chapter");
        case "\\section":
            return qsTranslate("CommandCompletion", "Section");
        case "\\subsection":
            return qsTranslate("CommandCompletion", "Subsection");
        case "\\subsubsection":
            return qsTranslate("CommandCompletion", "Subsubsection");
        }
        switch (candidate.commandId) {
        case "para.bullets":
            return qsTranslate("CommandCompletion", "Bulleted list");
        case "para.numbering":
            return qsTranslate("CommandCompletion", "Numbered list");
        case "page.break.insert":
            return qsTranslate("CommandCompletion", "Page break");
        case "text.soft_break":
            return qsTranslate("CommandCompletion", "Line break");
        case "insert.inline_math":
            return qsTranslate("CommandCompletion", "Inline math");
        case "insert.equation":
            return qsTranslate("CommandCompletion", "Single-line equation");
        case "insert.align":
            return qsTranslate("CommandCompletion", "Multiline equation");
        case "insert.equation.fraction":
            return qsTranslate("CommandCompletion", "Fraction");
        case "insert.equation.radical":
            return qsTranslate("CommandCompletion", "Square root");
        case "insert.table":
            return qsTranslate("CommandCompletion", "Table");
        default:
            return candidate.commandId;
        }
    }

    x: Math.max(Theme.spacingSmall, Math.min(anchorX, Math.max(Theme.spacingSmall, availableWidth - width - Theme.spacingSmall)))
    y: anchorY + height <= availableHeight - Theme.spacingSmall ? anchorY : Math.max(Theme.spacingSmall, anchorTopY - height - Theme.spacingTiny)
    width: Math.max(0, Math.min(preferredWidth, availableWidth - Theme.spacingMedium * 2))
    height: Math.max(0, Math.min(preferredHeight, availableHeight - Theme.spacingSmall * 2))
    radius: Theme.radiusSmall
    border.width: Theme.borderWidth
    border.color: Theme.colorPanelLine
    color: Theme.colorPanel
    visible: commandLine.active === true && PreferencesViewModel.commandSuggestionsEnabled

    TextMetrics {
        id: emptyMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTranslate("CommandCompletion", "No matching commands")
    }

    Item {
        visible: false

        Repeater {
            id: aliasMeasurements
            model: root.commandLine.candidates || []
            delegate: Text {
                required property var modelData
                font.family: PlatformFonts.fixedFamily
                font.pixelSize: Theme.fontSmall
                text: modelData.alias
            }
        }

        Repeater {
            id: labelMeasurements
            model: root.commandLine.candidates || []
            delegate: Text {
                required property var modelData
                font.pixelSize: Theme.fontBody
                text: root.labelFor(modelData)
            }
        }
    }

    ScrollableListView {
        id: candidateList
        anchors.fill: parent
        anchors.margins: Theme.spacingTiny
        interactive: true
        currentIndex: root.commandLine.selected || 0
        visible: root.commandLine.candidates && root.commandLine.candidates.length > 0
        model: root.commandLine.candidates || []
        delegate: CommandCandidateItem {
            required property var modelData
            required property int index
            width: candidateList.width
            height: root.rowHeight
            candidate: modelData
            label: root.labelFor(modelData)
            selected: index === root.commandLine.selected
            aliasColumnWidth: root.aliasColumnWidth
            labelColumnWidth: root.labelColumnWidth
            onActivated: {
                const selectedCandidate = LayoutViewModel.commandLineCommand("select", String(index));
                if (selectedCandidate.valid && modelData.available)
                    LayoutViewModel.commandLineCommand("accept", "");
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: candidateList.count === 0
        color: Theme.colorTextMuted
        font.pixelSize: Theme.fontSmall
        text: qsTranslate("CommandCompletion", "No matching commands")
    }
}
