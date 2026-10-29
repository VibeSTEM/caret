// 转换 Rust 设置目录并在确认后发布主题与界面语言变更。
#include "settings_view_model.hpp"

#include "application_translator.hpp"
#include "theme_catalog.hpp"
#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QObject>
#include <QString>
#include <QTranslator>
#include <QVariant>
#include <QVariantMap>
#include <cstddef>
#include <memory>
#include <qlogging.h>
#include <utility>

#ifdef CARET_ENABLE_APP
#include "settings_runtime_bridge.hpp"
#include <caret_ffi.h>
#endif

namespace caret {
namespace {

#ifdef CARET_ENABLE_APP
QString rust_string(const rust::String& value) {
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

bool read_object(const rust::String& source, QJsonObject& output) {
    const QByteArray bytes(source.data(), static_cast<qsizetype>(source.size()));
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        return false;
    }
    output = document.object();
    return true;
}
#endif

} // namespace

SettingsViewModel::SettingsViewModel(QObject* parent) : QObject(parent) {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return;
    }
    const ffi::SettingsSnapshot snapshot = ffi::settings_snapshot(*settings_runtime());
    if (!snapshot.error.empty()) {
        setError(rust_string(snapshot.error));
        return;
    }
    m_active_theme = rust_string(snapshot.theme_id);
    m_language = rust_string(snapshot.language);

    QJsonObject theme_catalog;
    if (!read_object(ffi::settings_theme_catalog(*settings_runtime()), theme_catalog)) {
        setError(QStringLiteral("settings.theme_catalog_invalid"));
        return;
    }
    const QJsonObject themes = theme_catalog.value(QStringLiteral("themes")).toObject();
    for (auto it = themes.constBegin(); it != themes.constEnd(); ++it) {
        m_available_themes.push_back(it.key());
    }
    m_available_themes.sort();

    const rust::String language_json = ffi::settings_language_catalog(*settings_runtime());
    const QByteArray language_bytes(language_json.data(),
                                    static_cast<qsizetype>(language_json.size()));
    QJsonParseError language_error;
    const QJsonDocument language_document =
        QJsonDocument::fromJson(language_bytes, &language_error);
    if (language_error.error != QJsonParseError::NoError || !language_document.isArray()) {
        setError(QStringLiteral("settings.language_catalog_invalid"));
        return;
    }
    const QJsonArray languages = language_document.array();
    for (const auto& value : languages) {
        if (!value.isObject()) {
            setError(QStringLiteral("settings.language_catalog_invalid"));
            m_available_languages.clear();
            return;
        }
        const QJsonObject language = value.toObject();
        if (language.value(QStringLiteral("locale")).toString().isEmpty() ||
            language.value(QStringLiteral("displayName")).toString().isEmpty()) {
            setError(QStringLiteral("settings.language_catalog_invalid"));
            m_available_languages.clear();
            return;
        }
        m_available_languages.push_back(language.toVariantMap());
    }
#else
    setError(QStringLiteral("settings.runtime_unavailable"));
#endif
}

bool SettingsViewModel::applySettings(const QString& theme_id, const QString& language,
                                      bool caret_blink_enabled, bool command_suggestions_enabled) {
#ifdef CARET_ENABLE_APP
    if (settings_runtime() == nullptr) {
        setError(QStringLiteral("settings.runtime_unavailable"));
        return false;
    }
    std::unique_ptr<QTranslator> candidate_translator;
    if (language != m_language) {
        candidate_translator = prepare_application_translator(language);
        if (!candidate_translator) {
            setError(QStringLiteral("settings.translation_resource_unavailable"));
            return false;
        }
    }

    const QByteArray theme_id_utf8 = theme_id.toUtf8();
    const QByteArray language_utf8 = language.toUtf8();
    const ffi::SettingsSnapshot snapshot = ffi::set_ui_selection(
        *settings_runtime(),
        rust::String(theme_id_utf8.constData(), static_cast<std::size_t>(theme_id_utf8.size())),
        rust::String(language_utf8.constData(), static_cast<std::size_t>(language_utf8.size())),
        caret_blink_enabled, command_suggestions_enabled);
    if (!snapshot.error.empty()) {
        setError(rust_string(snapshot.error));
        return false;
    }

    const rust::String theme_json = ffi::settings_theme_catalog(*settings_runtime());
    const QByteArray theme_bytes(theme_json.data(), static_cast<qsizetype>(theme_json.size()));
    if (!install_theme_catalog(theme_bytes)) {
        setError(QStringLiteral("settings.theme_projection_failed"));
        return false;
    }
    if (candidate_translator && !install_application_translator(std::move(candidate_translator))) {
        setError(QStringLiteral("settings.translation_install_failed"));
        return false;
    }

    const QString next_theme = rust_string(snapshot.theme_id);
    const QString next_language = rust_string(snapshot.language);
    if (m_active_theme != next_theme || m_language != next_language) {
        m_active_theme = next_theme;
        m_language = next_language;
        emit selectionChanged();
    }
    setError({});
    if (!snapshot.warning.empty()) {
        qWarning().noquote() << QStringLiteral("settings.persistence_warning:")
                             << rust_string(snapshot.warning);
    }
    return true;
#else
    Q_UNUSED(theme_id)
    Q_UNUSED(language)
    Q_UNUSED(caret_blink_enabled)
    Q_UNUSED(command_suggestions_enabled)
    setError(QStringLiteral("settings.runtime_unavailable"));
    return false;
#endif
}

void SettingsViewModel::setError(QString error) {
    if (m_error == error) {
        return;
    }
    m_error = std::move(error);
    emit errorChanged();
}

} // namespace caret
