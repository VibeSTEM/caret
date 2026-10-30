// 负责 AutoSave deadline、串行快照写入和外部冲突替换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model_support.hpp"
#endif

#include <QDebug>
#include <QFutureWatcher>
#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QString>
#include <QtConcurrent/qtconcurrentrun.h>
#include <QtCore/qlogging.h>
#include <algorithm>
#include <climits>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
void LayoutViewModel::setAutoSaveEnabled(bool enabled) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (layout_view_model_support::runtime_error(runtime,
                                                 QStringLiteral("autosave.runtime_unavailable"))) {
        return;
    }
    ffi::set_document_auto_save_enabled(*runtime, enabled);
    refreshAutoSaveDeadline();
#else
    Q_UNUSED(enabled)
#endif
}

void LayoutViewModel::refreshAutoSaveDeadline() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = document_runtime();
    if (runtime == nullptr) {
        m_auto_save_timer.stop();
        return;
    }
    const rust::String response = ffi::auto_save_deadline_json(*runtime);
    const QByteArray bytes(response.data(), static_cast<qsizetype>(response.size()));
    if (bytes.isEmpty()) {
        m_auto_save_timer.stop();
        return;
    }
    const QJsonDocument document = QJsonDocument::fromJson(bytes);
    if (!document.isObject()) {
        m_auto_save_timer.stop();
        return;
    }
    const QJsonObject object = document.object();
    bool session_ok = false;
    bool generation_ok = false;
    const qulonglong session =
        object.value(QStringLiteral("sessionGeneration")).toString().toULongLong(&session_ok);
    const qulonglong generation =
        object.value(QStringLiteral("generation")).toString().toULongLong(&generation_ok);
    if (!session_ok || !generation_ok) {
        m_auto_save_timer.stop();
        return;
    }
    m_auto_save_session_generation = session;
    m_auto_save_deadline_generation = generation;
    m_recovery_update_deadline = object.value(QStringLiteral("recovery")).toBool();
    const qint64 delay_ms = object.value(QStringLiteral("delayMs")).toInteger();
    m_auto_save_timer.start(static_cast<int>(std::clamp<qint64>(delay_ms, 0, INT_MAX)));
#endif
}

void LayoutViewModel::runAutoSaveDeadline() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (layout_view_model_support::runtime_error(runtime,
                                                 QStringLiteral("autosave.runtime_unavailable"))) {
        return;
    }
    ffi::DocumentSaveSnapshot snapshot =
        m_recovery_update_deadline
            ? ffi::recovery_update_deadline_fired(*runtime, m_auto_save_session_generation,
                                                  m_auto_save_deadline_generation)
            : ffi::auto_save_deadline_fired(*runtime, m_auto_save_session_generation,
                                            m_auto_save_deadline_generation);
    if (snapshot.stale_deadline) {
        refreshAutoSaveDeadline();
        return;
    }
    if (!snapshot.error.empty()) {
        const QString error =
            QString::fromUtf8(snapshot.error.data(), static_cast<qsizetype>(snapshot.error.size()));
        qWarning().noquote() << QStringLiteral("autosave.deadline_failed:") << error;
        emit documentFileOperationFinished(false, error);
        refreshAutoSaveDeadline();
        return;
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
    if (m_recovery_update_deadline) {
        const QString recovery_id = QString::fromUtf8(
            snapshot.recovery_id.data(), static_cast<qsizetype>(snapshot.recovery_id.size()));
        const QString original_path =
            QString::fromUtf8(snapshot.recovery_original_path.data(),
                              static_cast<qsizetype>(snapshot.recovery_original_path.size()));
        const QString recovery_baseline =
            QString::fromUtf8(snapshot.recovery_baseline_digest.data(),
                              static_cast<qsizetype>(snapshot.recovery_baseline_digest.size()));
        if (recovery_id.isEmpty() || original_path.isEmpty()) {
            (void)ffi::recovery_update_write_finished(*runtime, session_generation,
                                                      document_generation, false);
            refreshAutoSaveDeadline();
            return;
        }
        ++m_pending_file_operations;
        setDocumentFileBusy(true);
        emit documentFileChanged();
        auto* watcher = new QFutureWatcher<QString>(this);
        connect(watcher, &QFutureWatcher<QString>::finished, this,
                [this, watcher, session_generation, document_generation] {
                    const QString error = watcher->result();
                    watcher->deleteLater();
                    --m_pending_file_operations;
                    setDocumentFileBusy(m_pending_file_operations != 0);
                    ffi::DocumentRuntime* active_runtime = document_runtime();
                    if (!layout_view_model_support::current_session(active_runtime,
                                                                    session_generation)) {
                        refreshAutoSaveDeadline();
                        return;
                    }
                    (void)ffi::recovery_update_write_finished(*active_runtime, session_generation,
                                                              document_generation, error.isEmpty());
                    refreshAutoSaveDeadline();
                    if (!error.isEmpty()) {
                        qWarning().noquote() << QStringLiteral("recovery.update_failed:") << error;
                        emit documentFileOperationFinished(false, error);
                    } else {
                        emit documentFileOperationFinished(true, QString{});
                    }
                });
        watcher->setFuture(QtConcurrent::run(&m_document_io_pool, [recovery_id, original_path,
                                                                   recovery_baseline,
                                                                   bytes = std::move(bytes)] {
            rust::Vec<std::uint8_t> payload;
            payload.reserve(static_cast<std::size_t>(bytes.size()));
            for (const char byte : bytes) {
                payload.push_back(static_cast<std::uint8_t>(byte));
            }
            const rust::String result = ffi::update_recovery_document(
                layout_view_model_support::to_rust_string(recovery_id),
                layout_view_model_support::to_rust_string(original_path),
                layout_view_model_support::to_rust_string(recovery_baseline), std::move(payload));
            return QString::fromUtf8(result.data(), static_cast<qsizetype>(result.size()));
        }));
        return;
    }
    if (m_document_path.isEmpty()) {
        (void)ffi::auto_save_write_finished(
            *runtime, session_generation, document_generation, history_state_id,
            layout_view_model_support::to_rust_string(m_document_path),
            layout_view_model_support::to_rust_string(content_digest), false);
        refreshAutoSaveDeadline();
        emit documentFileOperationFinished(false, QStringLiteral("autosave.path_required"));
        return;
    }
    const QString path = m_document_path;
    const QString recovery_id = QString::fromUtf8(
        snapshot.recovery_id.data(), static_cast<qsizetype>(snapshot.recovery_id.size()));
    if (m_pending_save_operations == 0) {
        m_pending_save_digest_by_path.clear();
    }
    const QString queued_digest = m_pending_save_digest_by_path.value(path);
    m_pending_save_digest_by_path.insert(path, content_digest);
    ++m_pending_file_operations;
    ++m_pending_save_operations;
    setDocumentFileBusy(true);
    emit documentFileChanged();
    auto* watcher =
        new QFutureWatcher<std::shared_ptr<layout_view_model_support::DocumentWriteResult>>(this);
    connect(
        watcher,
        &QFutureWatcher<std::shared_ptr<layout_view_model_support::DocumentWriteResult>>::finished,
        this,
        [this, watcher, path, session_generation, document_generation, history_state_id,
         content_digest, baseline_digest] {
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
            ffi::DocumentRuntime* active_runtime = document_runtime();
            if (!layout_view_model_support::current_session(active_runtime, session_generation)) {
                return;
            }
            QString binding_error;
            if (active_runtime != nullptr) {
                if (result->conflict) {
                    m_conflict_path = path;
                    m_conflict_session_generation = session_generation;
                    m_conflict_can_replace = false;
                    emit documentFileChanged();
                    if (!result->recovery_id.isEmpty()) {
                        const rust::String binding = ffi::set_document_recovery_id(
                            *active_runtime, session_generation,
                            layout_view_model_support::to_rust_string(result->recovery_id),
                            layout_view_model_support::to_rust_string(path),
                            layout_view_model_support::to_rust_string(baseline_digest));
                        binding_error = QString::fromUtf8(binding.data(),
                                                          static_cast<qsizetype>(binding.size()));
                    }
                }
                (void)ffi::auto_save_write_finished(
                    *active_runtime, session_generation, document_generation, history_state_id,
                    layout_view_model_support::to_rust_string(path),
                    layout_view_model_support::to_rust_string(content_digest),
                    error.isEmpty() && !result->conflict);
            }
            refreshDocumentFileStatus();
            refreshAutoSaveDeadline();
            if (result->conflict) {
                emit documentFileOperationFinished(false, {}, true, !error.isEmpty(),
                                                   error.isEmpty() ? binding_error : error);
            } else if (!error.isEmpty()) {
                qWarning().noquote() << QStringLiteral("autosave.write_failed:") << error;
                emit documentFileOperationFinished(false, error);
            } else {
                emit documentFileOperationFinished(true, QString{});
            }
        });
    watcher->setFuture(
        QtConcurrent::run(&m_document_io_pool, [path, bytes = std::move(bytes), baseline_digest,
                                                queued_digest, recovery_id] {
            rust::Vec<std::uint8_t> payload;
            payload.reserve(static_cast<std::size_t>(bytes.size()));
            for (const char byte : bytes) {
                payload.push_back(static_cast<std::uint8_t>(byte));
            }
            return layout_view_model_support::from_write_result(ffi::write_document_file_checked(
                layout_view_model_support::to_rust_string(path), std::move(payload),
                layout_view_model_support::to_rust_string(baseline_digest),
                layout_view_model_support::to_rust_string(queued_digest),
                layout_view_model_support::to_rust_string(recovery_id)));
        }));
#endif
}

} // namespace caret
