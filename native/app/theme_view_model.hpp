/// 测试宿主或入口注入的主题快照。Theme.qml 只读这些属性，不保存第二份默认值。
#pragma once

#include <QHash>
#include <QObject>
#include <QStringList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// 在创建 QML 引擎之前写入快照。空目录表示尚未注入。
void install_theme_snapshots(QHash<QString, QVariantMap> themes, QString active_theme);

class ThemeViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QVariantMap tokens READ tokens NOTIFY tokensChanged)
    Q_PROPERTY(QStringList availableThemes READ availableThemes CONSTANT)
    Q_PROPERTY(QString activeTheme READ activeTheme NOTIFY activeThemeChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

  public:
    explicit ThemeViewModel(QObject* parent = nullptr);

    [[nodiscard]] const QVariantMap& tokens() const { return m_tokens; }
    [[nodiscard]] const QStringList& availableThemes() const { return m_availableThemes; }
    [[nodiscard]] const QString& activeTheme() const { return m_activeTheme; }
    [[nodiscard]] const QString& error() const { return m_error; }

    /// 切换已注入的主题。未知 ID 不替换当前快照。
    Q_INVOKABLE bool selectTheme(const QString& id);

  signals:
    void tokensChanged();
    void activeThemeChanged();
    void errorChanged();

  private:
    void setError(QString error);

    QVariantMap m_tokens;
    QStringList m_availableThemes;
    QString m_activeTheme;
    QString m_error;
};

} // namespace caret
