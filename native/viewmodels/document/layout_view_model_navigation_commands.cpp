// 把视觉方向键导航命令提交给 Rust。
#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "layout_view_model_support.hpp"
#endif
#include <QString>
#include <QVariant>
#include <qcontainerfwd.h>

namespace caret {
QVariantMap LayoutViewModel::moveVisualCaret(bool forward, bool extend) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("navigation.wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::move_visual_caret_json(*runtime, forward, extend);
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(forward)
    Q_UNUSED(extend)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::moveVerticalCaret(int line_delta, bool extend) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("navigation.wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::move_vertical_caret_json(*runtime, line_delta, extend);
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(line_delta)
    Q_UNUSED(extend)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::moveToVisualLineEdge(bool end, bool extend) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("navigation.wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::move_to_visual_line_edge_json(*runtime, end, extend);
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(end)
    Q_UNUSED(extend)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
