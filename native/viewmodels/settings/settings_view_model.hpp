// Rust Settings 的主题与界面语言投影及确认提交入口。
#pragma once

#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

namespace caret {

class SettingsViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QStringList availableThemes READ availableThemes CONSTANT)
    Q_PROPERTY(QVariantList availableLanguages READ availableLanguages CONSTANT)
    Q_PROPERTY(QString activeTheme READ activeTheme NOTIFY selectionChanged)
    Q_PROPERTY(QString language READ language NOTIFY selectionChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

  public:
    explicit SettingsViewModel(QObject* parent = nullptr);

    [[nodiscard]] const QStringList& availableThemes() const { return m_available_themes; }
    [[nodiscard]] const QVariantList& availableLanguages() const { return m_available_languages; }
    [[nodiscard]] const QString& activeTheme() const { return m_active_theme; }
    [[nodiscard]] const QString& language() const { return m_language; }
    [[nodiscard]] const QString& error() const { return m_error; }

    /// 原子提交设置窗口草稿，成功后更新主题投影和应用翻译器。
    Q_INVOKABLE bool applySettings(const QString& theme_id, const QString& language,
                                   bool caret_blink_enabled, bool command_suggestions_enabled);

  signals:
    void selectionChanged();
    void errorChanged();

  private:
    void setError(QString error);

    QStringList m_available_themes;
    QVariantList m_available_languages;
    QString m_active_theme;
    QString m_language;
    QString m_error;
};

} // namespace caret
