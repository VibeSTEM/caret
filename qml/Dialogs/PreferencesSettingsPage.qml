// Settings 窗口中的用户偏好；当前只暴露 Rust 管理的光标闪烁开关。
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

Item {
    id: page

    objectName: "preferencesSettingsPage"
    property bool caretBlinkEnabled: true
    property bool commandSuggestionsEnabled: true
    property string errorText: ""

    signal caretBlinkEnabledEdited(bool enabled)
    signal commandSuggestionsEnabledEdited(bool enabled)

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.spacingMedium

        ThemedLabel {
            Layout.fillWidth: true
            text: qsTranslate("PreferencesSettingsPage", "Control editing behavior and other user preferences.")
            textColor: Theme.colorTextMuted
            textSize: Theme.fontBody
            wrapMode: Text.WordWrap
        }

        ThemedSwitch {
            objectName: "caretBlinkPreference"
            text: qsTranslate("PreferencesSettingsPage", "Blink the insertion point")
            checked: page.caretBlinkEnabled
            onToggled: page.caretBlinkEnabledEdited(checked)
        }

        ThemedSwitch {
            objectName: "commandSuggestionsPreference"
            text: qsTranslate("PreferencesSettingsPage", "Show command suggestions")
            checked: page.commandSuggestionsEnabled
            onToggled: page.commandSuggestionsEnabledEdited(checked)
        }

        ThemedLabel {
            objectName: "preferencesSettingsError"
            Layout.fillWidth: true
            visible: page.errorText.length > 0
            text: page.errorText
            textColor: Theme.colorError
            textSize: Theme.fontSmall
            wrapMode: Text.WordWrap
        }

        Item {
            Layout.fillHeight: true
        }
    }
}
