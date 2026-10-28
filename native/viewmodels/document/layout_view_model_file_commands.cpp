// 负责文档文件的后台读取、顺序原子写入和 Rust 会话状态投影。
#include "layout_view_model.hpp"
// 独立 QML 测试目标不链接 Rust 应用库；桌面应用构建才启用文件会话转发。
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_snapshot_codec.hpp"
#include "layout_view_model_support.hpp"
#endif

#include <QDebug>
#include <QFutureWatcher>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QObject>
#include <QString>
#include <QtConcurrent/qtconcurrentrun.h>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <qcontainerfwd.h>
#include <qtmetamacros.h>
#include <utility>

namespace caret {

QVariantMap LayoutViewModel::openDocument(const QString& path) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("document.open_wrong_thread"))) {
        return *error;
    }
    if (path.isEmpty()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.open_missing_path")}};
    }
    if (m_document_file_busy) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.file_operation_busy")}};
    }
    if (m_document_edited) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.unsaved_changes")}};
    }

    const rust::String status_response = ffi::document_status_json(*runtime);
    const QByteArray status_bytes = layout_view_model_support::response_bytes(status_response);
    QJsonParseError status_parse_error;
    const QJsonDocument status_document =
        QJsonDocument::fromJson(status_bytes, &status_parse_error);
    if (status_parse_error.error != QJsonParseError::NoError || !status_document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.invalid_status_snapshot")}};
    }
    const QJsonObject status = status_document.object();
    bool session_generation_ok = false;
    bool document_generation_ok = false;
    const quint64 expected_session_generation = status.value(QStringLiteral("sessionGeneration"))
                                                    .toString()
                                                    .toULongLong(&session_generation_ok);
    const quint64 expected_document_generation = status.value(QStringLiteral("documentGeneration"))
                                                     .toString()
                                                     .toULongLong(&document_generation_ok);
    if (!session_generation_ok || !document_generation_ok) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.invalid_revision")}};
    }

    ++m_pending_file_operations;
    m_document_opening = true;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<DocumentOpenTaskResult>(this);
    connect(watcher, &QFutureWatcher<DocumentOpenTaskResult>::finished, this, [this, watcher] {
        const DocumentOpenTaskResult result = watcher->result();
        watcher->deleteLater();
        m_document_opening = false;
        --m_pending_file_operations;
        setDocumentFileBusy(m_pending_file_operations != 0);
        emit documentFileChanged();
        if (!result.error.isEmpty()) {
            emit documentFileOperationFinished(false, result.error);
            return;
        }
        if (result.snapshot == nullptr) {
            emit documentFileOperationFinished(false, QStringLiteral("layout.invalid_snapshot"));
            return;
        }

        ffi::DocumentRuntime* active_runtime = document_runtime();
        if (active_runtime == nullptr) {
            emit documentFileOperationFinished(false, QStringLiteral("layout.runtime_unavailable"));
            return;
        }
        const rust::String response =
            ffi::install_prepared_document(*active_runtime, result.ticket);
        const QByteArray response_bytes = layout_view_model_support::response_bytes(response);
        if (response_bytes.startsWith("error:")) {
            emit documentFileOperationFinished(false, QString::fromUtf8(response_bytes.sliced(6)));
            return;
        }
        if (result.ticket_owner != nullptr) {
            result.ticket_owner->armed = false;
        }
        if (!installSnapshot(std::move(*result.snapshot))) {
            emit documentFileOperationFinished(false, m_error);
            return;
        }
        clearSaveConflict();
        const rust::String caret_response = ffi::current_caret_json(*active_runtime);
        const QByteArray caret_bytes = layout_view_model_support::response_bytes(caret_response);
        QJsonParseError caret_parse_error;
        const QJsonDocument caret_document =
            QJsonDocument::fromJson(caret_bytes, &caret_parse_error);
        if (caret_parse_error.error != QJsonParseError::NoError || !caret_document.isObject()) {
            emit documentFileOperationFinished(false,
                                               QStringLiteral("layout.invalid_caret_geometry"));
            return;
        }
        installCaretGeometry(caret_document.object().toVariantMap());
        refreshDocumentFileStatus();
        emit documentFileOperationFinished(true, QString{});
    });
    watcher->setFuture(QtConcurrent::run(
        &m_document_io_pool, [path, expected_session_generation, expected_document_generation] {
            const rust::String rust_path = layout_view_model_support::to_rust_string(path);
            const ffi::DocumentOpenResult open_result = ffi::prepare_document_file(
                rust_path, expected_session_generation, expected_document_generation);
            DocumentOpenTaskResult result;
            if (!open_result.error.empty()) {
                result.error = QString::fromUtf8(open_result.error.data(),
                                                 static_cast<qsizetype>(open_result.error.size()));
                return result;
            }
            result.ticket = open_result.ticket;
            result.snapshot = std::make_shared<layout_snapshot::ParsedLayoutSnapshot>();
            const QByteArray snapshot_json =
                QByteArray::fromRawData(open_result.snapshot_json.data(),
                                        static_cast<qsizetype>(open_result.snapshot_json.size()));
            if (!layout_snapshot::parse(snapshot_json, *result.snapshot, result.error)) {
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
    Q_UNUSED(path)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::saveDocument(const QString& requested_path) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("document.save_wrong_thread"))) {
        return *error;
    }
    if (m_document_opening) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.file_operation_busy")}};
    }
    const QString path =
        requested_path.isEmpty()
            ? (m_pending_save_target.isEmpty() ? m_document_path : m_pending_save_target)
            : requested_path;
    if (path.isEmpty()) {
        return {{QStringLiteral("valid"), false}, {QStringLiteral("needsSaveAs"), true}};
    }

    ffi::DocumentSaveSnapshot snapshot = ffi::document_save_snapshot(*runtime);
    if (!snapshot.error.empty()) {
        const QString error =
            QString::fromUtf8(snapshot.error.data(), static_cast<qsizetype>(snapshot.error.size()));
        emit documentFileOperationFinished(false, error);
        return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), error}};
    }
    QByteArray bytes;
    bytes.reserve(static_cast<qsizetype>(snapshot.bytes.size()));
    for (const std::uint8_t byte : snapshot.bytes) {
        bytes.append(static_cast<char>(byte));
    }
    const quint64 session_generation = snapshot.session_generation;
    const quint64 document_generation = snapshot.document_generation;
    const quint64 history_state_id = snapshot.history_state_id;
    const QString content_digest = QString::fromUtf8(
        snapshot.content_digest.data(), static_cast<qsizetype>(snapshot.content_digest.size()));
    const QString baseline_digest = QString::fromUtf8(
        snapshot.baseline_digest.data(), static_cast<qsizetype>(snapshot.baseline_digest.size()));
    const QString recovery_id = QString::fromUtf8(
        snapshot.recovery_id.data(), static_cast<qsizetype>(snapshot.recovery_id.size()));
    const bool saving_current_path = path == m_document_path;
    if (m_pending_save_operations == 0) {
        m_pending_save_digest_by_path.clear();
    }
    const QString queued_digest =
        saving_current_path ? m_pending_save_digest_by_path.value(path) : QString{};
    if (saving_current_path) {
        m_pending_save_digest_by_path.insert(path, content_digest);
    }

    ++m_pending_file_operations;
    ++m_pending_save_operations;
    m_pending_save_target = path;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<QString>(this);
    connect(watcher, &QFutureWatcher<QString>::finished, this,
            [this, watcher, path, session_generation, document_generation, history_state_id,
             content_digest, saving_current_path, baseline_digest, queued_digest, recovery_id] {
                QString error = watcher->result();
                watcher->deleteLater();
                --m_pending_file_operations;
                --m_pending_save_operations;
                if (m_pending_save_operations == 0) {
                    m_pending_save_target.clear();
                    m_pending_save_digest_by_path.clear();
                }
                setDocumentFileBusy(m_pending_file_operations != 0);
                emit documentFileChanged();
                if (!error.isEmpty()) {
                    if (saving_current_path &&
                        error.startsWith(QStringLiteral("document.external_conflict"))) {
                        m_conflict_path = path;
                        m_conflict_session_generation = session_generation;
                        m_conflict_can_replace =
                            layout_view_model_support::replace_allowed_from_error(error);
                        emit documentFileChanged();
                        const QString id = layout_view_model_support::recovery_id_from_error(error);
                        if (!id.isEmpty()) {
                            if (auto* conflict_runtime = document_runtime()) {
                                const rust::String binding_error = ffi::set_document_recovery_id(
                                    *conflict_runtime, session_generation,
                                    layout_view_model_support::to_rust_string(id),
                                    layout_view_model_support::to_rust_string(path),
                                    layout_view_model_support::to_rust_string(baseline_digest));
                                if (!binding_error.empty()) {
                                    error += QStringLiteral("; recovery_binding_failed: ") +
                                             QString::fromUtf8(
                                                 binding_error.data(),
                                                 static_cast<qsizetype>(binding_error.size()));
                                }
                            } else {
                                error += QStringLiteral("; recovery_binding_failed: ") +
                                         QStringLiteral("layout.runtime_unavailable");
                            }
                        }
                    }
                    emit documentFileOperationFinished(false, error);
                    return;
                }
                ffi::DocumentRuntime* active_runtime = document_runtime();
                if (active_runtime == nullptr) {
                    emit documentFileOperationFinished(
                        false, QStringLiteral("layout.runtime_unavailable"));
                    return;
                }
                const rust::String status = ffi::mark_document_saved_json(
                    *active_runtime, session_generation, document_generation, history_state_id,
                    layout_view_model_support::to_rust_string(path),
                    layout_view_model_support::to_rust_string(content_digest));
                const QByteArray status_bytes = layout_view_model_support::response_bytes(status);
                if (status_bytes.startsWith("error:")) {
                    emit documentFileOperationFinished(false,
                                                       QString::fromUtf8(status_bytes.sliced(6)));
                    return;
                }
                clearSaveConflict();
                refreshDocumentFileStatus();
                emit documentFileOperationFinished(true, QString{});
            });
    watcher->setFuture(
        QtConcurrent::run(&m_document_io_pool, [path, bytes = std::move(bytes), baseline_digest,
                                                queued_digest, saving_current_path, recovery_id] {
            rust::Vec<std::uint8_t> payload;
            payload.reserve(static_cast<std::size_t>(bytes.size()));
            for (const char byte : bytes) {
                payload.push_back(static_cast<std::uint8_t>(byte));
            }
            const rust::String error =
                saving_current_path
                    ? ffi::write_document_file_checked(
                          layout_view_model_support::to_rust_string(path), std::move(payload),
                          layout_view_model_support::to_rust_string(baseline_digest),
                          layout_view_model_support::to_rust_string(queued_digest),
                          layout_view_model_support::to_rust_string(recovery_id))
                    : ffi::write_document_file(layout_view_model_support::to_rust_string(path),
                                               std::move(payload));
            return QString::fromUtf8(error.data(), static_cast<qsizetype>(error.size()));
        }));
    return {{QStringLiteral("valid"), true}, {QStringLiteral("pending"), true}};
#else
    Q_UNUSED(requested_path)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

void LayoutViewModel::clearSaveConflict() {
    if (m_conflict_path.isEmpty() && !m_conflict_can_replace &&
        m_conflict_session_generation == 0) {
        return;
    }
    m_conflict_path.clear();
    m_conflict_can_replace = false;
    m_conflict_session_generation = 0;
    emit documentFileChanged();
}

void LayoutViewModel::refreshDocumentFileStatus() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = document_runtime();
    if (runtime == nullptr) {
        return;
    }
    const rust::String response = ffi::document_status_json(*runtime);
    const QByteArray bytes = layout_view_model_support::response_bytes(response);
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        return;
    }
    const QJsonObject status = document.object();
    const QString path = status.value(QStringLiteral("path")).toString();
    const QString file_name = status.value(QStringLiteral("fileName")).toString();
    const bool edited = status.value(QStringLiteral("dirty")).toBool();
    const bool can_undo = status.value(QStringLiteral("canUndo")).toBool();
    const bool can_redo = status.value(QStringLiteral("canRedo")).toBool();
    if (m_document_path == path && m_document_file_name == file_name &&
        m_document_edited == edited && m_can_undo == can_undo && m_can_redo == can_redo) {
        return;
    }
    m_document_path = path;
    m_document_file_name = file_name.isEmpty() ? QStringLiteral("Untitled") : file_name;
    m_document_edited = edited;
    m_can_undo = can_undo;
    m_can_redo = can_redo;
    emit documentFileChanged();
#endif
}

void LayoutViewModel::setDocumentFileBusy(bool busy) {
    if (m_document_file_busy == busy) {
        return;
    }
    m_document_file_busy = busy;
    emit documentFileChanged();
}

QVariantMap LayoutViewModel::replaceExternalVersion() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("document.save_wrong_thread"))) {
        return *error;
    }
    if (m_conflict_path.isEmpty() || m_conflict_path != m_document_path ||
        !m_conflict_can_replace) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.no_pending_conflict")}};
    }
    ffi::DocumentSaveSnapshot snapshot = ffi::document_save_snapshot(*runtime);
    if (!snapshot.error.empty() || snapshot.session_generation != m_conflict_session_generation) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.stale_conflict")}};
    }
    QByteArray bytes;
    bytes.reserve(static_cast<qsizetype>(snapshot.bytes.size()));
    for (const std::uint8_t byte : snapshot.bytes) {
        bytes.append(static_cast<char>(byte));
    }
    const QString path = m_conflict_path;
    const QString digest = QString::fromUtf8(
        snapshot.content_digest.data(), static_cast<qsizetype>(snapshot.content_digest.size()));
    const quint64 session_generation = snapshot.session_generation;
    const quint64 document_generation = snapshot.document_generation;
    const quint64 history_state_id = snapshot.history_state_id;
    ++m_pending_file_operations;
    ++m_pending_save_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<QString>(this);
    connect(
        watcher, &QFutureWatcher<QString>::finished, this,
        [this, watcher, path, digest, session_generation, document_generation, history_state_id] {
            const QString error = watcher->result();
            watcher->deleteLater();
            --m_pending_file_operations;
            --m_pending_save_operations;
            setDocumentFileBusy(m_pending_file_operations != 0);
            if (!error.isEmpty()) {
                if (error.startsWith(QStringLiteral("document.external_conflict"))) {
                    m_conflict_path = path;
                    m_conflict_session_generation = session_generation;
                    m_conflict_can_replace = false;
                    emit documentFileChanged();
                }
                emit documentFileOperationFinished(false, error);
                return;
            }
            ffi::DocumentRuntime* active_runtime = document_runtime();
            if (active_runtime == nullptr) {
                emit documentFileOperationFinished(false,
                                                   QStringLiteral("layout.runtime_unavailable"));
                return;
            }
            const rust::String status = ffi::mark_document_saved_json(
                *active_runtime, session_generation, document_generation, history_state_id,
                layout_view_model_support::to_rust_string(path),
                layout_view_model_support::to_rust_string(digest));
            const QByteArray status_bytes = layout_view_model_support::response_bytes(status);
            if (status_bytes.startsWith("error:")) {
                emit documentFileOperationFinished(false,
                                                   QString::fromUtf8(status_bytes.sliced(6)));
                return;
            }
            clearSaveConflict();
            refreshDocumentFileStatus();
            emit documentFileOperationFinished(true, QString{});
        });
    watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [path, bytes = std::move(bytes)] {
        rust::Vec<std::uint8_t> payload;
        payload.reserve(static_cast<std::size_t>(bytes.size()));
        for (const char byte : bytes) {
            payload.push_back(static_cast<std::uint8_t>(byte));
        }
        const rust::String result = ffi::write_document_file_replace_external(
            layout_view_model_support::to_rust_string(path), std::move(payload));
        return QString::fromUtf8(result.data(), static_cast<qsizetype>(result.size()));
    }));
    return {{QStringLiteral("valid"), true}, {QStringLiteral("pending"), true}};
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
