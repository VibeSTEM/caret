// 底栏高度来自主题；左侧展示页面与当前结构信息，右侧提供缩放控制。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Caret.Canvas
import Caret.ViewModels

Rectangle {
    id: statusBar

    signal zoomEdited(real value)

    readonly property int rowHeight: Theme.statusbarHeight
    readonly property int sliderHeight: Theme.iconSizeCompact
    readonly property var structureLabelTranslations: ({
            "paragraph": qsTranslate("WindowStatusBar", "Paragraph"),
            "heading": qsTranslate("WindowStatusBar", "Heading"),
            "section": qsTranslate("WindowStatusBar", "Section"),
            "custom": qsTranslate("WindowStatusBar", "Custom environment"),
            "equation": qsTranslate("WindowStatusBar", "Equation"),
            "align": qsTranslate("WindowStatusBar", "Align environment")
        })
    readonly property string structureDescription: {
        const entries = LayoutViewModel.currentStructure;
        const labels = [];
        for (let index = 0; index < entries.length; ++index)
            labels.push(structureLabelTranslations[entries[index].kind] || entries[index].kind);
        return labels.join(" › ");
    }
    readonly property string mathFontWarning: {
        const diagnostic = LayoutViewModel.mathFontDiagnostic;
        if (!diagnostic || !diagnostic.code)
            return "";
        if (diagnostic.code === "math.font.family_missing")
            return qsTranslate("WindowStatusBar", 'Math font "%1" is not installed; using STIX Two Math.').arg(diagnostic.requestedFamily);
        if (diagnostic.code === "math.font.unsupported_family")
            return qsTranslate("WindowStatusBar", 'Math font "%1" is unsupported; using STIX Two Math.').arg(diagnostic.requestedFamily);
        return "";
    }
    readonly property string textWarnings: {
        const warnings = [];
        if (mathFontWarning.length > 0)
            warnings.push(mathFontWarning);
        const diagnostics = LayoutViewModel.textDiagnostics;
        const missingTextFamilies = [];
        let hasMissingGlyph = false;
        for (let index = 0; index < diagnostics.length; ++index) {
            const diagnostic = diagnostics[index];
            if (diagnostic.code === "math.glyph.missing" || diagnostic.code === "text.glyph.missing")
                hasMissingGlyph = true;
            else if ((diagnostic.code === "math.text.font.family_missing" || diagnostic.code === "text.font.family_missing") && diagnostic.detail.length > 0 && missingTextFamilies.indexOf(diagnostic.detail) < 0)
                missingTextFamilies.push(diagnostic.detail);
        }
        if (missingTextFamilies.length > 0)
            warnings.push(qsTranslate("WindowStatusBar", 'Text font "%1" is not installed; using fallback fonts.').arg(missingTextFamilies.join(", ")));
        if (hasMissingGlyph)
            warnings.push(qsTranslate("WindowStatusBar", "Text contains characters unavailable in installed fonts."));
        return warnings.join(" · ");
    }

    function zoomOut() {
        zoomSlider.value = Math.max(zoomSlider.from, zoomSlider.value - zoomSlider.stepSize);
    }

    function zoomIn() {
        zoomSlider.value = Math.min(zoomSlider.to, zoomSlider.value + zoomSlider.stepSize);
    }

    implicitHeight: rowHeight
    clip: true
    color: Theme.colorStatus

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: Theme.borderWidth
        color: Theme.colorChromeLine
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingMedium
        anchors.rightMargin: Theme.spacingMedium
        spacing: Theme.spacingLarge

        ThemedLabel {
            Layout.alignment: Qt.AlignVCenter
            text: qsTr("Page %1 of %2").arg(1).arg(1)
            textSize: Theme.fontSmall
            textColor: Theme.colorText
        }

        ThemedLabel {
            Layout.alignment: Qt.AlignVCenter
            text: qsTr("%1 words").arg(0)
            textSize: Theme.fontSmall
            textColor: Theme.colorText
        }

        ThemedLabel {
            Layout.alignment: Qt.AlignVCenter
            text: qsTr("English (United States)")
            textSize: Theme.fontSmall
            textColor: Theme.colorText
        }

        ThemedLabel {
            objectName: "currentStructureLabel"
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 180
            Layout.maximumWidth: 240
            elide: Text.ElideRight
            text: statusBar.structureDescription
            textSize: Theme.fontSmall
            textColor: Theme.colorText
        }

        ThemedLabel {
            objectName: "textFontWarningLabel"
            visible: statusBar.textWarnings.length > 0
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 220
            Layout.maximumWidth: 260
            elide: Text.ElideRight
            text: statusBar.textWarnings
            textSize: Theme.fontSmall
            textColor: Theme.colorError
            Accessible.description: statusBar.textWarnings
            ToolTip.visible: warningHover.hovered
            ToolTip.text: statusBar.textWarnings

            HoverHandler {
                id: warningHover
            }
        }

        Item {
            Layout.fillWidth: true
        }

        ThemedToolButton {
            objectName: "zoomOutButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: Theme.toolbarButtonHeight
            Layout.preferredHeight: statusBar.rowHeight - Theme.spacingSmall
            Layout.maximumHeight: statusBar.rowHeight - Theme.spacingXSmall
            controlHeight: statusBar.rowHeight - Theme.spacingSmall
            contentPadding: 0
            iconName: "zoom-out"
            iconSize: Theme.iconSizeCompact
            accessibleName: qsTr("Zoom out")
            contentColor: Theme.colorText
            onClicked: statusBar.zoomOut()
        }

        Slider {
            id: zoomSlider
            objectName: "zoomSlider"
            Accessible.name: qsTr("Zoom")
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 120
            Layout.preferredHeight: statusBar.sliderHeight
            Layout.maximumHeight: statusBar.sliderHeight
            padding: 0
            from: DocumentUnits.percentToFactor(CanvasPolicy.minimumZoomPercent)
            to: DocumentUnits.percentToFactor(CanvasPolicy.maximumZoomPercent)
            stepSize: DocumentUnits.percentToFactor(CanvasPolicy.zoomStepPercent)
            value: DocumentUnits.percentToFactor(CanvasPolicy.defaultZoomPercent)
            onValueChanged: statusBar.zoomEdited(value)

            background: Rectangle {
                x: zoomSlider.leftPadding
                y: zoomSlider.topPadding + (zoomSlider.availableHeight - height) / 2
                implicitWidth: 120
                implicitHeight: 2
                width: zoomSlider.availableWidth
                height: 2
                radius: 1
                color: Theme.colorPanelLine

                Rectangle {
                    width: zoomSlider.visualPosition * parent.width
                    height: parent.height
                    radius: 1
                    color: Theme.colorFocus
                }
            }

            handle: Rectangle {
                x: zoomSlider.leftPadding + zoomSlider.visualPosition * (zoomSlider.availableWidth - width)
                y: zoomSlider.topPadding + (zoomSlider.availableHeight - height) / 2
                implicitWidth: statusBar.sliderHeight
                implicitHeight: statusBar.sliderHeight
                width: statusBar.sliderHeight
                height: statusBar.sliderHeight
                radius: width / 2
                color: Theme.colorPanel
                border.width: Theme.borderWidth + 1
                border.color: Theme.colorFocus
            }
        }

        ThemedToolButton {
            objectName: "zoomInButton"
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: Theme.toolbarButtonHeight
            Layout.preferredHeight: statusBar.rowHeight - Theme.spacingSmall
            Layout.maximumHeight: statusBar.rowHeight - Theme.spacingXSmall
            controlHeight: statusBar.rowHeight - Theme.spacingSmall
            contentPadding: 0
            iconName: "zoom-in"
            iconSize: Theme.iconSizeCompact
            accessibleName: qsTr("Zoom in")
            contentColor: Theme.colorText
            onClicked: statusBar.zoomIn()
        }

        ThemedLabel {
            objectName: "zoomLabel"
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 42
            horizontalAlignment: Text.AlignRight
            text: DocumentUnits.factorToPercent(zoomSlider.value) + "%"
            textSize: Theme.fontSmall
            textColor: Theme.colorText
        }
    }
}
