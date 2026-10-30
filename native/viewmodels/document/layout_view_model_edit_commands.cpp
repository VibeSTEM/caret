// 把文本输入、段落拆分与删除命令提交给 Rust。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "layout_view_model_support.hpp"
#endif
#include <QString>
#include <QVariant>
#include <qcontainerfwd.h>

namespace caret {
QVariantMap LayoutViewModel::insertText(const QString& text) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("text.insert_wrong_thread"))) {
        return *error;
    }
    if (text.isEmpty()) {
        return {{QStringLiteral("valid"), true}};
    }
    const rust::String input = layout_view_model_support::to_rust_string(text);
    const rust::String response = ffi::insert_text_json(*runtime, input);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(text)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::splitParagraph() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("text.split_paragraph_wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::split_paragraph_json(*runtime);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::deleteBackward() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("text.delete_wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::delete_backward_json(*runtime);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::undo() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("history.undo_wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::undo_json(*runtime);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::redo() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("history.redo_wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::redo_json(*runtime);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::insertSoftBreak() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("text.soft_break_wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::insert_soft_break_json(*runtime);
    return installEditResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
