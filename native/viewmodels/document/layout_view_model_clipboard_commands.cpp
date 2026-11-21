// 负责系统剪贴板多格式读写，并通过 Rust 执行文稿片段粘贴事务。
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
#include <QMimeData>
#include <QString>
#include <QVariant>
#include <memory>
#include <qcontainerfwd.h>

namespace caret {
namespace {

// Caret 专用片段 MIME：Qt 只原样读写信封字节，格式解释与校验归 Rust。
constexpr auto kClipboardFragmentMime = "application/vnd.caret.fragment+json";

#ifdef CARET_ENABLE_APP
QString fragment_mime_name() { return QString::fromLatin1(kClipboardFragmentMime); }

// 片段载荷必须是合法 UTF-8 才能进入 cxx `rust::String`；非 UTF-8 由 Qt 替换为
// U+FFFD，往返编码不再相等。这里先返回稳定错误码，避免异常逃出 Q_INVOKABLE。
bool fragment_payload_is_utf8(const QByteArray& payload) {
    const QString decoded = QString::fromUtf8(payload);
    return decoded.toUtf8() == payload;
}
#endif

} // namespace

QVariantMap LayoutViewModel::copySelectionToClipboard() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("clipboard.wrong_thread"))) {
        return *error;
    }
    QClipboard* clipboard = QGuiApplication::clipboard();
    if (runtime == nullptr || clipboard == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
    }
    const rust::String response = ffi::clipboard_copy_json(*runtime);
    const QByteArray json = layout_view_model_support::response_bytes(response);
    if (json.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(json.sliced(6))}};
    }
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(json, &parse_error);
    const QJsonObject payload = document.object();
    const QJsonValue fragment = payload.value(QStringLiteral("fragment"));
    const QJsonValue text = payload.value(QStringLiteral("text"));
    if (parse_error.error != QJsonParseError::NoError || !document.isObject() || !text.isString() ||
        !(fragment.isString() || fragment.isNull())) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.invalid_payload")}};
    }
    if (fragment.isNull() && text.toString().isEmpty()) {
        // 空选区没有可传输内容：保持系统剪贴板不变，也不把旧片段降级为纯文本。
        return {{QStringLiteral("valid"), true}};
    }
    if (fragment.isNull()) {
        // 没有可传输结构载荷但仍有可读文本的选区：只写 text/plain。
        clipboard->setText(text.toString());
        return {{QStringLiteral("valid"), true}};
    }
    // QClipboard 接管 QMimeData 所有权；X11/Wayland 需要堆分配以跨进程延迟读取。
    auto mime = std::make_unique<QMimeData>();
    mime->setData(fragment_mime_name(), fragment.toString().toUtf8());
    mime->setText(text.toString());
    clipboard->setMimeData(mime.release());
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
    QClipboard* clipboard = QGuiApplication::clipboard();
    if (runtime == nullptr || clipboard == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
    }
    // 只读取一次 mimeData，避免格式探测与内容读取之间系统剪贴板被其他进程替换。
    const QMimeData* mime = clipboard->mimeData();
    const bool has_fragment_format = mime != nullptr && mime->hasFormat(fragment_mime_name());
    QString text;
    QByteArray fragment;
    if (has_fragment_format) {
        // 原样转发 Caret 信封字节，由 Rust 严格解码；Qt 不解码也不在失败后降级为纯文本。
        fragment = mime->data(fragment_mime_name());
        text = mime->text();
        if (!fragment_payload_is_utf8(fragment)) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"),
                     QStringLiteral("clipboard.invalid_fragment: payload is not UTF-8")}};
        }
    } else {
        // 与格式探测共用同一次 mimeData 快照，避免两次读取之间被外部进程替换。
        text = mime != nullptr ? mime->text() : clipboard->text();
    }
    if (has_fragment_format && fragment.isEmpty()) {
        // 声明了 Caret 格式但载荷为空属于损坏数据，明确拒绝而不是当作纯文本粘贴。
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("clipboard.invalid_fragment")}};
    }
    if (fragment.isEmpty() && text.isEmpty()) {
        return {{QStringLiteral("valid"), true}};
    }
    const rust::String result =
        ffi::clipboard_paste_json(*runtime, layout_view_model_support::to_rust_string(text),
                                  layout_view_model_support::to_rust_string(fragment));
    return installEditResponse(layout_view_model_support::response_bytes(result));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("clipboard.unavailable")}};
#endif
}

} // namespace caret
