// 将 Rust SettingsService 的偏好投影桥接到 QML，并提交单项用户意图。
#include "preferences_view_model.hpp"

#include <QString>
#include <qlogging.h>
#include <qobject.h>
#include <qtmetamacros.h>
#include <utility>

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "settings_runtime_bridge.hpp"
#endif

#ifdef CARET_ENABLE_APP
#include <QDebug>
#endif

namespace caret {
#ifdef CARET_ENABLE_APP
namespace {

void log_persistence_warning(const ffi::SettingsSnapshot& snapshot) {
    if (!snapshot.warning.empty()) {
        qWarning().noquote() << QStringLiteral("settings.persistence_warning:")
                             << QString::fromUtf8(snapshot.warning.data(),
                                                  static_cast<qsizetype>(snapshot.warning.size()));
    }
}

} // namespace
#endif

PreferencesViewModel::PreferencesViewModel(QObject* parent) : QObject(parent) {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return;
    }
    const ffi::SettingsSnapshot snapshot = ffi::settings_snapshot(*settings_runtime());
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return;
    }
    log_persistence_warning(snapshot);
    installSnapshot(
        snapshot.caret_blink_enabled, snapshot.command_suggestions_enabled,
        snapshot.auto_save_enabled,
        QSize(static_cast<int>(snapshot.window_width), static_cast<int>(snapshot.window_height)));
    const ffi::SettingsConstraints constraints = ffi::settings_constraints();
    m_minimum_window_width = static_cast<int>(constraints.minimum_window_width);
    m_minimum_window_height = static_cast<int>(constraints.minimum_window_height);
    const rust::String warning = ffi::settings_load_warning(*settings_runtime());
    if (!warning.empty()) {
        setError(QString::fromUtf8(warning.data(), static_cast<qsizetype>(warning.size())));
    }
#endif
}

bool PreferencesViewModel::setCaretBlinkEnabled(bool enabled) {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return false;
    }
    const ffi::SettingsSnapshot snapshot =
        ffi::set_caret_blink_enabled(*settings_runtime(), enabled);
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return false;
    }
    log_persistence_warning(snapshot);
    installSnapshot(
        snapshot.caret_blink_enabled, snapshot.command_suggestions_enabled,
        snapshot.auto_save_enabled,
        QSize(static_cast<int>(snapshot.window_width), static_cast<int>(snapshot.window_height)));
    setError({});
    emit snapshotChanged();
    return true;
#else
    Q_UNUSED(enabled)
    setError(QStringLiteral("settings.runtime_unavailable"));
    return false;
#endif
}

bool PreferencesViewModel::refresh() {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return false;
    }
    const ffi::SettingsSnapshot snapshot = ffi::settings_snapshot(*settings_runtime());
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return false;
    }
    installSnapshot(
        snapshot.caret_blink_enabled, snapshot.command_suggestions_enabled,
        snapshot.auto_save_enabled,
        QSize(static_cast<int>(snapshot.window_width), static_cast<int>(snapshot.window_height)));
    setError({});
    emit snapshotChanged();
    return true;
#else
    setError(QStringLiteral("settings.runtime_unavailable"));
    return false;
#endif
}

bool PreferencesViewModel::setAutoSaveEnabled(bool enabled) {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return false;
    }
    const ffi::SettingsSnapshot snapshot = ffi::set_auto_save_enabled(*settings_runtime(), enabled);
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return false;
    }
    log_persistence_warning(snapshot);
    installSnapshot(
        snapshot.caret_blink_enabled, snapshot.command_suggestions_enabled,
        snapshot.auto_save_enabled,
        QSize(static_cast<int>(snapshot.window_width), static_cast<int>(snapshot.window_height)));
    setError({});
    emit snapshotChanged();
    return true;
#else
    Q_UNUSED(enabled)
    setError(QStringLiteral("settings.runtime_unavailable"));
    return false;
#endif
}

void PreferencesViewModel::installSnapshot(bool caret_blink_enabled,
                                           bool command_suggestions_enabled, bool auto_save_enabled,
                                           QSize window_size) {
    m_caret_blink_enabled = caret_blink_enabled;
    m_command_suggestions_enabled = command_suggestions_enabled;
    m_auto_save_enabled = auto_save_enabled;
    m_window_width = window_size.width();
    m_window_height = window_size.height();
}

void PreferencesViewModel::setError(QString error) {
    if (m_error == error) {
        return;
    }
    m_error = std::move(error);
    emit errorChanged();
}

} // namespace caret
