// 验证恢复列表、恢复文档、导出外部版本与丢弃操作的用户可见结果。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model.hpp"
#include <QByteArray>
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QtCore/qglobal.h>
#include <QtCore/qiodevice.h>
#include <QtCore/qlist.h>
#include <QtCore/qstring.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtestcase.h>
#include <cstddef>
#include <cstdint>
#include <string_view>
#include <utility>

namespace {

rust::String to_rust_string(const QString& value) {
    const QByteArray bytes = value.toUtf8();
    return rust::String(bytes.constData(), static_cast<std::size_t>(bytes.size()));
}

rust::Vec<std::uint8_t> to_rust_bytes(const QByteArray& bytes) {
    rust::Vec<std::uint8_t> result;
    result.reserve(static_cast<std::size_t>(bytes.size()));
    for (const char byte : bytes) {
        result.push_back(static_cast<std::uint8_t>(byte));
    }
    return result;
}

bool install_blank_runtime() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    if (std::string_view(snapshot.data(), snapshot.size()).starts_with("error:")) {
        return false;
    }
    if (!caret::install_layout_snapshot(
            QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size())))) {
        return false;
    }
    return caret::install_document_runtime(std::move(runtime));
}

QList<QVariant> wait_for_operation(QSignalSpy& spy) {
    if (spy.isEmpty() && !spy.wait(10000)) {
        return {};
    }
    return spy.takeFirst();
}

} // namespace

void ShellLoadTest::recoveryEntriesCanBeRestoredExportedAndDiscarded() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    QVERIFY(caret::ffi::configure_document_recovery(to_rust_string(directory.path())).empty());

    const QString document_path = directory.filePath(QStringLiteral("recoverable.cdoc"));
    QString local_recovery_id;
    {
        QVERIFY(install_blank_runtime());
        caret::LayoutViewModel source_view_model;
        QSignalSpy operations(&source_view_model,
                              &caret::LayoutViewModel::documentFileOperationFinished);
        QVERIFY(operations.isValid());
        QVERIFY(source_view_model.insertText(QStringLiteral("恢复的正文"))
                    .value(QStringLiteral("valid"))
                    .toBool());
        QVERIFY(
            source_view_model.saveDocument(document_path).value(QStringLiteral("valid")).toBool());
        const QList<QVariant> saved = wait_for_operation(operations);
        QVERIFY(!saved.isEmpty());
        QVERIFY2(saved.at(0).toBool(), qPrintable(saved.at(1).toString()));

        QFile external_file(document_path);
        QVERIFY(external_file.open(QIODevice::WriteOnly | QIODevice::Truncate));
        QCOMPARE(external_file.write("external version"), qint64(16));
        external_file.close();
        QVERIFY(source_view_model.insertText(QStringLiteral(" 未保存编辑"))
                    .value(QStringLiteral("valid"))
                    .toBool());
        QVERIFY(source_view_model.saveDocument().value(QStringLiteral("valid")).toBool());
        const QList<QVariant> conflicted = wait_for_operation(operations);
        QVERIFY(!conflicted.isEmpty());
        QVERIFY(!conflicted.at(0).toBool());
        QVERIFY(conflicted.at(2).toBool());

        QVERIFY(source_view_model.insertText(QStringLiteral(" 后续恢复编辑"))
                    .value(QStringLiteral("valid"))
                    .toBool());
        const QList<QVariant> recovery_updated = wait_for_operation(operations);
        QVERIFY(!recovery_updated.isEmpty());
        QVERIFY2(recovery_updated.at(0).toBool(), qPrintable(recovery_updated.at(1).toString()));

        // 模拟应用重启：释放旧会话租约后，恢复列表才应向用户展示该条目。
        QVERIFY(install_blank_runtime());
        QVERIFY(
            source_view_model.refreshRecoveryEntries().value(QStringLiteral("pending")).toBool());
        const QList<QVariant> listed = wait_for_operation(operations);
        QVERIFY(!listed.isEmpty());
        QVERIFY2(listed.at(0).toBool(), qPrintable(listed.at(1).toString()));
        QCOMPARE(source_view_model.recoveryEntries().size(), 1);
        local_recovery_id = source_view_model.recoveryEntries()
                                .first()
                                .toMap()
                                .value(QStringLiteral("id"))
                                .toString();
        QVERIFY(!local_recovery_id.isEmpty());
    }

    QVERIFY(install_blank_runtime());
    caret::LayoutViewModel restored_view_model;
    QSignalSpy restored_operations(&restored_view_model,
                                   &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(restored_operations.isValid());
    const QVariantMap restore = restored_view_model.restoreRecovery(local_recovery_id);
    QVERIFY2(restore.value(QStringLiteral("pending")).toBool(),
             qPrintable(restore.value(QStringLiteral("error")).toString()));
    const QList<QVariant> restored = wait_for_operation(restored_operations);
    QVERIFY(!restored.isEmpty());
    QVERIFY2(restored.at(0).toBool(), qPrintable(restored.at(1).toString()));
    QCOMPARE(restored_view_model.paragraphLayouts().size(), 1);
    QVERIFY(restored_view_model.paragraphLayouts()
                .first()
                .toMap()
                .value(QStringLiteral("text"))
                .toString()
                .contains(QStringLiteral("恢复的正文 未保存编辑 后续恢复编辑")));
    QVERIFY(restored_view_model.documentSaveStatus() == QStringLiteral("edited"));

    QVERIFY(restored_view_model.discardCurrentRecovery().value(QStringLiteral("valid")).toBool());
    QVERIFY(restored_view_model.refreshRecoveryEntries().value(QStringLiteral("pending")).toBool());
    const QList<QVariant> after_discard = wait_for_operation(restored_operations);
    QVERIFY(!after_discard.isEmpty());
    QVERIFY2(after_discard.at(0).toBool(), qPrintable(after_discard.at(1).toString()));
    QVERIFY(restored_view_model.recoveryEntries().isEmpty());

    const QByteArray external_bytes("original external bytes");
    QFile replacement_target(document_path);
    QVERIFY(replacement_target.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(replacement_target.write(external_bytes), qint64(external_bytes.size()));
    replacement_target.close();
    auto replacement_runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot_json = caret::ffi::layout_snapshot_json(*replacement_runtime);
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot_json.data(), static_cast<qsizetype>(snapshot_json.size()))));
    const auto replacement_snapshot = caret::ffi::document_save_snapshot(*replacement_runtime);
    QByteArray replacement_bytes;
    replacement_bytes.reserve(static_cast<qsizetype>(replacement_snapshot.bytes.size()));
    for (const std::uint8_t byte : replacement_snapshot.bytes) {
        replacement_bytes.append(static_cast<char>(byte));
    }
    QVERIFY(caret::ffi::write_document_file_replace_external(to_rust_string(document_path),
                                                             to_rust_bytes(replacement_bytes))
                .empty());

    QVERIFY(restored_view_model.refreshRecoveryEntries().value(QStringLiteral("pending")).toBool());
    const QList<QVariant> external_listed = wait_for_operation(restored_operations);
    QVERIFY(!external_listed.isEmpty());
    QVERIFY2(external_listed.at(0).toBool(), qPrintable(external_listed.at(1).toString()));
    QCOMPARE(restored_view_model.recoveryEntries().size(), 1);
    const QVariantMap external_entry = restored_view_model.recoveryEntries().first().toMap();
    QCOMPARE(external_entry.value(QStringLiteral("kind")).toString(),
             QStringLiteral("external_version"));
    const QString external_id = external_entry.value(QStringLiteral("id")).toString();
    const QString export_path = directory.filePath(QStringLiteral("exported-original.bin"));
    QVERIFY(restored_view_model.exportRecovery(external_id, export_path)
                .value(QStringLiteral("pending"))
                .toBool());
    const QList<QVariant> exported = wait_for_operation(restored_operations);
    QVERIFY(!exported.isEmpty());
    QVERIFY2(exported.at(0).toBool(), qPrintable(exported.at(1).toString()));
    QFile exported_file(export_path);
    QVERIFY(exported_file.open(QIODevice::ReadOnly));
    QCOMPARE(exported_file.readAll(), external_bytes);

    const QVariantMap discarded = restored_view_model.discardRecovery(external_id);
    QVERIFY2(discarded.value(QStringLiteral("valid")).toBool(),
             qPrintable(discarded.value(QStringLiteral("error")).toString()));
    QCOMPARE(discarded.value(QStringLiteral("count")).toInt(), 0);
    QVERIFY(restored_view_model.recoveryEntries().isEmpty());
}

void ShellLoadTest::missingRecoveryEntryFailsWithoutReplacingTheOpenDocument() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    QVERIFY(caret::ffi::configure_document_recovery(to_rust_string(directory.path())).empty());
    QVERIFY(install_blank_runtime());

    caret::LayoutViewModel view_model;
    QSignalSpy operations(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    const QVariantMap missing = view_model.restoreRecovery(QStringLiteral("missing-entry"));
    QVERIFY2(missing.value(QStringLiteral("pending")).toBool(),
             qPrintable(missing.value(QStringLiteral("error")).toString()));
    const QVariantMap busy_refresh = view_model.refreshRecoveryEntries();
    QVERIFY(!busy_refresh.value(QStringLiteral("valid")).toBool());
    const QVariantMap empty_export = view_model.exportRecovery(QStringLiteral("missing-entry"), {});
    QVERIFY(!empty_export.value(QStringLiteral("valid")).toBool());
    const QVariantMap empty_discard = view_model.discardRecovery(QString{});
    QVERIFY(!empty_discard.value(QStringLiteral("valid")).toBool());
    const QVariantMap unknown_discard = view_model.discardRecovery(QStringLiteral("missing-entry"));
    QVERIFY(!unknown_discard.value(QStringLiteral("valid")).toBool());
    QVERIFY(!unknown_discard.value(QStringLiteral("error")).toString().isEmpty());

    const QList<QVariant> restore_failure = wait_for_operation(operations);
    QVERIFY(!restore_failure.isEmpty());
    QVERIFY(!restore_failure.at(0).toBool());
    QVERIFY(!restore_failure.at(1).toString().isEmpty());
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const QList<QVariant> refreshed = operations.takeFirst();
    QVERIFY2(refreshed.at(0).toBool(), qPrintable(refreshed.at(1).toString()));
    QVERIFY(view_model.recoveryEntries().isEmpty());
    const QString export_path = directory.filePath(QStringLiteral("missing-export.cdoc"));
    const QVariantMap failed_export =
        view_model.exportRecovery(QStringLiteral("missing-entry"), export_path);
    QVERIFY2(failed_export.value(QStringLiteral("pending")).toBool(),
             qPrintable(failed_export.value(QStringLiteral("error")).toString()));
    const QList<QVariant> export_failure = wait_for_operation(operations);
    QVERIFY(!export_failure.isEmpty());
    QVERIFY(!export_failure.at(0).toBool());
    QVERIFY(!export_failure.at(1).toString().isEmpty());
    QVERIFY(!QFile::exists(export_path));
    QCOMPARE(view_model.paragraphLayouts().size(), 1);
    QVERIFY(view_model.paragraphLayouts()
                .first()
                .toMap()
                .value(QStringLiteral("text"))
                .toString()
                .isEmpty());
}
#endif
