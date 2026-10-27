// 集中 ViewModel 文档命令的 GUI 线程、Rust 运行时与 FFI 字符串适配。
#pragma once

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include <QByteArray>
#include <QCoreApplication>
#include <QString>
#include <QThread>
#include <QVariantMap>
#include <optional>
#include <string>

namespace caret::layout_view_model_support {

inline std::optional<QVariantMap> runtime_error(ffi::DocumentRuntime*& runtime,
                                                const QString& thread_error) {
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return QVariantMap{{QStringLiteral("valid"), false},
                           {QStringLiteral("error"), thread_error}};
    }
    runtime = document_runtime();
    if (runtime == nullptr) {
        return QVariantMap{{QStringLiteral("valid"), false},
                           {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    return std::nullopt;
}

inline QByteArray response_bytes(const rust::String& response) {
    return QByteArray(response.data(), static_cast<qsizetype>(response.size()));
}

inline rust::String to_rust_string(const QString& text) {
    const QByteArray utf8 = text.toUtf8();
    return rust::String(std::string(utf8.constData(), static_cast<std::size_t>(utf8.size())));
}

inline rust::String to_rust_string(const QByteArray& utf8) {
    return rust::String(std::string(utf8.constData(), static_cast<std::size_t>(utf8.size())));
}

} // namespace caret::layout_view_model_support
#endif
