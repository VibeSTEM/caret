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

namespace caret {

struct DocumentOpenTicketOwner final {
    qulonglong ticket = 0;
    bool armed = true;

    ~DocumentOpenTicketOwner() {
        if (armed && ticket != 0) {
            ffi::cancel_prepared_document(ticket);
        }
    }
};

namespace layout_view_model_support {

inline QString recovery_id_from_error(const QString& error) {
    const qsizetype marker = error.indexOf(QStringLiteral("recovery="));
    if (marker < 0) {
        return {};
    }
    return error.sliced(marker + 9).section(QLatin1Char(';'), 0, 0).section(QLatin1Char(' '), 0, 0);
}

inline bool replace_allowed_from_error(const QString& error) {
    return error.contains(QStringLiteral("replace_allowed=true"));
}

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

} // namespace layout_view_model_support
} // namespace caret
#endif
