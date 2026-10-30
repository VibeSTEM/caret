// 负责系统剪贴板读写，并通过 Rust 执行文稿粘贴事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "layout_view_model_support.hpp"
#endif
#include <QByteArray>
#include <QClipboard>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QString>
#include <QVariant>
#include <qcontainerfwd.h>

namespace caret {
QVariantMap LayoutViewModel::copySelectionToClipboard() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("clipboard.wrong_thread"))) {
        return *error;
    }
    if (runtime == nullptr || QGuiApplication::clipboard() == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
    }
    const rust::String response = ffi::selected_text_json(*runtime);
    const QByteArray json = layout_view_model_support::response_bytes(response);
    if (json.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(json.sliced(6))}};
    }
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(json, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject() ||
        !document.object().value(QStringLiteral("text")).isString()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.invalid_selection_text")}};
    }
    QGuiApplication::clipboard()->setText(
        document.object().value(QStringLiteral("text")).toString());
    return {{QStringLiteral("valid"), true}};
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
#endif
}

QVariantMap LayoutViewModel::pasteFromClipboard() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("clipboard.wrong_thread"))) {
        return *error;
    }
    if (QGuiApplication::clipboard() == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
    }
    const QString text = QGuiApplication::clipboard()->text();
    if (text.isEmpty()) {
        return {{QStringLiteral("valid"), true}};
    }
    const rust::String input = layout_view_model_support::to_rust_string(text);
    const rust::String result = ffi::paste_text_json(*runtime, input);
    return installEditResponse(layout_view_model_support::response_bytes(result));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
#endif
}

} // namespace caret
