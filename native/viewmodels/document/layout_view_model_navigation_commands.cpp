// 把视觉方向键导航命令提交给 Rust。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "layout_view_model_support.hpp"
#include <QByteArray>
#include <cstddef>
#include <string>
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

QVariantMap LayoutViewModel::moveStructure(const QString& action) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("navigation.structure_wrong_thread"))) {
        return *error;
    }
    const QByteArray action_utf8 = action.toUtf8();
    const rust::String response = ffi::move_structure_json(
        *runtime, rust::String(std::string(action_utf8.constData(),
                                           static_cast<std::size_t>(action_utf8.size()))));
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(action)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::handleEditorKey(const QString& key) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("interaction.editor_key_wrong_thread"))) {
        return *error;
    }
    const QByteArray key_utf8 = key.toUtf8();
    const rust::String response = ffi::handle_editor_key_json(
        *runtime,
        rust::String(std::string(key_utf8.constData(), static_cast<std::size_t>(key_utf8.size()))));
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(key)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
