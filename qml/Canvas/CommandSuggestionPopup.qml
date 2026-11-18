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
    // 此函数仅供 lupdate 扫描，运行时不会执行。
    function translationSourcesForLupdateOnly() {
        return [qsTranslate("CommandCompletion", "Chapter"), qsTranslate("CommandCompletion", "Section"), qsTranslate("CommandCompletion", "Subsection"), qsTranslate("CommandCompletion", "Subsubsection"), qsTranslate("CommandCompletion", "Bulleted list"), qsTranslate("CommandCompletion", "Numbered list"), qsTranslate("CommandCompletion", "Page break"), qsTranslate("CommandCompletion", "Inline math"), qsTranslate("CommandCompletion", "Single-line equation"), qsTranslate("CommandCompletion", "Multiline equation"), qsTranslate("CommandCompletion", "Table"), qsTranslate("CommandCompletion", "Line break"), qsTranslate("CommandCompletion", "New paragraph"), qsTranslate("CommandCompletion", "One-character-width space"), qsTranslate("CommandCompletion", "Two-character-width space"), qsTranslate("CommandCompletion", "First-line indent"), qsTranslate("CommandCompletion", "Bold"), qsTranslate("CommandCompletion", "Italic"), qsTranslate("CommandCompletion", "Underline"), qsTranslate("CommandCompletion", "Font family"), qsTranslate("CommandCompletion", "Default font"), qsTranslate("CommandCompletion", "Fraction"), qsTranslate("CommandCompletion", "Square root"), qsTranslate("CommandCompletion", "Text"), qsTranslate("CommandCompletion", "Superscript"), qsTranslate("CommandCompletion", "Subscript"), qsTranslate("CommandCompletion", "Summation"), qsTranslate("CommandCompletion", "Integral"), qsTranslate("CommandCompletion", "Infinity"), qsTranslate("CommandCompletion", "Greek alpha"), qsTranslate("CommandCompletion", "Greek beta"), qsTranslate("CommandCompletion", "Greek gamma"), qsTranslate("CommandCompletion", "Greek delta"), qsTranslate("CommandCompletion", "Greek epsilon"), qsTranslate("CommandCompletion", "Greek zeta"), qsTranslate("CommandCompletion", "Greek eta"), qsTranslate("CommandCompletion", "Greek theta"), qsTranslate("CommandCompletion", "Greek iota"), qsTranslate("CommandCompletion", "Greek kappa"), qsTranslate("CommandCompletion", "Greek lambda"), qsTranslate("CommandCompletion", "Greek mu"), qsTranslate("CommandCompletion", "Greek nu"), qsTranslate("CommandCompletion", "Greek xi"), qsTranslate("CommandCompletion", "Greek pi"), qsTranslate("CommandCompletion", "Greek rho"), qsTranslate("CommandCompletion", "Greek sigma"), qsTranslate("CommandCompletion", "Greek tau"), qsTranslate("CommandCompletion", "Greek upsilon"), qsTranslate("CommandCompletion", "Greek phi"), qsTranslate("CommandCompletion", "Greek chi"), qsTranslate("CommandCompletion", "Greek psi"), qsTranslate("CommandCompletion", "Greek omega"), qsTranslate("CommandCompletion", "Greek capital Gamma"), qsTranslate("CommandCompletion", "Greek capital Delta"), qsTranslate("CommandCompletion", "Greek capital Theta"), qsTranslate("CommandCompletion", "Greek capital Lambda"), qsTranslate("CommandCompletion", "Greek capital Xi"), qsTranslate("CommandCompletion", "Greek capital Pi"), qsTranslate("CommandCompletion", "Greek capital Sigma"), qsTranslate("CommandCompletion", "Greek capital Upsilon"), qsTranslate("CommandCompletion", "Greek capital Phi"), qsTranslate("CommandCompletion", "Greek capital Psi"), qsTranslate("CommandCompletion", "Greek capital Omega")];
    }

    function labelFor(candidate) {
        return qsTranslate("CommandCompletion", candidate.label || candidate.commandId || candidate.alias || "");
    }
}
