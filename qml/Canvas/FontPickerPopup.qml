// 显示字体族和文档默认字体目标的专用候选布局。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

SuggestionListPopup {
    id: root

    readonly property bool fontFamilyMode: commandLine.mode === "fontFamily" || commandLine.mode === "defaultFontFamily"
    // 固定的舒适宽度，避免为数百个系统字体逐项创建测量对象。
    readonly property real fontListPreferredWidth: 360
    readonly property real targetListPreferredWidth: 380

    maximumRows: 8
    maximumPopupWidth: 420
    maximumColumnWidth: 170
    fillAliasColumn: root.fontFamilyMode
    aliasFontPixelSize: root.fontFamilyMode ? Theme.fontBody : Theme.fontSmall
    preferredContentWidth: root.fontFamilyMode ? Math.min(root.maximumPopupWidth, root.fontListPreferredWidth) : root.targetListPreferredWidth

    function labelFor(candidate) {
        if (root.fontFamilyMode)
            return "";
        switch (candidate.commandId) {
        case "command_line.close_font_picker":
            return "";
        case "document.default_font.target":
            return qsTranslate("CommandCompletion", "Script");
        case "document.default_font.script":
            return qsTranslate("CommandCompletion", "Script font");
        case "document.default_font.script_language_scope":
            return qsTranslate("CommandCompletion", "Script + language override");
        case "document.default_font.language":
            return qsTranslate("CommandCompletion", "Language override");
        default:
            return candidate.label || "";
        }
    }

    function aliasFor(candidate) {
        if (candidate.commandId === "command_line.close_font_picker")
            return qsTranslate("CommandCompletion", "Close");
        if (candidate.commandId === "document.default_font.language")
            return qsTranslate("CommandCompletion", "Use language: %1").arg(candidate.value || "");
        return candidate.alias || "";
    }
}
