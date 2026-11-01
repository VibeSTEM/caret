// 显示普通命令的候选别名、说明和可用状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import Caret.ViewModels

CommandPopupFrame {
    id: root

    readonly property real rowHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)
    readonly property real verticalPadding: Theme.spacingTiny * 2
    readonly property int maximumRows: 4
    readonly property real maximumColumnWidth: 105
    readonly property real columnSpacing: Theme.spacingMedium
    readonly property real maximumPopupWidth: 240
    readonly property real aliasColumnWidth: Math.min(maximumColumnWidth, widestTextWidth(aliasMeasurements))
    readonly property real labelColumnWidth: Math.min(maximumColumnWidth, widestTextWidth(labelMeasurements))
    preferredWidth: candidateList.count > 0 ? Math.min(maximumPopupWidth, aliasColumnWidth + (labelColumnWidth > 0 ? columnSpacing + labelColumnWidth : 0) + Theme.spacingSmall * 2 + Theme.spacingTiny * 2 + Theme.borderWidth * 2) : Math.min(maximumPopupWidth, emptyMetrics.advanceWidth + Theme.spacingTiny * 2 + Theme.borderWidth * 2)
    preferredHeight: candidateList.count > 0 ? Math.min(maximumRows, candidateList.count) * rowHeight + verticalPadding : emptyMetrics.height + verticalPadding

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
        case "text.split_paragraph":
            return qsTranslate("CommandCompletion", "New paragraph");
        case "text.insert.one_character_width_space":
            return qsTranslate("CommandCompletion", "One-character-width space");
        case "text.insert.two_character_width_space":
            return qsTranslate("CommandCompletion", "Two-character-width space");
        case "text.format.bold":
            return qsTranslate("CommandCompletion", "Bold");
        case "text.format.italic":
            return qsTranslate("CommandCompletion", "Italic");
        case "text.format.underline":
            return qsTranslate("CommandCompletion", "Underline");
        case "text.format.font":
            return qsTranslate("CommandCompletion", "Font family");
        case "paragraph.first_line_indent.set":
            return qsTranslate("CommandCompletion", "First-line indent");
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

    TextMetrics {
        id: emptyMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTranslate("CommandCompletion", "No matching commands")
    }

    Item {
        visible: false

        Repeater {
            id: aliasMeasurements
            model: root.popupEnabled ? root.commandLine.candidates || [] : []
            delegate: Text {
                required property var modelData
                font.family: PlatformFonts.codeFamily
                font.pixelSize: Theme.fontSmall
                text: modelData.alias || ""
            }
        }

        Repeater {
            id: labelMeasurements
            model: root.popupEnabled ? root.commandLine.candidates || [] : []
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
        interactive: true
        currentIndex: root.commandLine.selected || 0
        visible: root.commandLine.candidates && root.commandLine.candidates.length > 0
        model: root.popupEnabled ? root.commandLine.candidates || [] : []
        delegate: CommandCandidateItem {
            required property var modelData
            required property int index
            width: candidateList.width
            height: root.rowHeight
            candidate: modelData
            displayAlias: modelData.alias || ""
            label: root.labelFor(modelData)
            selected: index === root.commandLine.selected
            aliasColumnWidth: root.aliasColumnWidth
            labelColumnWidth: root.labelColumnWidth
            onActivated: {
                if ((root.commandLine.preedit || "").length > 0)
                    return;
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
