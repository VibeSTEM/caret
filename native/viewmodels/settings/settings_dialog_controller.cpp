// 用事件驱动设置窗口生命周期，并只在确认时提交偏好草稿。
#include "settings_dialog_controller.hpp"

#include "preferences_view_model.hpp"
#include "settings_view_model.hpp"
#include <QState>
#include <QString>
#include <qobject.h>
#include <qtmetamacros.h>
#include <utility>

namespace caret {

SettingsDialogController::SettingsDialogController(QObject* parent) : QObject(parent) {
    auto* closed_state = new QState(&m_machine);
    auto* editing_state = new QState(&m_machine);
    auto* applying_state = new QState(&m_machine);
    auto* failed_state = new QState(&m_machine);
    m_machine.setInitialState(closed_state);
    closed_state->addTransition(this, &SettingsDialogController::openRequested, editing_state);
    for (QState* editable : {editing_state, failed_state}) {
        editable->addTransition(this, &SettingsDialogController::applyRequested, applying_state);
        editable->addTransition(this, &SettingsDialogController::cancelRequested, closed_state);
    }
    failed_state->addTransition(this, &SettingsDialogController::editRequested, editing_state);
    applying_state->addTransition(this, &SettingsDialogController::applySucceeded, closed_state);
    applying_state->addTransition(this, &SettingsDialogController::applyFailed, failed_state);
    connect(closed_state, &QState::entered, this, [this]() { enterState(Closed); });
    connect(editing_state, &QState::entered, this, [this]() { enterState(Editing); });
    connect(failed_state, &QState::entered, this, [this]() { enterState(Failed); });
    connect(applying_state, &QState::entered, this, [this]() {
        enterState(Applying);
        submitDraft();
    });
    connect(&m_machine, &QStateMachine::started, this, [this]() {
        if (m_open_when_ready) {
            m_open_when_ready = false;
            open();
        }
    });
    m_machine.start();
}

void SettingsDialogController::setPreferences(PreferencesViewModel* preferences) {
    if (m_preferences == preferences || m_state != Closed) {
        return;
    }
    m_preferences = preferences;
    emit preferencesChanged();
}

void SettingsDialogController::setPreferencesObject(QObject* preferences) {
    setPreferences(qobject_cast<PreferencesViewModel*>(preferences));
}

QObject* SettingsDialogController::settingsObject() const { return m_settings; }

void SettingsDialogController::setSettingsObject(QObject* settings) {
    if (m_settings == settings || m_state != Closed) {
        return;
    }
    m_settings = settings;
    emit settingsChanged();
}

void SettingsDialogController::open() {
    if (!m_machine.isRunning()) {
        m_open_when_ready = true;
        return;
    }
    if (m_state != Closed) {
        return;
    }
    if (m_preferences != nullptr) {
        m_draft_caret_blink_enabled = m_preferences->caretBlinkEnabled();
        m_draft_command_suggestions_enabled = m_preferences->commandSuggestionsEnabled();
        setError(m_preferences->error());
    } else {
        setError(QStringLiteral("settings.preferences_unavailable"));
    }
    if (auto* settings = qobject_cast<SettingsViewModel*>(m_settings.data())) {
        m_draft_theme_id = settings->activeTheme();
        m_draft_language = settings->language();
        if (m_error.isEmpty()) {
            setError(settings->error());
        }
    }
    emit draftChanged();
    emit openRequested();
}

void SettingsDialogController::editCaretBlinkEnabled(bool enabled) {
    if ((m_state != Editing && m_state != Failed) || m_draft_caret_blink_enabled == enabled) {
        return;
    }
    m_draft_caret_blink_enabled = enabled;
    setError({});
    emit draftChanged();
    emit editRequested();
}

void SettingsDialogController::editCommandSuggestionsEnabled(bool enabled) {
    if ((m_state != Editing && m_state != Failed) ||
        m_draft_command_suggestions_enabled == enabled) {
        return;
    }
    m_draft_command_suggestions_enabled = enabled;
    setError({});
    emit draftChanged();
    emit editRequested();
}

void SettingsDialogController::editThemeId(const QString& theme_id) {
    if ((m_state != Editing && m_state != Failed) || m_draft_theme_id == theme_id) {
        return;
    }
    m_draft_theme_id = theme_id;
    setError({});
    emit draftChanged();
    emit editRequested();
}

void SettingsDialogController::editLanguage(const QString& language) {
    if ((m_state != Editing && m_state != Failed) || m_draft_language == language) {
        return;
    }
    m_draft_language = language;
    setError({});
    emit draftChanged();
    emit editRequested();
}

void SettingsDialogController::apply() {
    if (m_state == Editing || m_state == Failed) {
        emit applyRequested();
    }
}

void SettingsDialogController::cancel() {
    m_open_when_ready = false;
    if (m_state == Editing || m_state == Failed) {
        emit cancelRequested();
    }
}

void SettingsDialogController::enterState(State state) {
    if (m_state == state) {
        return;
    }
    const State previous = m_state;
    m_state = state;
    emit stateChanged();
    if (state == Closed) {
        emit closed();
    } else if (previous == Closed) {
        emit opened();
    }
}

void SettingsDialogController::submitDraft() {
    if (auto* settings = qobject_cast<SettingsViewModel*>(m_settings.data())) {
        if (!settings->applySettings(m_draft_theme_id, m_draft_language,
                                     m_draft_caret_blink_enabled,
                                     m_draft_command_suggestions_enabled)) {
            setError(settings->error());
            emit applyFailed();
            return;
        }
        if (m_preferences != nullptr && !m_preferences->refresh()) {
            setError(m_preferences->error());
            emit applyFailed();
            return;
        }
        setError({});
        emit applySucceeded();
        return;
    }
    if (m_preferences == nullptr) {
        setError(QStringLiteral("settings.preferences_unavailable"));
        emit applyFailed();
        return;
    }
    if (m_draft_caret_blink_enabled == m_preferences->caretBlinkEnabled() ||
        m_preferences->setCaretBlinkEnabled(m_draft_caret_blink_enabled)) {
        setError({});
        emit applySucceeded();
        return;
    }
    setError(m_preferences->error());
    emit applyFailed();
}

void SettingsDialogController::setError(QString error) {
    if (m_error == error) {
        return;
    }
    m_error = std::move(error);
    emit errorChanged();
}

} // namespace caret
