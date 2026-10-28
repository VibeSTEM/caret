// 验证桌面文档的保存状态、失败保留与异步重开流程。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model.hpp"
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qglobal.h>
#include <QtCore/qiodevice.h>
#include <QtCore/qlist.h>
#include <QtCore/qstring.h>
#include <QtCore/qstringlist.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtestcase.h>
#include <cstddef>
#include <string_view>
#include <utility>

void ShellLoadTest::documentFileFlowPreservesSaveStateAndReopens() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    const QByteArray recovery_root = directory.path().toUtf8();
    const rust::String recovery_configuration = caret::ffi::configure_document_recovery(
        rust::String(recovery_root.constData(), static_cast<std::size_t>(recovery_root.size())));
    QVERIFY(recovery_configuration.empty());

    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String blank_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(blank_snapshot.data(), blank_snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(blank_snapshot.data(), static_cast<qsizetype>(blank_snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    QSignalSpy operation_spy(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operation_spy.isValid());
    QStringList observed_save_states;
    connect(&view_model, &caret::LayoutViewModel::documentFileChanged, &view_model,
            [&view_model, &observed_save_states] {
                observed_save_states.push_back(view_model.documentSaveStatus());
            });
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));

    const QVariantMap inserted = view_model.insertText(QStringLiteral("保存後も編集"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    const QVariantMap split = view_model.splitParagraph();
    QVERIFY2(split.value(QStringLiteral("valid")).toBool(),
             qPrintable(split.value(QStringLiteral("error")).toString()));
    const QVariantMap merge = view_model.deleteBackward();
    QVERIFY2(merge.value(QStringLiteral("valid")).toBool(),
             qPrintable(merge.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.paragraphLayouts().size(), 1);
    QCOMPARE(view_model.paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("保存後も編集"));
    const QVariantMap split_again = view_model.splitParagraph();
    QVERIFY2(split_again.value(QStringLiteral("valid")).toBool(),
             qPrintable(split_again.value(QStringLiteral("error")).toString()));
    const QVariantMap inserted_second_paragraph = view_model.insertText(QStringLiteral("段落"));
    QVERIFY2(inserted_second_paragraph.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted_second_paragraph.value(QStringLiteral("error")).toString()));
    const QVariantMap deleted = view_model.deleteBackward();
    QVERIFY2(deleted.value(QStringLiteral("valid")).toBool(),
             qPrintable(deleted.value(QStringLiteral("error")).toString()));
    const QVariantMap restored = view_model.insertText(QStringLiteral("落"));
    QVERIFY2(restored.value(QStringLiteral("valid")).toBool(),
             qPrintable(restored.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("edited"));

    const QString document_path = directory.filePath(QStringLiteral("roundtrip.cdoc"));
    const QVariantMap save_request = view_model.saveDocument(document_path);
    QVERIFY2(save_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(save_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> first_save = operation_spy.takeFirst();
    QVERIFY2(first_save.at(0).toBool(), qPrintable(first_save.at(1).toString()));
    QVERIFY(observed_save_states.contains(QStringLiteral("saving")));
    QVERIFY(observed_save_states.contains(QStringLiteral("edited")));
    QVERIFY(observed_save_states.contains(QStringLiteral("saved")));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));
    QCOMPARE(view_model.documentPath(), document_path);
    QCOMPARE(view_model.documentFileName(), QStringLiteral("roundtrip.cdoc"));

    const QVariantMap second_edit = view_model.insertText(QStringLiteral("追加！"));
    QVERIFY2(second_edit.value(QStringLiteral("valid")).toBool(),
             qPrintable(second_edit.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("edited"));

    const QString blocked_parent = directory.filePath(QStringLiteral("blocked"));
    QFile blocked_file(blocked_parent);
    QVERIFY(blocked_file.open(QIODevice::WriteOnly));
    blocked_file.write("file, not a directory");
    blocked_file.close();
    const QString invalid_path = blocked_parent + QStringLiteral("/roundtrip.cdoc");
    const QVariantMap failed_save_request = view_model.saveDocument(invalid_path);
    QVERIFY2(failed_save_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(failed_save_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> failed_save = operation_spy.takeFirst();
    QVERIFY(!failed_save.at(0).toBool());
    QVERIFY(!failed_save.at(1).toString().isEmpty());
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("edited"));
    QCOMPARE(view_model.paragraphLayouts().size(), 2);
    QCOMPARE(view_model.paragraphLayouts().at(0).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("保存後も編集"));
    QCOMPARE(view_model.paragraphLayouts().at(1).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("段落追加！"));
    QVERIFY(QFile::exists(document_path));

    const QVariantMap retry_save_request = view_model.saveDocument();
    QVERIFY2(retry_save_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(retry_save_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> retry_save = operation_spy.takeFirst();
    QVERIFY2(retry_save.at(0).toBool(), qPrintable(retry_save.at(1).toString()));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));

    const QVariantMap earlier_edit = view_model.insertText(QStringLiteral("旧快照"));
    QVERIFY2(earlier_edit.value(QStringLiteral("valid")).toBool(),
             qPrintable(earlier_edit.value(QStringLiteral("error")).toString()));
    const QVariantMap earlier_save_request = view_model.saveDocument(document_path);
    QVERIFY2(earlier_save_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(earlier_save_request.value(QStringLiteral("error")).toString()));
    const QVariantMap later_edit = view_model.insertText(QStringLiteral("新快照"));
    QVERIFY2(later_edit.value(QStringLiteral("valid")).toBool(),
             qPrintable(later_edit.value(QStringLiteral("error")).toString()));
    const QVariantMap later_save_request = view_model.saveDocument(document_path);
    QVERIFY2(later_save_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(later_save_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 2, 10000);
    const QList<QVariant> earlier_save = operation_spy.takeFirst();
    const QList<QVariant> later_save = operation_spy.takeFirst();
    QVERIFY2(earlier_save.at(0).toBool(), qPrintable(earlier_save.at(1).toString()));
    QVERIFY2(later_save.at(0).toBool(), qPrintable(later_save.at(1).toString()));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));

    const QString fixture_path = QString::fromUtf8(CARET_SOURCE_DIR) +
                                 QStringLiteral("/tests/fixtures/document/v1-minimal.cdoc");
    const QString alternate_path = directory.filePath(QStringLiteral("alternate.cdoc"));
    QVERIFY(QFile::copy(fixture_path, alternate_path));
    const QVariantMap alternate_open_request = view_model.openDocument(alternate_path);
    QVERIFY2(alternate_open_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(alternate_open_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> alternate_opened = operation_spy.takeFirst();
    QVERIFY2(alternate_opened.at(0).toBool(), qPrintable(alternate_opened.at(1).toString()));
    QCOMPARE(view_model.documentFileName(), QStringLiteral("alternate.cdoc"));
    QCOMPARE(view_model.paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("Caret"));

    const QVariantMap open_request = view_model.openDocument(document_path);
    QVERIFY2(open_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_request.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> opened = operation_spy.takeFirst();
    QVERIFY2(opened.at(0).toBool(), qPrintable(opened.at(1).toString()));
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));
    QCOMPARE(view_model.documentPath(), document_path);
    QCOMPARE(view_model.pageSizesPt().size(), 1);
    QCOMPARE(view_model.paragraphLayouts().size(), 2);
    QCOMPARE(view_model.paragraphLayouts().at(0).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("保存後も編集"));
    QCOMPARE(view_model.paragraphLayouts().at(1).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("段落追加！旧快照新快照"));
}

void ShellLoadTest::saveConflictCannotReplaceAPreviousDocumentAfterSaveAs() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QByteArray recovery_root = directory.path().toUtf8();
    const rust::String recovery_configuration = caret::ffi::configure_document_recovery(
        rust::String(recovery_root.constData(), static_cast<std::size_t>(recovery_root.size())));
    QVERIFY(recovery_configuration.empty());

    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String blank_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(blank_snapshot.data(), blank_snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(blank_snapshot.data(), static_cast<qsizetype>(blank_snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    QSignalSpy operation_spy(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operation_spy.isValid());
    const auto wait_for_operation = [&operation_spy]() {
        if (operation_spy.isEmpty() && !operation_spy.wait(10000)) {
            return QList<QVariant>{};
        }
        return operation_spy.takeFirst();
    };
    const auto write_external = [](const QString& path, const QByteArray& contents) {
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            return false;
        }
        return file.write(contents) == contents.size();
    };

    const QString original_path = directory.filePath(QStringLiteral("original.cdoc"));
    const QString copy_path = directory.filePath(QStringLiteral("copy.cdoc"));
    QVERIFY(view_model.insertText(QStringLiteral("document A"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model.saveDocument(original_path).value(QStringLiteral("valid")).toBool());
    const QList<QVariant> original_save = wait_for_operation();
    QVERIFY(!original_save.isEmpty());
    QVERIFY2(original_save.at(0).toBool(), qPrintable(original_save.at(1).toString()));

    QVERIFY(view_model.insertText(QStringLiteral(" local edit"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QByteArray external_original("external version A");
    QVERIFY(write_external(original_path, external_original));
    QVERIFY(view_model.saveDocument().value(QStringLiteral("valid")).toBool());
    const QList<QVariant> original_conflict = wait_for_operation();
    QVERIFY(!original_conflict.isEmpty());
    QVERIFY(!original_conflict.at(0).toBool());
    QVERIFY(original_conflict.at(1).toString().startsWith(
        QStringLiteral("document.external_conflict")));
    QVERIFY(view_model.saveConflictCanReplace());

    QVERIFY(view_model.saveDocument(copy_path).value(QStringLiteral("valid")).toBool());
    const QList<QVariant> copy_save = wait_for_operation();
    QVERIFY(!copy_save.isEmpty());
    QVERIFY2(copy_save.at(0).toBool(), qPrintable(copy_save.at(1).toString()));
    QCOMPARE(view_model.documentPath(), copy_path);
    QVERIFY(!view_model.saveConflictCanReplace());

    const QByteArray external_copy("external version B");
    QVERIFY(write_external(copy_path, external_copy));
    view_model.setAutoSaveEnabled(true);
    QVERIFY(view_model.insertText(QStringLiteral(" after copy"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QList<QVariant> copy_conflict = wait_for_operation();
    QVERIFY(!copy_conflict.isEmpty());
    QVERIFY(!copy_conflict.at(0).toBool());
    QVERIFY(
        copy_conflict.at(1).toString().startsWith(QStringLiteral("document.external_conflict")));
    QVERIFY(!view_model.saveConflictCanReplace());

    const QVariantMap stale_replace = view_model.replaceExternalVersion();
    QVERIFY(!stale_replace.value(QStringLiteral("valid")).toBool());
    QCOMPARE(stale_replace.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.no_pending_conflict"));
    QFile original_file(original_path);
    QVERIFY(original_file.open(QIODevice::ReadOnly));
    QCOMPARE(original_file.readAll(), external_original);
    QFile copy_file(copy_path);
    QVERIFY(copy_file.open(QIODevice::ReadOnly));
    QCOMPARE(copy_file.readAll(), external_copy);
}
#endif

#ifdef CARET_ENABLE_APP
void ShellLoadTest::documentHistoryUndoRedoTracksRustSession() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String blank_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(blank_snapshot.data(), blank_snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(blank_snapshot.data(), static_cast<qsizetype>(blank_snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    QVERIFY(!view_model.canUndo());
    QVERIFY(!view_model.canRedo());

    const QVariantMap inserted = view_model.insertText(QStringLiteral("history"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    QVERIFY(view_model.canUndo());

    const QVariantMap undone = view_model.undo();
    QVERIFY2(undone.value(QStringLiteral("valid")).toBool(),
             qPrintable(undone.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
             QString{});
    QVERIFY(!view_model.canUndo());
    QVERIFY(view_model.canRedo());

    const QVariantMap redone = view_model.redo();
    QVERIFY2(redone.value(QStringLiteral("valid")).toBool(),
             qPrintable(redone.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("history"));
    QVERIFY(view_model.canUndo());
    QVERIFY(!view_model.canRedo());
}
#endif
