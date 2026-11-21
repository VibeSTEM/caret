// 验证桌面文档的保存状态、失败保留与异步重开流程。
#include "shell_document_file_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model.hpp"
#include "shell_test_support.hpp"
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMetaObject>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qglobal.h>
#include <QtCore/qiodevice.h>
#include <QtCore/qlist.h>
#include <QtCore/qnamespace.h>
#include <QtCore/qstring.h>
#include <QtCore/qstringlist.h>
#include <QtCore/qtestsupport_core.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <cstdint>

#ifdef CARET_ENABLE_APP
namespace {

QList<QVariant> waitForOperation(QSignalSpy& spy) {
    if (spy.isEmpty() && !spy.wait(10000)) {
        return {};
    }
    return spy.takeFirst();
}

bool writeFile(const QString& path, const QByteArray& bytes) {
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return false;
    }
    return file.write(bytes) == qint64(bytes.size()) && file.flush();
}

QByteArray readFile(const QString& path) {
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray{};
}

QVariant objectProperty(const QObject* object, const char* name) {
    return object == nullptr ? QVariant{} : object->property(name);
}

bool setObjectProperty(QObject* object, const char* name, const QVariant& value) {
    return object != nullptr && object->setProperty(name, value);
}

bool invokeObjectMethod(QObject* object, const char* method) {
    return object != nullptr && QMetaObject::invokeMethod(object, method, Qt::DirectConnection);
}

bool initializeWindow(AppFixture& fixture, QString* error) {
    if (!fixture.directory.isValid()) {
        *error = QStringLiteral("temporary directory is unavailable");
        return false;
    }
    return fixture.initialize(fixture.directory.path(), error);
}

// 让窗口进入一次由标题栏 Save 触发、且外部版本已被改写的保存冲突。
bool beginManualSaveConflict(AppFixture& fixture, const QString& path, const QString& edit,
                             const QByteArray& external_bytes, QSignalSpy& operations) {
    if (!fixture.saveTo(path) || !fixture.insert(edit) || !writeFile(path, external_bytes)) {
        return false;
    }
    // saveTo 自身的完成结果不属于本次冲突，先丢弃再触发用户保存。
    operations.clear();
    if (!fixture.click(QStringLiteral("saveButton"))) {
        return false;
    }
    QObject* conflict = fixture.child(QStringLiteral("saveConflictDialog"));
    if (conflict == nullptr) {
        return false;
    }
    for (int elapsed = 0; elapsed < 10000; elapsed += 10) {
        if (conflict->property("visible").toBool()) {
            return true;
        }
        QTest::qWait(10);
    }
    return false;
}

QJsonObject recoveryEntryWithKind(const QString& recovery_root, const char* kind) {
    const QStringList files = QDir(recovery_root + QStringLiteral("/recovery"))
                                  .entryList({QStringLiteral("*.json")}, QDir::Files, QDir::Name);
    for (const QString& file_name : files) {
        QFile file(recovery_root + QStringLiteral("/recovery/") + file_name);
        if (!file.open(QIODevice::ReadOnly)) {
            continue;
        }
        QJsonParseError parse_error;
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &parse_error);
        if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
            continue;
        }
        const QJsonObject entry = document.object();
        if (entry.value(QStringLiteral("kind")).toString() == QString::fromLatin1(kind)) {
            return entry;
        }
    }
    return {};
}

} // namespace
#endif

void ShellDocumentFileTest::documentFileFlowPreservesSaveStateAndReopens() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    QVERIFY(configureDocumentRecovery(directory.path()));
    QVERIFY(installBlankDocumentRuntime());

    caret::LayoutViewModel view_model;
    QSignalSpy operation_spy(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operation_spy.isValid());
    QSignalSpy save_completion_spy(&view_model, &caret::LayoutViewModel::documentSaveFinished);
    QVERIFY(save_completion_spy.isValid());
    QStringList observed_save_states;
    connect(&view_model, &caret::LayoutViewModel::documentFileChanged, &view_model,
            [&view_model, &observed_save_states] {
                observed_save_states.push_back(view_model.documentSaveStatus());
            });
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));

    const QVariantMap save_without_target = view_model.saveDocument();
    QVERIFY(!save_without_target.value(QStringLiteral("valid")).toBool());
    QVERIFY(save_without_target.value(QStringLiteral("needsSaveAs")).toBool());
    QCOMPARE(save_completion_spy.size(), 1);
    QVERIFY(!save_completion_spy.takeFirst().first().toBool());
    const QVariantMap open_without_path = view_model.openDocument(QString{});
    QVERIFY(!open_without_path.value(QStringLiteral("valid")).toBool());
    QCOMPARE(open_without_path.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.open_missing_path"));

    const QString missing_path = directory.filePath(QStringLiteral("missing.cdoc"));
    const QVariantMap failed_open_request = view_model.openDocument(missing_path);
    QVERIFY2(failed_open_request.value(QStringLiteral("valid")).toBool(),
             qPrintable(failed_open_request.value(QStringLiteral("error")).toString()));
    QVERIFY(view_model.documentOpening());
    const QVariantMap busy_open_request = view_model.openDocument(missing_path);
    QVERIFY(!busy_open_request.value(QStringLiteral("valid")).toBool());
    QCOMPARE(busy_open_request.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.file_operation_busy"));
    const QVariantMap busy_save_request = view_model.saveDocument(missing_path);
    QVERIFY(!busy_save_request.value(QStringLiteral("valid")).toBool());
    QCOMPARE(busy_save_request.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.file_operation_busy"));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    const QList<QVariant> failed_open = operation_spy.takeFirst();
    QVERIFY(!failed_open.at(0).toBool());
    QVERIFY(!failed_open.at(1).toString().isEmpty());
    QVERIFY(!view_model.documentOpening());
    QVERIFY(view_model.documentPath().isEmpty());

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
    const QVariantMap rejected_open = view_model.openDocument(QStringLiteral("ignored.cdoc"));
    QVERIFY(!rejected_open.value(QStringLiteral("valid")).toBool());
    QCOMPARE(rejected_open.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.unsaved_changes"));

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

void ShellDocumentFileTest::saveConflictCannotReplaceAPreviousDocumentAfterSaveAs() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    QVERIFY(configureDocumentRecovery(directory.path()));
    QVERIFY(installBlankDocumentRuntime());

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
    QVERIFY(original_conflict.at(2).toBool());
    QVERIFY(!original_conflict.at(3).toBool());
    QVERIFY(original_conflict.at(4).toString().isEmpty());
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
    QVERIFY(copy_conflict.at(2).toBool());
    QVERIFY(!copy_conflict.at(3).toBool());
    QVERIFY(!view_model.saveConflictCanReplace());

    view_model.setAutoSaveEnabled(false);
    QVERIFY(QFile::remove(copy_path));
    QVERIFY(view_model.saveDocument().value(QStringLiteral("valid")).toBool());
    const QList<QVariant> missing_conflict = wait_for_operation();
    QVERIFY(!missing_conflict.at(0).toBool());
    QVERIFY(missing_conflict.at(2).toBool());
    QVERIFY(!missing_conflict.at(3).toBool());
    QVERIFY(!view_model.saveConflictCanReplace());

    QVERIFY(configureDocumentRecovery(original_path));
    QVERIFY(view_model.saveDocument().value(QStringLiteral("valid")).toBool());
    const QList<QVariant> failed_recovery = wait_for_operation();
    QVERIFY(!failed_recovery.at(0).toBool());
    QVERIFY(failed_recovery.at(2).toBool());
    QVERIFY(failed_recovery.at(3).toBool());
    QVERIFY(!failed_recovery.at(4).toString().isEmpty());
    QVERIFY(!view_model.saveConflictCanReplace());

    const QVariantMap stale_replace = view_model.replaceExternalVersion();
    QVERIFY(!stale_replace.value(QStringLiteral("valid")).toBool());
    QCOMPARE(stale_replace.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.no_pending_conflict"));
    QFile original_file(original_path);
    QVERIFY(original_file.open(QIODevice::ReadOnly));
    QCOMPARE(original_file.readAll(), external_original);
    QVERIFY(write_external(copy_path, external_copy));
    QFile copy_file(copy_path);
    QVERIFY(copy_file.open(QIODevice::ReadOnly));
    QCOMPARE(copy_file.readAll(), external_copy);
}

void ShellDocumentFileTest::confirmedConflictReplacementWritesCurrentDocument() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    QVERIFY(configureDocumentRecovery(directory.path()));
    QVERIFY(installBlankDocumentRuntime());

    caret::LayoutViewModel view_model;
    QSignalSpy operation_spy(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operation_spy.isValid());
    const auto wait_for_operation = [&operation_spy]() {
        if (operation_spy.isEmpty() && !operation_spy.wait(10000)) {
            return QList<QVariant>{};
        }
        return operation_spy.takeFirst();
    };
    const QString path = directory.filePath(QStringLiteral("conflict.cdoc"));
    QVERIFY(view_model.insertText(QStringLiteral("local version"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model.saveDocument(path).value(QStringLiteral("valid")).toBool());
    const QList<QVariant> initial_save = wait_for_operation();
    QVERIFY(!initial_save.isEmpty());
    QVERIFY2(initial_save.at(0).toBool(), qPrintable(initial_save.at(1).toString()));

    QVERIFY(view_model.insertText(QStringLiteral(" with edits"))
                .value(QStringLiteral("valid"))
                .toBool());
    QFile external_file(path);
    QVERIFY(external_file.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QVERIFY(external_file.write("external version") > 0);
    external_file.close();
    QVERIFY(view_model.saveDocument().value(QStringLiteral("valid")).toBool());
    const QList<QVariant> conflict = wait_for_operation();
    QVERIFY(!conflict.isEmpty());
    QVERIFY(!conflict.at(0).toBool());
    QVERIFY(conflict.at(2).toBool());
    QVERIFY(view_model.saveConflictCanReplace());

    const QVariantMap replace = view_model.replaceExternalVersion();
    QVERIFY2(replace.value(QStringLiteral("valid")).toBool(),
             qPrintable(replace.value(QStringLiteral("error")).toString()));
    const QList<QVariant> replaced = wait_for_operation();
    QVERIFY(!replaced.isEmpty());
    QVERIFY2(replaced.at(0).toBool(), qPrintable(replaced.at(1).toString()));
    QVERIFY(!view_model.saveConflictCanReplace());
    QCOMPARE(view_model.documentSaveStatus(), QStringLiteral("saved"));

    auto* active_runtime = caret::document_runtime();
    QVERIFY(active_runtime != nullptr);
    const auto expected_snapshot = caret::ffi::document_save_snapshot(*active_runtime);
    QVERIFY(expected_snapshot.error.empty());
    QByteArray expected_contents;
    expected_contents.reserve(static_cast<qsizetype>(expected_snapshot.bytes.size()));
    for (const std::uint8_t byte : expected_snapshot.bytes) {
        expected_contents.append(static_cast<char>(byte));
    }

    QVERIFY(external_file.open(QIODevice::ReadOnly));
    const QByteArray saved_contents = external_file.readAll();
    QCOMPARE(saved_contents, expected_contents);
}
#endif

#ifdef CARET_ENABLE_APP
void ShellDocumentFileTest::documentHistoryUndoRedoTracksRustSession() {
    QVERIFY(installBlankDocumentRuntime());

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

void ShellDocumentFileTest::manualSaveConflictCanSaveACopyWithoutReplacingTheExternalFile() {
    AppFixture fixture;
    QString error;
    if (!initializeWindow(fixture, &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    const QString path = fixture.directory.filePath(QStringLiteral("conflict-source.cdoc"));
    const QString copy_path = fixture.directory.filePath(QStringLiteral("conflict-copy.cdoc"));
    const QByteArray external_bytes("external version");
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    if (!beginManualSaveConflict(fixture, path, QStringLiteral("local revision"), external_bytes,
                                 operations)) {
        QFAIL("manual save conflict did not reach the conflict dialog");
        return;
    }
    const QList<QVariant> conflict = waitForOperation(operations);
    QVERIFY(!conflict.isEmpty());
    QVERIFY(!conflict.at(0).toBool());
    QVERIFY(conflict.at(2).toBool());
    QObject* conflict_dialog = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict_dialog != nullptr);
    QVERIFY(objectProperty(conflict_dialog, "visible").toBool());
    QVERIFY(objectProperty(conflict_dialog, "replaceAllowed").toBool());
    QObject* conflict_message = fixture.child(QStringLiteral("saveConflictMessage"));
    QVERIFY(conflict_message != nullptr);
    QVERIFY(objectProperty(conflict_message, "text").toString().contains("conflict-source.cdoc"));

    QVERIFY(fixture.click(QStringLiteral("saveConflictCopyButton")));
    QTRY_VERIFY_WITH_TIMEOUT(!objectProperty(conflict_dialog, "visible").toBool(), 10000);
    QObject* save_dialog = fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(save_dialog != nullptr);
    QTRY_VERIFY_WITH_TIMEOUT(objectProperty(save_dialog, "visible").toBool(), 10000);
    QVERIFY(setObjectProperty(save_dialog, "selectedFile", QUrl::fromLocalFile(copy_path)));
    QVERIFY(invokeObjectMethod(save_dialog, "accepted"));
    const QList<QVariant> copied = waitForOperation(operations);
    QVERIFY(!copied.isEmpty());
    QVERIFY2(copied.at(0).toBool(), qPrintable(copied.at(1).toString()));
    QCOMPARE(fixture.view_model->documentPath(), copy_path);
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("saved"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.window->property("documentStatusLabel").toString(),
                              QStringLiteral("Saved"), 10000);
    QCOMPARE(readFile(path), external_bytes);
    QVERIFY(readFile(copy_path).contains("local revision"));
    QVERIFY(fixture.window->isVisible());
}

void ShellDocumentFileTest::manualSaveConflictReplaceBacksUpExternalBytesBeforeWriting() {
    AppFixture fixture;
    QString error;
    if (!initializeWindow(fixture, &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    const QString path = fixture.directory.filePath(QStringLiteral("conflict-replace.cdoc"));
    const QByteArray external_bytes("external bytes before replace");
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    if (!beginManualSaveConflict(fixture, path, QStringLiteral("local revision"), external_bytes,
                                 operations)) {
        QFAIL("manual save conflict did not reach the conflict dialog");
        return;
    }
    const QList<QVariant> conflict = waitForOperation(operations);
    QVERIFY(!conflict.isEmpty());
    QVERIFY(conflict.at(2).toBool());
    QObject* conflict_dialog = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict_dialog != nullptr);
    QVERIFY(objectProperty(conflict_dialog, "replaceAllowed").toBool());
    QVERIFY(fixture.click(QStringLiteral("saveConflictReplaceButton")));
    QTRY_VERIFY_WITH_TIMEOUT(!objectProperty(conflict_dialog, "visible").toBool(), 10000);
    const QList<QVariant> replaced = waitForOperation(operations);
    QVERIFY(!replaced.isEmpty());
    QVERIFY2(replaced.at(0).toBool(), qPrintable(replaced.at(1).toString()));
    QCOMPARE(fixture.view_model->documentPath(), path);
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("saved"));
    QCOMPARE(fixture.window->property("documentStatusLabel").toString(), QStringLiteral("Saved"));

    auto* active_runtime = caret::document_runtime();
    QVERIFY(active_runtime != nullptr);
    const auto expected_snapshot = caret::ffi::document_save_snapshot(*active_runtime);
    QVERIFY(expected_snapshot.error.empty());
    QByteArray expected_contents;
    expected_contents.reserve(static_cast<qsizetype>(expected_snapshot.bytes.size()));
    for (const std::uint8_t byte : expected_snapshot.bytes) {
        expected_contents.append(static_cast<char>(byte));
    }
    QCOMPARE(readFile(path), expected_contents);

    // 替换前必须先把磁盘上的外部原始字节原子写入恢复区。
    const QJsonObject external_entry =
        recoveryEntryWithKind(fixture.directory.path(), "external_version");
    QVERIFY(!external_entry.isEmpty());
    QCOMPARE(QByteArray::fromBase64(
                 external_entry.value(QStringLiteral("payload_base64")).toString().toLatin1()),
             external_bytes);
    QVERIFY(recoveryEntryWithKind(fixture.directory.path(), "local_document").isEmpty());
    QVERIFY(fixture.window->isVisible());
}

void ShellDocumentFileTest::manualSaveConflictCancelKeepsExternalFileAndLocalEdits() {
    AppFixture fixture;
    QString error;
    if (!initializeWindow(fixture, &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    const QString path = fixture.directory.filePath(QStringLiteral("conflict-cancel.cdoc"));
    const QByteArray external_bytes("external version to keep");
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    if (!beginManualSaveConflict(fixture, path, QStringLiteral("local revision"), external_bytes,
                                 operations)) {
        QFAIL("manual save conflict did not reach the conflict dialog");
        return;
    }
    const QList<QVariant> conflict = waitForOperation(operations);
    QVERIFY(!conflict.isEmpty());
    QVERIFY(conflict.at(2).toBool());
    QObject* conflict_dialog = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict_dialog != nullptr);
    QVERIFY(fixture.click(QStringLiteral("saveConflictCancelButton")));
    QTRY_VERIFY_WITH_TIMEOUT(!objectProperty(conflict_dialog, "visible").toBool(), 10000);
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));
    QCOMPARE(fixture.window->property("documentStatusLabel").toString(), QStringLiteral("Edited"));
    QCOMPARE(fixture.view_model->documentPath(), path);
    QCOMPARE(readFile(path), external_bytes);
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("local revision"));
}

void ShellDocumentFileTest::manualSaveConflictWithoutRecoveryBackupForbidsReplace() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString blocked_root = directory.filePath(QStringLiteral("recovery-blocker"));
    QVERIFY(writeFile(blocked_root, QByteArrayLiteral("file")));
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(blocked_root, &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    const QString path = directory.filePath(QStringLiteral("unbacked-conflict.cdoc"));
    const QByteArray external_bytes("external version that cannot be backed up");
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    if (!beginManualSaveConflict(fixture, path, QStringLiteral("local revision"), external_bytes,
                                 operations)) {
        QFAIL("manual save conflict did not reach the conflict dialog");
        return;
    }
    const QList<QVariant> conflict = waitForOperation(operations);
    QVERIFY(!conflict.isEmpty());
    QVERIFY(!conflict.at(0).toBool());
    QVERIFY(conflict.at(2).toBool());
    QVERIFY(conflict.at(3).toBool());
    QObject* conflict_dialog = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict_dialog != nullptr);
    QVERIFY(objectProperty(conflict_dialog, "visible").toBool());
    QVERIFY(!objectProperty(conflict_dialog, "replaceAllowed").toBool());
    QVERIFY(objectProperty(conflict_dialog, "recoveryWriteFailed").toBool());
    QObject* replace_button = fixture.child(QStringLiteral("saveConflictReplaceButton"));
    QVERIFY(replace_button != nullptr);
    QVERIFY(!objectProperty(replace_button, "visible").toBool());
    QObject* conflict_message = fixture.child(QStringLiteral("saveConflictMessage"));
    QVERIFY(conflict_message != nullptr);
    const QString message = objectProperty(conflict_message, "text").toString();
    QVERIFY(message.contains(QStringLiteral("The recovery copy could not be written:")));
    QVERIFY(!message.contains(QStringLiteral("recovery.")));
    QCOMPARE(readFile(path), external_bytes);
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));

    QVERIFY(fixture.click(QStringLiteral("saveConflictCancelButton")));
    QTRY_VERIFY_WITH_TIMEOUT(!objectProperty(conflict_dialog, "visible").toBool(), 10000);
    QVERIFY(fixture.window->isVisible());
    QCOMPARE(readFile(path), external_bytes);
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("local revision"));
}
#endif
