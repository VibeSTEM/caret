// 负责文档文件的后台读取、顺序原子写入和 Rust 会话状态投影。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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

// openDocument 与 restoreRecovery 共用的异步安装骨架：在 GUI 线程读取并校验
// session/document 代际，在 IO 池准备候选文档，再回到 GUI 线程重新核对代际后安装。
// 两个命令只在忙碌标记、安装错误解释、失败清理与成功后续动作上不同。
QVariantMap LayoutViewModel::beginPreparedDocumentInstall(PreparedDocumentSource source,
                                                          const QString& source_id) {
#ifdef CARET_ENABLE_APP
    const bool restoring_recovery = source == PreparedDocumentSource::RecoveryEntry;
    ffi::DocumentRuntime* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
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
    // 解析失败时 fromJson 必得非对象文档，两个命令共用同一道状态快照校验。
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

    if (restoring_recovery) {
        m_recovery_operation_busy = true;
    } else {
        m_document_opening = true;
    }
    ++m_pending_file_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher = new QFutureWatcher<DocumentOpenTaskResult>(this);
    connect(
        watcher, &QFutureWatcher<DocumentOpenTaskResult>::finished, this,
        [this, watcher, restoring_recovery] {
            const DocumentOpenTaskResult result = watcher->result();
            watcher->deleteLater();
            if (restoring_recovery) {
                m_recovery_operation_busy = false;
            } else {
                m_document_opening = false;
            }
            --m_pending_file_operations;
            setDocumentFileBusy(m_pending_file_operations != 0);
            emit documentFileChanged();
            // 恢复命令失败后重新读取条目，让列表与恢复区保持一致。
            const auto refresh_recovery_after_failure = [this, restoring_recovery] {
                if (restoring_recovery) {
                    (void)refreshRecoveryEntries();
                }
            };
            if (!result.error.isEmpty()) {
                emit documentFileOperationFinished(false, result.error);
                refresh_recovery_after_failure();
                return;
            }
            if (result.snapshot == nullptr) {
                emit documentFileOperationFinished(false,
                                                   QStringLiteral("layout.invalid_snapshot"));
                refresh_recovery_after_failure();
                return;
            }

            ffi::DocumentRuntime* active_runtime = document_runtime();
            if (active_runtime == nullptr) {
                emit documentFileOperationFinished(false,
                                                   QStringLiteral("layout.runtime_unavailable"));
                refresh_recovery_after_failure();
                return;
            }
            const rust::String install_response =
                ffi::install_prepared_document(*active_runtime, result.ticket);
            const QByteArray install_bytes =
                layout_view_model_support::response_bytes(install_response);
            if (restoring_recovery) {
                // 恢复安装失败返回完整的服务端错误文本。
                if (!install_bytes.isEmpty()) {
                    emit documentFileOperationFinished(false, QString::fromUtf8(install_bytes));
                    refresh_recovery_after_failure();
                    return;
                }
            } else if (install_bytes.startsWith("error:")) {
                // 打开命令只把 error: 前缀之后的部分作为错误文本。
                emit documentFileOperationFinished(false,
                                                   QString::fromUtf8(install_bytes.sliced(6)));
                return;
            }
            if (result.ticket_owner != nullptr) {
                result.ticket_owner->armed = false;
            }
            if (!installSnapshot(std::move(*result.snapshot))) {
                emit documentFileOperationFinished(false, m_error);
                return;
            }
            if (!restoring_recovery) {
                clearSaveConflict();
            }
            const rust::String caret_response = ffi::current_caret_json(*active_runtime);
            const QByteArray caret_bytes =
                layout_view_model_support::response_bytes(caret_response);
            // 解析失败时 fromJson 必得非对象文档，两个命令共用同一道 caret 校验。
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
            if (restoring_recovery) {
                refreshAutoSaveDeadline();
            }
            emit documentFileOperationFinished(true, QString{});
        });
    watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [source, source_id,
                                                               expected_session_generation,
                                                               expected_document_generation] {
        const ffi::DocumentOpenResult prepared =
            source == PreparedDocumentSource::RecoveryEntry
                ? ffi::prepare_recovery_document(
                      layout_view_model_support::to_rust_string(source_id),
                      expected_session_generation, expected_document_generation)
                : ffi::prepare_document_file(layout_view_model_support::to_rust_string(source_id),
                                             expected_session_generation,
                                             expected_document_generation);
        DocumentOpenTaskResult result;
        if (!prepared.error.empty()) {
            result.error = QString::fromUtf8(prepared.error.data(),
                                             static_cast<qsizetype>(prepared.error.size()));
            return result;
        }
        result.ticket = prepared.ticket;
        result.snapshot = std::make_shared<layout_snapshot::ParsedLayoutSnapshot>();
        const QByteArray snapshot_json = QByteArray::fromRawData(
            prepared.snapshot_json.data(), static_cast<qsizetype>(prepared.snapshot_json.size()));
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
    Q_UNUSED(source)
    Q_UNUSED(source_id)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

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
    return beginPreparedDocumentInstall(PreparedDocumentSource::FilePath, path);
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
        emit documentSaveFinished(false);
        return *error;
    }
    if (m_document_opening) {
        emit documentSaveFinished(false);
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.file_operation_busy")}};
    }
    if (m_document_math_source_draft_pending) {
        const QString error = QStringLiteral("document.save_incomplete_math_source_draft");
        emit documentSaveFinished(false);
        emit documentFileOperationFinished(false, error);
        return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), error}};
    }
    const QString path =
        requested_path.isEmpty()
            ? (m_pending_save_target.isEmpty() ? m_document_path : m_pending_save_target)
            : requested_path;
    if (path.isEmpty()) {
        emit documentSaveFinished(false);
        return {{QStringLiteral("valid"), false}, {QStringLiteral("needsSaveAs"), true}};
    }

    ffi::DocumentSaveSnapshot snapshot = ffi::document_save_snapshot(*runtime);
    if (!snapshot.error.empty()) {
        const QString error =
            QString::fromUtf8(snapshot.error.data(), static_cast<qsizetype>(snapshot.error.size()));
        emit documentSaveFinished(false);
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
    const quint64 recovery_binding_generation = snapshot.recovery_binding_generation;
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
    auto* watcher =
        new QFutureWatcher<std::shared_ptr<layout_view_model_support::DocumentWriteResult>>(this);
    connect(
        watcher,
        &QFutureWatcher<std::shared_ptr<layout_view_model_support::DocumentWriteResult>>::finished,
        this,
        [this, watcher, path, session_generation, document_generation, history_state_id,
         recovery_binding_generation, content_digest, saving_current_path, baseline_digest,
         queued_digest, recovery_id] {
            const auto result = watcher->result();
            QString error = result->error;
            watcher->deleteLater();
            --m_pending_file_operations;
            --m_pending_save_operations;
            if (m_pending_save_operations == 0) {
                m_pending_save_target.clear();
                m_pending_save_digest_by_path.clear();
            }
            setDocumentFileBusy(m_pending_file_operations != 0);
            emit documentFileChanged();
            if (!layout_view_model_support::current_session(document_runtime(),
                                                            session_generation)) {
                emit documentSaveFinished(false);
                emit documentFileActivityFinished();
                return;
            }
            if (result->conflict) {
                QString binding_error;
                if (!result->recovery_id.isEmpty()) {
                    const rust::String binding = ffi::set_document_recovery_id(
                        *document_runtime(), session_generation, document_generation,
                        recovery_binding_generation,
                        layout_view_model_support::to_rust_string(result->recovery_id),
                        layout_view_model_support::to_rust_string(path),
                        layout_view_model_support::to_rust_string(baseline_digest));
                    binding_error =
                        QString::fromUtf8(binding.data(), static_cast<qsizetype>(binding.size()));
                }
                m_conflict_path = path;
                m_conflict_session_generation = session_generation;
                m_conflict_can_replace = result->replace_allowed && binding_error.isEmpty();
                emit documentFileChanged();
                refreshDocumentFileStatus();
                refreshAutoSaveDeadline();
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false, {}, true, !error.isEmpty(),
                                                   error.isEmpty() ? binding_error : error);
                return;
            }
            if (!error.isEmpty()) {
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false, error);
                return;
            }
            ffi::DocumentRuntime* active_runtime = document_runtime();
            if (active_runtime == nullptr) {
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false,
                                                   QStringLiteral("layout.runtime_unavailable"));
                return;
            }
            const rust::String status = ffi::mark_document_saved_json(
                *active_runtime, session_generation, document_generation, history_state_id,
                layout_view_model_support::to_rust_string(path),
                layout_view_model_support::to_rust_string(content_digest));
            const QByteArray status_bytes = layout_view_model_support::response_bytes(status);
            if (status_bytes.startsWith("error:")) {
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false,
                                                   QString::fromUtf8(status_bytes.sliced(6)));
                return;
            }
            clearSaveConflict();
            refreshDocumentFileStatus();
            emit documentSaveFinished(true);
            emit documentFileOperationFinished(true, QString{});
        });
    watcher->setFuture(
        QtConcurrent::run(&m_document_io_pool, [path, bytes = std::move(bytes), baseline_digest,
                                                queued_digest, saving_current_path, recovery_id,
                                                recovery_binding_generation, document_generation] {
            rust::Vec<std::uint8_t> payload;
            payload.reserve(static_cast<std::size_t>(bytes.size()));
            for (const char byte : bytes) {
                payload.push_back(static_cast<std::uint8_t>(byte));
            }
            if (saving_current_path) {
                return layout_view_model_support::from_write_result(
                    ffi::write_document_file_checked(
                        layout_view_model_support::to_rust_string(path), std::move(payload),
                        layout_view_model_support::to_rust_string(baseline_digest),
                        layout_view_model_support::to_rust_string(queued_digest),
                        layout_view_model_support::to_rust_string(recovery_id),
                        recovery_binding_generation, document_generation));
            }
            const rust::String error = ffi::write_document_file(
                layout_view_model_support::to_rust_string(path), std::move(payload));
            auto result = std::make_shared<layout_view_model_support::DocumentWriteResult>();
            result->error = QString::fromUtf8(error.data(), static_cast<qsizetype>(error.size()));
            return result;
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
    installDocumentFileStatus(document.object());
#endif
}

bool LayoutViewModel::installDocumentFileStatus(const QJsonObject& status) {
    const QJsonValue path_value = status.value(QStringLiteral("path"));
    const QJsonValue file_name_value = status.value(QStringLiteral("fileName"));
    const QJsonValue edited_value = status.value(QStringLiteral("dirty"));
    const QJsonValue math_source_draft_pending_value =
        status.value(QStringLiteral("mathSourceDraftPending"));
    const QJsonValue can_undo_value = status.value(QStringLiteral("canUndo"));
    const QJsonValue can_redo_value = status.value(QStringLiteral("canRedo"));
    if (!path_value.isString() || !file_name_value.isString() || !edited_value.isBool() ||
        !math_source_draft_pending_value.isBool() || !can_undo_value.isBool() ||
        !can_redo_value.isBool()) {
        return false;
    }
    const QString path = path_value.toString();
    const QString file_name = file_name_value.toString();
    const bool edited = edited_value.toBool();
    const bool math_source_draft_pending = math_source_draft_pending_value.toBool();
    const bool can_undo = can_undo_value.toBool();
    const bool can_redo = can_redo_value.toBool();
    if (m_document_path == path && m_document_file_name == file_name &&
        m_document_edited == edited &&
        m_document_math_source_draft_pending == math_source_draft_pending &&
        m_can_undo == can_undo && m_can_redo == can_redo) {
        return true;
    }
    m_document_path = path;
    m_document_file_name = file_name.isEmpty() ? QStringLiteral("Untitled") : file_name;
    m_document_edited = edited;
    m_document_math_source_draft_pending = math_source_draft_pending;
    m_can_undo = can_undo;
    m_can_redo = can_redo;
    emit documentFileChanged();
    return true;
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
        emit documentSaveFinished(false);
        return *error;
    }
    if (m_conflict_path.isEmpty() || m_conflict_path != m_document_path ||
        !m_conflict_can_replace) {
        emit documentSaveFinished(false);
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("document.no_pending_conflict")}};
    }
    ffi::DocumentSaveSnapshot snapshot = ffi::document_save_snapshot(*runtime);
    if (!snapshot.error.empty() || snapshot.session_generation != m_conflict_session_generation) {
        emit documentSaveFinished(false);
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
            if (!layout_view_model_support::current_session(document_runtime(),
                                                            session_generation)) {
                emit documentSaveFinished(false);
                emit documentFileActivityFinished();
                return;
            }
            if (!error.isEmpty()) {
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false, error);
                return;
            }
            ffi::DocumentRuntime* active_runtime = document_runtime();
            if (active_runtime == nullptr) {
                emit documentSaveFinished(false);
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
                emit documentSaveFinished(false);
                emit documentFileOperationFinished(false,
                                                   QString::fromUtf8(status_bytes.sliced(6)));
                return;
            }
            clearSaveConflict();
            refreshDocumentFileStatus();
            emit documentSaveFinished(true);
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
