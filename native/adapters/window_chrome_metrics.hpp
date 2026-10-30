// 管理窗口标题栏系统控件度量与窗口行为。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <QEvent>
#include <QMetaObject>
#include <QObject>
#include <QPointer>
#include <QQuickWindow>
#include <QVector>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// 查询原生窗口控制占用的客户区边距，供标题栏布局避让系统控件。
class WindowChromeMetrics final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(qreal leadingSystemControlsInset READ leadingSystemControlsInset NOTIFY
                   leadingSystemControlsInsetChanged)
    Q_PROPERTY(qreal trailingSystemControlsInset READ trailingSystemControlsInset NOTIFY
                   trailingSystemControlsInsetChanged)

  public:
    explicit WindowChromeMetrics(QObject* parent = nullptr);

    /// 跟踪主窗口生命周期，使原生按钮几何变化时刷新 QML 布局。
    Q_INVOKABLE void observeWindow(QQuickWindow* window);

    /// 按 macOS 系统偏好执行标题栏双击动作。
    Q_INVOKABLE void performTitlebarDoubleClickAction();

    /// 返回最左侧系统标题栏控件右边缘相对客户区左侧的位置。
    [[nodiscard]] qreal leadingSystemControlsInset() const;

    /// 返回最右侧系统标题栏控件左边缘相对客户区右侧的位置。
    [[nodiscard]] qreal trailingSystemControlsInset() const;

  signals:
    void leadingSystemControlsInsetChanged();
    void trailingSystemControlsInsetChanged();

  private:
    bool eventFilter(QObject* watched, QEvent* event) override;

    QPointer<QQuickWindow> m_window;
    QVector<QMetaObject::Connection> m_windowConnections;
};

} // namespace caret
