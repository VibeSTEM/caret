// 显示普通命令的候选别名、说明和可用状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

SuggestionListPopup {
    id: root

    maximumRows: 4
    maximumPopupWidth: 240
    maximumColumnWidth: 105
    readonly property var mathCandidateLabels: ({
            "\\frac": qsTranslate("CommandCompletion", "Fraction"),
            "\\sqrt": qsTranslate("CommandCompletion", "Square root"),
            "\\text": qsTranslate("CommandCompletion", "Text"),
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
}
