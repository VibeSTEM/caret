#include "window_chrome_metrics.hpp"

#include <QEvent>
#include <QObject>
#include <QQuickWindow>
#include <QWindow>
#include <qnamespace.h>
#include <QtCore/qtmetamacros.h>
#ifdef Q_OS_WIN
#include <QtCore/qt_windows.h>
#endif

namespace caret {

WindowChromeMetrics::WindowChromeMetrics(QObject* parent) : QObject(parent) {}

void WindowChromeMetrics::observeWindow(QQuickWindow* window) {
    if (m_window == window) {
        return;
    }
    if (m_window != nullptr) {
        m_window->removeEventFilter(this);
    }
    for (const auto& connection : m_windowConnections) {
        QObject::disconnect(connection);
    }
    m_windowConnections.clear();
    m_window = window;
    if (window != nullptr) {
        window->installEventFilter(this);
        const auto notify = [this] {
            emit leadingSystemControlsInsetChanged();
            emit trailingSystemControlsInsetChanged();
        };
        m_windowConnections.append(connect(window, &QWindow::visibilityChanged, this, notify));
        m_windowConnections.append(connect(window, &QWindow::windowStateChanged, this, notify));
        m_windowConnections.append(connect(window, &QWindow::screenChanged, this, notify));
        m_windowConnections.append(connect(window, &QObject::destroyed, this, [this] {
            m_window = nullptr;
            emit leadingSystemControlsInsetChanged();
            emit trailingSystemControlsInsetChanged();
        }));
    }
    emit leadingSystemControlsInsetChanged();
    emit trailingSystemControlsInsetChanged();
    // visible 的窗口可能在观察之前就已经 expose，排队再量一次系统按钮。
    QMetaObject::invokeMethod(
        this,
        [this] {
            emit leadingSystemControlsInsetChanged();
            emit trailingSystemControlsInsetChanged();
        },
        Qt::QueuedConnection);
}

#ifndef Q_OS_MACOS
void WindowChromeMetrics::performTitlebarDoubleClickAction() {
    if (m_window == nullptr) {
        return;
    }

    if (m_window->windowStates().testFlag(Qt::WindowMaximized)) {
        m_window->showNormal();
    } else {
        m_window->showMaximized();
    }
}
#endif

bool WindowChromeMetrics::eventFilter(QObject* watched, QEvent* event) {
    if (watched == m_window && (event->type() == QEvent::Show || event->type() == QEvent::Expose ||
                                event->type() == QEvent::DevicePixelRatioChange ||
                                event->type() == QEvent::ScreenChangeInternal)) {
        emit leadingSystemControlsInsetChanged();
        emit trailingSystemControlsInsetChanged();
    }
    return QObject::eventFilter(watched, event);
}

#ifndef Q_OS_MACOS
auto WindowChromeMetrics::leadingSystemControlsInset() const -> qreal { return 0; }
#endif

#ifndef Q_OS_WIN
auto WindowChromeMetrics::trailingSystemControlsInset() const -> qreal { return 0; }
#else
auto WindowChromeMetrics::trailingSystemControlsInset() const -> qreal {
    QQuickWindow* window = m_window.data();
    if (window == nullptr || !window->isExposed() || window->handle() == nullptr) {
        return 0;
    }

    const HWND handle = reinterpret_cast<HWND>(window->winId());
    const UINT dpi = GetDpiForWindow(handle);
    if (dpi == 0) {
        return 0;
    }

    // Qt 6.11.2 按 (缩放边框 + 标题栏高度) × 1.5 计算每个按钮的绘制和命中宽度。
    // 这里用 Windows 当前 DPI 的系统指标复用同一几何，避免 SafeArea.right 为零或
    // 另行维护固定像素；Qt 升级时需复核该平台实现是否改变。
    const int title_bar_height = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) +
                                 GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) +
                                 GetSystemMetricsForDpi(SM_CYCAPTION, dpi);
    const int button_width = static_cast<int>(title_bar_height * 1.5);
    return button_width > 0 ? 3.0 * button_width / window->devicePixelRatio() : 0;
}
#endif

} // namespace caret
