// 命令与字体候选弹窗共用的列表外壳：列宽测量、行尺寸、滚动列表与接受流程。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import Caret.Adapters
import Caret.ViewModels

CommandPopupFrame {
    id: root

    property int maximumRows: 8
    property real maximumPopupWidth: 420
    property real maximumColumnWidth: 170
    // 字体族候选按弹窗宽度整行显示别名，跳过逐项测量并把标签列宽度归零。
    property bool fillAliasColumn: false
    property real aliasFontPixelSize: Theme.fontSmall
    property real preferredContentWidth: Math.min(root.maximumPopupWidth, root.aliasColumnWidth + (root.labelColumnWidth > 0 ? root.columnSpacing + root.labelColumnWidth : 0) + Theme.spacingSmall * 2 + Theme.spacingTiny * 2 + Theme.borderWidth * 2)

    readonly property real rowHeight: Math.max(Theme.iconSizeSmall, Theme.fontBody + Theme.spacingTiny * 2)
    readonly property real verticalPadding: Theme.spacingTiny * 2
    readonly property real columnSpacing: Theme.spacingMedium
    readonly property real aliasColumnWidth: root.fillAliasColumn ? Math.max(0, width - Theme.spacingTiny * 2 - Theme.borderWidth * 2 - Theme.spacingSmall * 2) : Math.min(root.maximumColumnWidth, root.widestTextWidth(aliasMeasurements))
    readonly property real labelColumnWidth: root.fillAliasColumn ? 0 : Math.min(root.maximumColumnWidth, root.widestTextWidth(labelMeasurements))
    readonly property var measurementModel: root.popupEnabled && !root.fillAliasColumn ? root.commandLine.candidates || [] : []
    readonly property int candidateCount: candidateList.count

    preferredWidth: root.candidateCount > 0 ? root.preferredContentWidth : Math.min(root.maximumPopupWidth, emptyMetrics.advanceWidth + Theme.spacingTiny * 2 + Theme.borderWidth * 2)
    preferredHeight: root.candidateCount > 0 ? Math.min(root.maximumRows, root.candidateCount) * root.rowHeight + root.verticalPadding : emptyMetrics.height + root.verticalPadding

    function widestTextWidth(measurements) {
        let widest = 0;
        for (let index = 0; index < measurements.count; ++index) {
            const textItem = measurements.itemAt(index);
            if (textItem)
                widest = Math.max(widest, textItem.implicitWidth);
        }
        return Math.ceil(widest);
    }

    function aliasFor(candidate) {
        return candidate.alias || "";
    }

    function labelFor(candidate) {
        return candidate.label || "";
    }

    function acceptCandidate(candidate, index) {
        if ((root.commandLine.preedit || "").length > 0)
            return;
        const selectedCandidate = LayoutViewModel.commandLineCommand("select", String(index));
        if (selectedCandidate.valid && candidate.available)
            LayoutViewModel.commandLineCommand("accept", "");
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
            model: root.measurementModel
            delegate: Text {
                required property var modelData
                font.family: PlatformFonts.codeFamily
                font.pixelSize: root.aliasFontPixelSize
                text: root.aliasFor(modelData)
            }
        }

        Repeater {
            id: labelMeasurements
            model: root.measurementModel
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
            aliasFontPixelSize: root.aliasFontPixelSize
            onActivated: root.acceptCandidate(modelData, index)
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
