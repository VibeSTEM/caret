// 显示字体族和文档默认字体目标的专用候选布局。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import Caret.ViewModels

CommandPopupFrame {
    id: root

    readonly property bool fontFamilyMode: commandLine.mode === "fontFamily" || commandLine.mode === "defaultFontFamily"
    readonly property real rowHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)
    readonly property real verticalPadding: Theme.spacingTiny * 2
    readonly property int maximumRows: 8
    readonly property real maximumPopupWidth: 420
    readonly property real columnSpacing: Theme.spacingMedium
    readonly property real aliasColumnWidth: fontFamilyMode ? Math.max(0, width - Theme.spacingTiny * 2 - Theme.borderWidth * 2 - Theme.spacingSmall * 2) : Math.min(170, widestTextWidth(aliasMeasurements))
    readonly property real labelColumnWidth: fontFamilyMode ? 0 : Math.min(170, widestTextWidth(labelMeasurements))
    // 固定的舒适宽度，避免为数百个系统字体逐项创建测量对象。
    readonly property real fontListPreferredWidth: 360
    readonly property real targetListPreferredWidth: 380
    preferredWidth: candidateList.count > 0 ? (fontFamilyMode ? Math.min(maximumPopupWidth, fontListPreferredWidth) : targetListPreferredWidth) : Math.min(maximumPopupWidth, emptyMetrics.advanceWidth + Theme.spacingTiny * 2 + Theme.borderWidth * 2)
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
        if (fontFamilyMode)
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

    TextMetrics {
        id: emptyMetrics
        font.pixelSize: Theme.fontSmall
        text: qsTranslate("CommandCompletion", "No matching commands")
    }

    Item {
        visible: false

        Repeater {
            id: aliasMeasurements
            model: !root.popupEnabled || root.fontFamilyMode ? [] : root.commandLine.candidates || []
            delegate: Text {
                required property var modelData
                font.family: PlatformFonts.codeFamily
                font.pixelSize: root.fontFamilyMode ? Theme.fontBody : Theme.fontSmall
                text: root.aliasFor(modelData)
            }
        }

        Repeater {
            id: labelMeasurements
            model: !root.popupEnabled || root.fontFamilyMode ? [] : root.commandLine.candidates || []
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
            displayAlias: root.aliasFor(modelData)
            label: root.labelFor(modelData)
            selected: index === root.commandLine.selected
            aliasColumnWidth: root.aliasColumnWidth
            labelColumnWidth: root.labelColumnWidth
            aliasFontPixelSize: root.fontFamilyMode ? Theme.fontBody : Theme.fontSmall
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
