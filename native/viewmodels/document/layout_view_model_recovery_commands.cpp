// 负责恢复条目列表、丢弃及异步恢复会话安装。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
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
#include <qcontainerfwd.h>
#include <qtmetamacros.h>

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
    return beginPreparedDocumentInstall(PreparedDocumentSource::RecoveryEntry, id);
#else
    Q_UNUSED(id)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
