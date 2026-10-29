// Rust Preferences 快照到 QML 的只读属性与候选提交桥接。
#pragma once

#include <QObject>
#include <QSize>
#include <QString>
#include <QtQml/qqmlregistration.h>

namespace caret {

class PreferencesViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool caretBlinkEnabled READ caretBlinkEnabled NOTIFY snapshotChanged)
    Q_PROPERTY(bool autoSaveEnabled READ autoSaveEnabled NOTIFY snapshotChanged)
    Q_PROPERTY(int windowWidth READ windowWidth CONSTANT)
    Q_PROPERTY(int windowHeight READ windowHeight CONSTANT)
    Q_PROPERTY(int minimumWindowWidth READ minimumWindowWidth CONSTANT)
    Q_PROPERTY(int minimumWindowHeight READ minimumWindowHeight CONSTANT)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

  public:
    explicit PreferencesViewModel(QObject* parent = nullptr);

    [[nodiscard]] bool caretBlinkEnabled() const { return m_caret_blink_enabled; }
    [[nodiscard]] bool autoSaveEnabled() const { return m_auto_save_enabled; }
    [[nodiscard]] int windowWidth() const { return m_window_width; }
    [[nodiscard]] int windowHeight() const { return m_window_height; }
    [[nodiscard]] int minimumWindowWidth() const { return m_minimum_window_width; }
    [[nodiscard]] int minimumWindowHeight() const { return m_minimum_window_height; }
    [[nodiscard]] const QString& error() const { return m_error; }

    /// 校验并持久化开关候选；失败时保留最后有效快照。
    bool setCaretBlinkEnabled(bool enabled);
    /// 刷新已由 Rust Settings 总服务提交的偏好投影。
    bool refresh();
    /// 校验并持久化 AutoSave 全局偏好；失败时保留最后有效快照。
    Q_INVOKABLE bool setAutoSaveEnabled(bool enabled);

  signals:
    void snapshotChanged();
    void errorChanged();

  private:
    void installSnapshot(bool caret_blink_enabled, bool auto_save_enabled, QSize window_size);
    void setError(QString error);

    bool m_caret_blink_enabled = true;
    bool m_auto_save_enabled = false;
    int m_window_width = 0;
    int m_window_height = 0;
    int m_minimum_window_width = 0;
    int m_minimum_window_height = 0;
    QString m_error;
};

} // namespace caret
