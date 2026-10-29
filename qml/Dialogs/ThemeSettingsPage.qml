// Settings 内的主题选择页；主题目录和当前值由 Rust Settings 投影提供。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: page

    objectName: "themeSettingsPage"
    property var availableThemes: []
    property string selectedTheme: ""
    property string errorText: ""

    signal themeSelected(string themeId)

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.spacingMedium

        ThemedLabel {
            Layout.fillWidth: true
            text: qsTranslate("ThemeSettingsPage", "Choose the application theme.")
            textColor: Theme.colorTextMuted
            textSize: Theme.fontBody
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingMedium

            ThemedLabel {
                text: qsTranslate("ThemeSettingsPage", "Application theme")
                textSize: Theme.fontBody
            }

            ThemedComboBox {
                id: themePicker
                objectName: "settingsThemePicker"
                Layout.fillWidth: true
                enabled: page.availableThemes.length > 0
                model: page.availableThemes
                currentIndex: page.availableThemes.indexOf(page.selectedTheme)
                displayText: currentIndex >= 0 ? page.themeName(currentText) : ""
                Accessible.name: qsTranslate("ThemeSettingsPage", "Application theme")

                delegate: ItemDelegate {
                    required property int index
                    required property string modelData
                    width: themePicker.width
                    text: page.themeName(modelData)
                    highlighted: themePicker.highlightedIndex === index
                }

                onActivated: index => page.themeSelected(page.availableThemes[index])
            }
        }

        ThemedLabel {
            Layout.fillWidth: true
            visible: page.availableThemes.length === 0
            text: qsTranslate("ThemeSettingsPage", "Theme settings are unavailable in this build.")
            textColor: Theme.colorTextMuted
            textSize: Theme.fontSmall
            wrapMode: Text.WordWrap
        }

        ThemedLabel {
            objectName: "themeSettingsError"
            Layout.fillWidth: true
            visible: page.errorText.length > 0
            text: page.errorText
            textColor: Theme.colorError
            textSize: Theme.fontBody
            wrapMode: Text.WordWrap
        }

        Item {
            Layout.fillHeight: true
        }
    }

    function themeName(themeId) {
        if (themeId === "dark")
            return qsTranslate("ThemeSettingsPage", "Dark");
        if (themeId === "light")
            return qsTranslate("ThemeSettingsPage", "Light");
        return themeId;
    }
}
