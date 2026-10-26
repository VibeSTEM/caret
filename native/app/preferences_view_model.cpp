// 将 Rust SettingsService 的偏好投影桥接到 QML，并提交单项用户意图。
#include "preferences_view_model.hpp"

#include <QString>
#include <utility>

#ifdef CARET_ENABLE_APP
#include <memory>
#endif

namespace caret {
#ifdef CARET_ENABLE_APP
namespace {

std::unique_ptr<rust::Box<ffi::SettingsRuntime>>& installed_runtime() {
    static std::unique_ptr<rust::Box<ffi::SettingsRuntime>> runtime;
    return runtime;
}

QString from_rust(const rust::String& value) {
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

} // namespace

bool install_preferences_runtime(rust::Box<ffi::SettingsRuntime> runtime) {
    installed_runtime() = std::make_unique<rust::Box<ffi::SettingsRuntime>>(std::move(runtime));
    return true;
}
#endif

PreferencesViewModel::PreferencesViewModel(QObject* parent) : QObject(parent) {
#ifdef CARET_ENABLE_APP
    if (!installed_runtime()) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return;
    }
    const ffi::SettingsSnapshot snapshot = ffi::settings_snapshot(**installed_runtime());
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return;
    }
    installSnapshot(snapshot.caret_blink_enabled, static_cast<int>(snapshot.window_width),
                    static_cast<int>(snapshot.window_height));
    const ffi::SettingsConstraints constraints = ffi::settings_constraints();
    m_minimum_window_width = static_cast<int>(constraints.minimum_window_width);
    m_minimum_window_height = static_cast<int>(constraints.minimum_window_height);
    const rust::String warning = ffi::settings_load_warning(**installed_runtime());
    if (!warning.empty()) {
        setError(from_rust(warning));
    }
#endif
}

bool PreferencesViewModel::setCaretBlinkEnabled(bool enabled) {
#ifdef CARET_ENABLE_APP
    if (!installed_runtime()) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return false;
    }
    const ffi::SettingsSnapshot snapshot =
        ffi::set_caret_blink_enabled(**installed_runtime(), enabled);
    if (!snapshot.error.empty()) {
        setError(QString::fromUtf8(snapshot.error.data(),
                                   static_cast<qsizetype>(snapshot.error.size())));
        return false;
    }
    installSnapshot(snapshot.caret_blink_enabled, static_cast<int>(snapshot.window_width),
                    static_cast<int>(snapshot.window_height));
    setError({});
    emit snapshotChanged();
    return true;
#else
    Q_UNUSED(enabled)
    setError(QStringLiteral("settings.runtime_unavailable"));
    return false;
#endif
}

void PreferencesViewModel::installSnapshot(bool caret_blink_enabled, int window_width,
                                           int window_height) {
    m_caret_blink_enabled = caret_blink_enabled;
    m_window_width = window_width;
    m_window_height = window_height;
}

void PreferencesViewModel::setError(QString error) {
    if (m_error == error) {
        return;
    }
    m_error = std::move(error);
    emit errorChanged();
}

} // namespace caret
