// Settings 内的语言选择页；locale 列表由应用设置服务提供。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml

Item {
    id: page

    objectName: "languageSettingsPage"
    property var availableLanguages: []
    property string selectedLanguage: ""
    property string errorText: ""

    signal languageSelected(string locale)

    function languageName(locale) {
        const languageLocale = Qt.locale(String(locale).replace("-", "_"));
        return languageLocale.nativeLanguageName || locale;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.spacingMedium

        ThemedLabel {
            Layout.fillWidth: true
            text: qsTranslate("SettingsLanguagePage", "Choose the language used by the application. The change is applied when you select OK.")
            textColor: Theme.colorTextMuted
            textSize: Theme.fontBody
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingMedium

            ThemedLabel {
                text: qsTranslate("SettingsLanguagePage", "Application language")
                textSize: Theme.fontBody
            }

            ThemedComboBox {
                id: languagePicker

                objectName: "settingsLanguagePicker"
                Layout.fillWidth: true
                enabled: page.availableLanguages.length > 0
                model: page.availableLanguages
                currentIndex: page.availableLanguages.indexOf(page.selectedLanguage)
                displayText: currentIndex >= 0 ? page.languageName(currentText) : ""
                Accessible.name: qsTranslate("SettingsLanguagePage", "Application language")

                delegate: ItemDelegate {
                    required property int index
                    required property var modelData
                    width: languagePicker.width
                    text: page.languageName(modelData)
                    highlighted: languagePicker.highlightedIndex === index
                }

                onActivated: page.languageSelected(currentText)
            }
        }

        ThemedLabel {
            Layout.fillWidth: true
            visible: page.availableLanguages.length === 0
            text: qsTranslate("SettingsLanguagePage", "Language settings are unavailable in this build.")
            textColor: Theme.colorTextMuted
            textSize: Theme.fontSmall
            wrapMode: Text.WordWrap
        }

        ThemedLabel {
            objectName: "settingsLanguageError"
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
}
