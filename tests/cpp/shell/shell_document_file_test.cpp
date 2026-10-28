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
#include <string_view>
#include <utility>

void ShellLoadTest::documentFileFlowPreservesSaveStateAndReopens() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

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
             QStringLiteral("段落追加！"));
}
#endif
