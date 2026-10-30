// 负责恢复条目列表、丢弃及异步恢复会话安装。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_snapshot_codec.hpp"
#include "layout_view_model_support.hpp"
#endif

#include <QByteArray>
#include <QFutureWatcher>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QObject>
#include <QString>
#include <QVariantMap>
#include <QtConcurrent/qtconcurrentrun.h>
#include <memory>
#include <qcontainerfwd.h>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
QVariantMap LayoutViewModel::refreshRecoveryEntries() {
#ifdef CARET_ENABLE_APP
    if (m_recovery_operation_busy || m_document_file_busy) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("recovery.operation_unavailable")}};
    }
    m_recovery_operation_busy = true;
    ++m_pending_file_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<QString>(this);
    connect(watcher, &QFutureWatcher<QString>::finished, this, [this, watcher] {
        const QString response = watcher->result();
        watcher->deleteLater();
        m_recovery_operation_busy = false;
        --m_pending_file_operations;
        setDocumentFileBusy(m_pending_file_operations != 0);
        if (response.startsWith(QStringLiteral("error:"))) {
            emit documentFileChanged();
            emit documentFileOperationFinished(false, response.sliced(6));
            return;
        }
        QJsonParseError parse_error;
        const QJsonDocument document = QJsonDocument::fromJson(response.toUtf8(), &parse_error);
        if (parse_error.error != QJsonParseError::NoError || !document.isArray()) {
            emit documentFileChanged();
            emit documentFileOperationFinished(false, QStringLiteral("recovery.invalid_entries"));
            return;
        }
        m_recovery_entries.clear();
        for (const auto& value : document.array()) {
            if (value.isObject()) {
                m_recovery_entries.append(value.toObject().toVariantMap());
            }
        }
        emit documentFileChanged();
        emit documentFileOperationFinished(true, QString{});
    });
    watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [] {
        const rust::String response = ffi::recovery_entries_json();
        return QString::fromUtf8(response.data(), static_cast<qsizetype>(response.size()));
    }));
    return {{QStringLiteral("valid"), true}, {QStringLiteral("pending"), true}};
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::discardRecovery(const QString& id) {
#ifdef CARET_ENABLE_APP
    if (id.isEmpty() || m_recovery_operation_busy || m_document_file_busy) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("recovery.operation_unavailable")}};
    }
    const rust::String response =
        ffi::discard_recovery(layout_view_model_support::to_rust_string(id));
    const QByteArray bytes(response.data(), static_cast<qsizetype>(response.size()));
    if (!bytes.isEmpty()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(bytes)}};
    }
    for (qsizetype index = 0; index < m_recovery_entries.size(); ++index) {
        if (m_recovery_entries[index].toMap().value(QStringLiteral("id")).toString() == id) {
            m_recovery_entries.removeAt(index);
            break;
        }
    }
    emit documentFileChanged();
    return {{QStringLiteral("valid"), true}, {QStringLiteral("count"), m_recovery_entries.size()}};
#else
    Q_UNUSED(id)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::exportRecovery(const QString& id, const QString& path) {
#ifdef CARET_ENABLE_APP
    if (id.isEmpty() || path.isEmpty() || m_recovery_operation_busy || m_document_file_busy) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("recovery.operation_unavailable")}};
    }
    m_recovery_operation_busy = true;
    ++m_pending_file_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<QString>(this);
    connect(watcher, &QFutureWatcher<QString>::finished, this, [this, watcher] {
        const QString error = watcher->result();
        watcher->deleteLater();
        m_recovery_operation_busy = false;
        --m_pending_file_operations;
        setDocumentFileBusy(m_pending_file_operations != 0);
        emit documentFileChanged();
        emit documentFileOperationFinished(error.isEmpty(), error);
    });
    watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [id, path] {
        const rust::String response =
            ffi::export_recovery(layout_view_model_support::to_rust_string(id),
                                 layout_view_model_support::to_rust_string(path));
        return QString::fromUtf8(response.data(), static_cast<qsizetype>(response.size()));
    }));
    return {{QStringLiteral("valid"), true}, {QStringLiteral("pending"), true}};
#else
    Q_UNUSED(id)
    Q_UNUSED(path)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::discardCurrentRecovery() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (layout_view_model_support::runtime_error(runtime,
                                                 QStringLiteral("document.save_wrong_thread"))) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.save_wrong_thread")}};
    }
    const rust::String response = ffi::discard_document_recovery(*runtime);
    const QByteArray bytes(response.data(), static_cast<qsizetype>(response.size()));
    if (!bytes.isEmpty()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(bytes)}};
    }
    refreshAutoSaveDeadline();
    return {{QStringLiteral("valid"), true}};
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::restoreRecovery(const QString& id) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (layout_view_model_support::runtime_error(runtime,
                                                 QStringLiteral("document.open_wrong_thread"))) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.open_wrong_thread")}};
    }
    if (id.isEmpty() || m_document_file_busy || m_recovery_operation_busy || m_document_edited) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("recovery.operation_unavailable")}};
    }
    const rust::String status_response = ffi::document_status_json(*runtime);
    const QByteArray status_bytes = layout_view_model_support::response_bytes(status_response);
    const QJsonDocument status_document = QJsonDocument::fromJson(status_bytes);
    if (!status_document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.invalid_status_snapshot")}};
    }
    const QJsonObject status = status_document.object();
    bool session_ok = false;
    bool document_ok = false;
    const quint64 expected_session_generation =
        status.value(QStringLiteral("sessionGeneration")).toString().toULongLong(&session_ok);
    const quint64 expected_document_generation =
        status.value(QStringLiteral("documentGeneration")).toString().toULongLong(&document_ok);
    if (!session_ok || !document_ok) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.invalid_revision")}};
    }

    m_recovery_operation_busy = true;
    ++m_pending_file_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<DocumentOpenTaskResult>(this);
    connect(watcher, &QFutureWatcher<DocumentOpenTaskResult>::finished, this, [this, watcher] {
        const DocumentOpenTaskResult result = watcher->result();
        watcher->deleteLater();
        m_recovery_operation_busy = false;
        --m_pending_file_operations;
        setDocumentFileBusy(m_pending_file_operations != 0);
        emit documentFileChanged();
        if (!result.error.isEmpty() || result.snapshot == nullptr) {
            emit documentFileOperationFinished(
                false,
                result.error.isEmpty() ? QStringLiteral("layout.invalid_snapshot") : result.error);
            (void)refreshRecoveryEntries();
            return;
        }
        ffi::DocumentRuntime* active_runtime = document_runtime();
        if (active_runtime == nullptr) {
            emit documentFileOperationFinished(false, QStringLiteral("layout.runtime_unavailable"));
            (void)refreshRecoveryEntries();
            return;
        }
        const rust::String response =
            ffi::install_prepared_document(*active_runtime, result.ticket);
        const QByteArray response_bytes = layout_view_model_support::response_bytes(response);
        if (!response_bytes.isEmpty()) {
            emit documentFileOperationFinished(false, QString::fromUtf8(response_bytes));
            (void)refreshRecoveryEntries();
            return;
        }
        if (result.ticket_owner != nullptr) {
            result.ticket_owner->armed = false;
        }
        if (!installSnapshot(std::move(*result.snapshot))) {
            emit documentFileOperationFinished(false, m_error);
            return;
        }
        const rust::String caret_response = ffi::current_caret_json(*active_runtime);
        const QByteArray caret_bytes = layout_view_model_support::response_bytes(caret_response);
        const QJsonDocument caret_document = QJsonDocument::fromJson(caret_bytes);
        if (!caret_document.isObject()) {
            emit documentFileOperationFinished(false,
                                               QStringLiteral("layout.invalid_caret_geometry"));
            return;
        }
        installCaretGeometry(caret_document.object().toVariantMap());
        refreshDocumentFileStatus();
        refreshAutoSaveDeadline();
        emit documentFileOperationFinished(true, QString{});
    });
    watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [id, expected_session_generation,
                                                               expected_document_generation] {
        const auto prepared = ffi::prepare_recovery_document(
            layout_view_model_support::to_rust_string(id), expected_session_generation,
            expected_document_generation);
        DocumentOpenTaskResult result;
        if (!prepared.error.empty()) {
            result.error = QString::fromUtf8(prepared.error.data(),
                                             static_cast<qsizetype>(prepared.error.size()));
            return result;
        }
        result.ticket = prepared.ticket;
        result.snapshot = std::make_shared<layout_snapshot::ParsedLayoutSnapshot>();
        const QByteArray json = QByteArray::fromRawData(
            prepared.snapshot_json.data(), static_cast<qsizetype>(prepared.snapshot_json.size()));
        if (!layout_snapshot::parse(json, *result.snapshot, result.error)) {
            ffi::cancel_prepared_document(result.ticket);
            result.ticket = 0;
            result.snapshot.reset();
            return result;
        }
        result.ticket_owner = std::make_shared<DocumentOpenTicketOwner>();
        result.ticket_owner->ticket = result.ticket;
        return result;
    }));
    return {{QStringLiteral("valid"), true}, {QStringLiteral("pending"), true}};
#else
    Q_UNUSED(id)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
