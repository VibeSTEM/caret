#include "window_chrome_metrics.hpp"
#import <AppKit/AppKit.h>
#include <QGuiApplication>
#include <QQuickWindow>
#include <algorithm>

namespace caret {

void WindowChromeMetrics::performTitlebarDoubleClickAction() {
    QQuickWindow* window = m_window.data();
    if (window == nullptr || QGuiApplication::platformName() != QStringLiteral("cocoa") ||
        window->handle() == nullptr) {
        return;
    }

    auto* view = reinterpret_cast<NSView*>(window->winId()); // NOLINT(performance-no-int-to-ptr)
    NSWindow* native_window = view.window;
    NSButton* zoom_button = [native_window standardWindowButton:NSWindowZoomButton];
    if (native_window == nil || zoom_button == nil || ![zoom_button isEnabled]) {
        return;
    }

    NSString* action =
        [[NSUserDefaults standardUserDefaults] stringForKey:@"AppleActionOnDoubleClick"];
    if ([action isEqualToString:@"Minimize"]) {
        [native_window performMiniaturize:zoom_button];
    } else if ([action isEqualToString:@"Fill"]) {
        if (window->windowStates().testFlag(Qt::WindowMaximized)) {
            window->showNormal();
        } else {
            window->showMaximized();
        }
    } else if (action == nil || [action isEqualToString:@"Maximize"] ||
               [action isEqualToString:@"Zoom"]) {
        [native_window performZoom:zoom_button];
    }
}

auto WindowChromeMetrics::leadingSystemControlsInset() const -> qreal {
    QQuickWindow* window = m_window.data();
    if (window == nullptr || QGuiApplication::platformName() != QStringLiteral("cocoa") ||
        !window->isExposed() || window->handle() == nullptr) {
        return 0;
    }

    // macOS 的 QWindow 原生句柄是其 NSView；从实际标准标题栏按钮位置计算避让，
    // 不把 safeAreaInsets 或平台固定像素当作交通灯宽度。
    // Cocoa 的 WId 编码 NSView*；这里只恢复 Qt 的句柄，不计算数值地址。
    auto* view = reinterpret_cast<NSView*>(window->winId()); // NOLINT(performance-no-int-to-ptr)
    NSWindow* native_window = view.window;
    NSView* content_view = native_window.contentView;
    if (native_window == nil || content_view == nil) {
        return 0;
    }

    qreal left_edge = 0;
    qreal right_edge = 0;
    bool has_button = false;
    for (const NSWindowButton button_type :
         {NSWindowCloseButton, NSWindowMiniaturizeButton, NSWindowZoomButton}) {
        NSButton* button = [native_window standardWindowButton:button_type];
        if (button == nil || button.superview == nil) {
            continue;
        }
        const NSRect frame = [button.superview convertRect:button.frame toView:content_view];
        const qreal button_left = static_cast<qreal>(NSMinX(frame));
        right_edge = std::max(right_edge, static_cast<qreal>(NSMaxX(frame)));
        left_edge = has_button ? std::min(left_edge, button_left) : button_left;
        has_button = true;
    }
    // 左侧窗口边缘到按钮组的间距也留在按钮组右侧，保证两侧留白对称。
    return has_button ? right_edge + left_edge : 0;
}

} // namespace caret
