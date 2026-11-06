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
    readonly property var mathCandidateLabels: ({
            "\\frac": qsTranslate("CommandCompletion", "Fraction"),
            "\\sqrt": qsTranslate("CommandCompletion", "Square root"),
            "\\superscript": qsTranslate("CommandCompletion", "Superscript"),
            "\\subscript": qsTranslate("CommandCompletion", "Subscript"),
            "\\sum": qsTranslate("CommandCompletion", "Summation"),
            "\\int": qsTranslate("CommandCompletion", "Integral"),
            "\\infty": qsTranslate("CommandCompletion", "Infinity"),
            "\\alpha": qsTranslate("CommandCompletion", "Greek alpha"),
            "\\beta": qsTranslate("CommandCompletion", "Greek beta"),
            "\\gamma": qsTranslate("CommandCompletion", "Greek gamma"),
            "\\delta": qsTranslate("CommandCompletion", "Greek delta"),
            "\\epsilon": qsTranslate("CommandCompletion", "Greek epsilon"),
            "\\zeta": qsTranslate("CommandCompletion", "Greek zeta"),
            "\\eta": qsTranslate("CommandCompletion", "Greek eta"),
            "\\theta": qsTranslate("CommandCompletion", "Greek theta"),
            "\\iota": qsTranslate("CommandCompletion", "Greek iota"),
            "\\kappa": qsTranslate("CommandCompletion", "Greek kappa"),
            "\\lambda": qsTranslate("CommandCompletion", "Greek lambda"),
            "\\mu": qsTranslate("CommandCompletion", "Greek mu"),
            "\\nu": qsTranslate("CommandCompletion", "Greek nu"),
            "\\xi": qsTranslate("CommandCompletion", "Greek xi"),
            "\\pi": qsTranslate("CommandCompletion", "Greek pi"),
            "\\rho": qsTranslate("CommandCompletion", "Greek rho"),
            "\\sigma": qsTranslate("CommandCompletion", "Greek sigma"),
            "\\tau": qsTranslate("CommandCompletion", "Greek tau"),
            "\\upsilon": qsTranslate("CommandCompletion", "Greek upsilon"),
            "\\phi": qsTranslate("CommandCompletion", "Greek phi"),
            "\\chi": qsTranslate("CommandCompletion", "Greek chi"),
            "\\psi": qsTranslate("CommandCompletion", "Greek psi"),
            "\\omega": qsTranslate("CommandCompletion", "Greek omega"),
            "\\Gamma": qsTranslate("CommandCompletion", "Greek capital Gamma"),
            "\\Delta": qsTranslate("CommandCompletion", "Greek capital Delta"),
            "\\Theta": qsTranslate("CommandCompletion", "Greek capital Theta"),
            "\\Lambda": qsTranslate("CommandCompletion", "Greek capital Lambda"),
            "\\Xi": qsTranslate("CommandCompletion", "Greek capital Xi"),
            "\\Pi": qsTranslate("CommandCompletion", "Greek capital Pi"),
            "\\Sigma": qsTranslate("CommandCompletion", "Greek capital Sigma"),
            "\\Upsilon": qsTranslate("CommandCompletion", "Greek capital Upsilon"),
            "\\Phi": qsTranslate("CommandCompletion", "Greek capital Phi"),
            "\\Psi": qsTranslate("CommandCompletion", "Greek capital Psi"),
            "\\Omega": qsTranslate("CommandCompletion", "Greek capital Omega")
        })
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
        case "math.split_inline":
            return qsTranslate("CommandCompletion", "Inline math");
        case "insert.equation":
            return qsTranslate("CommandCompletion", "Single-line equation");
        case "insert.align":
            return qsTranslate("CommandCompletion", "Multiline equation");
        case "math.insert":
            return root.mathCandidateLabels[candidate.alias] || candidate.alias || candidate.commandId;
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
