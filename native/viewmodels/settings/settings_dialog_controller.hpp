// 设置窗口的交互状态机；草稿属于视图，提交结果由 Rust 设置服务决定。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include "preferences_view_model.hpp"
#include <QObject>
#include <QPointer>
#include <QStateMachine>
#include <QString>
#include <QtQml/qqmlregistration.h>
#include <cstdint>

namespace caret {

class SettingsDialogController : public QObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QObject* preferences READ preferencesObject WRITE setPreferencesObject NOTIFY
                   preferencesChanged)
    Q_PROPERTY(QObject* settings READ settingsObject WRITE setSettingsObject NOTIFY settingsChanged)
    Q_PROPERTY(State state READ state NOTIFY stateChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY stateChanged)
    Q_PROPERTY(bool draftCaretBlinkEnabled READ draftCaretBlinkEnabled NOTIFY draftChanged)
    Q_PROPERTY(
        bool draftCommandSuggestionsEnabled READ draftCommandSuggestionsEnabled NOTIFY draftChanged)
    Q_PROPERTY(QString draftThemeId READ draftThemeId NOTIFY draftChanged)
    Q_PROPERTY(QString draftLanguage READ draftLanguage NOTIFY draftChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

  public:
    enum State : std::uint8_t { Closed, Editing, Applying, Failed };
    Q_ENUM(State)

    explicit SettingsDialogController(QObject* parent = nullptr);
    [[nodiscard]] QObject* preferencesObject() const { return m_preferences; }
    void setPreferencesObject(QObject* preferences);
    void setPreferences(PreferencesViewModel* preferences);
    [[nodiscard]] QObject* settingsObject() const;
    void setSettingsObject(QObject* settings);
    [[nodiscard]] State state() const { return m_state; }
    [[nodiscard]] bool busy() const { return m_state == Applying; }
    [[nodiscard]] bool draftCaretBlinkEnabled() const { return m_draft_caret_blink_enabled; }
    [[nodiscard]] bool draftCommandSuggestionsEnabled() const {
        return m_draft_command_suggestions_enabled;
    }
    [[nodiscard]] const QString& draftThemeId() const { return m_draft_theme_id; }
    [[nodiscard]] const QString& draftLanguage() const { return m_draft_language; }
    [[nodiscard]] const QString& error() const { return m_error; }

    Q_INVOKABLE void open();
    Q_INVOKABLE void editCaretBlinkEnabled(bool enabled);
    Q_INVOKABLE void editCommandSuggestionsEnabled(bool enabled);
    Q_INVOKABLE void editThemeId(const QString& theme_id);
    Q_INVOKABLE void editLanguage(const QString& language);
    Q_INVOKABLE void apply();
    Q_INVOKABLE void cancel();

  signals:
    void preferencesChanged();
    void settingsChanged();
    void stateChanged();
    void draftChanged();
    void errorChanged();
    void opened();
    void closed();

  private:
    Q_SIGNAL void openRequested();
    Q_SIGNAL void editRequested();
    Q_SIGNAL void applyRequested();
    Q_SIGNAL void cancelRequested();
    Q_SIGNAL void applySucceeded();
    Q_SIGNAL void applyFailed();
    void enterState(State state);
    void submitDraft();
    void setError(QString error);

    QStateMachine m_machine;
    QPointer<PreferencesViewModel> m_preferences;
    QPointer<QObject> m_settings;
    State m_state = Closed;
    bool m_draft_caret_blink_enabled = true;
    bool m_draft_command_suggestions_enabled = true;
    QString m_draft_theme_id;
    QString m_draft_language;
    bool m_open_when_ready = false;
    QString m_error;
};

} // namespace caret
