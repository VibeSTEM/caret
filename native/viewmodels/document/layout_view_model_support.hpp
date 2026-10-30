// 集中 ViewModel 文档命令的 GUI 线程、Rust 运行时与 FFI 字符串适配。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include <QByteArray>
#include <QCoreApplication>
#include <QString>
#include <QThread>
#include <QVariantMap>
#include <memory>
#include <optional>
#include <string>
#include <utility>

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

struct DocumentWriteResult final {
    bool conflict = false;
    bool replace_allowed = false;
    bool recovery_created = false;
    rust::String pending_recovery_id;
    QString recovery_id;
    QString error;
    ~DocumentWriteResult() noexcept {
        if (recovery_created && !pending_recovery_id.empty()) {
            ffi::cancel_pending_recovery_lease(std::move(pending_recovery_id));
        }
    }
};

inline std::shared_ptr<DocumentWriteResult> from_write_result(ffi::DocumentWriteResult&& result) {
    auto value = std::make_shared<DocumentWriteResult>();
    value->conflict = result.conflict;
    value->replace_allowed = result.replace_allowed;
    value->recovery_created = result.recovery_created;
    value->recovery_id = QString::fromUtf8(result.recovery_id.data(),
                                           static_cast<qsizetype>(result.recovery_id.size()));
    value->pending_recovery_id = std::move(result.recovery_id);
    value->error =
        QString::fromUtf8(result.error.data(), static_cast<qsizetype>(result.error.size()));
    return value;
}

inline bool current_session(ffi::DocumentRuntime* runtime, quint64 generation) {
    return runtime != nullptr && ffi::document_session_generation(*runtime) == generation;
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
