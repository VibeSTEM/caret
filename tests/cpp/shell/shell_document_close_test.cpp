// 验证真实应用窗口处理未保存文档时的关闭结果。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "shell_document_close_test.hpp"

#include "shell_test_support.hpp"

#ifdef CARET_ENABLE_APP
#include "layout_view_model.hpp"
#include "preferences_view_model.hpp"
#include <QDir>
#include <QFile>
#include <QIODevice>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaObject>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QString>
#include <QStringList>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QVariant>
#include <QtCore/Qt>
#include <QtCore/qdebug.h>
#include <QtCore/qobject.h>
#include <QtCore/qstringlist.h>
#include <QtCore/qvariant.h>
#include <QtGui/qtestsupport_gui.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <memory>

namespace {

bool documentContains(const QString& path, const QByteArray& expected) {
    QFile file(path);
    return file.open(QIODevice::ReadOnly) && file.readAll().contains(expected);
}

QByteArray documentBytes(const QString& path) {
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray{};
}

auto recoveryFiles(const QString& root) {
    return QDir(root + QStringLiteral("/recovery"))
        .entryList({QStringLiteral("*.json")}, QDir::Files, QDir::Name);
}

} // namespace

void ShellDocumentCloseTest::closingAnUneditedWindowClosesImmediately() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid()) {
        QFAIL("temporary directory is unavailable");
        return;
    }
    if (!fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    fixture.window->close();
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->isVisible(), 5000);
    QVERIFY(
        fixture.child(QStringLiteral("closeDocumentConfirmation")) == nullptr ||
        !fixture.child(QStringLiteral("closeDocumentConfirmation"))->property("visible").toBool());
}

void ShellDocumentCloseTest::cancellingEditedDocumentCloseKeepsChanges() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    QVERIFY(fixture.insert(QStringLiteral("keep this edit")));
    fixture.window->close();
    auto* confirmation =
        qobject_cast<QQuickWindow*>(fixture.child(QStringLiteral("closeDocumentConfirmation")));
    QVERIFY(confirmation != nullptr);
    QTRY_VERIFY(confirmation->isVisible());
    const int confirming_state = fixture.window->property("closeFlowState").toInt();
    fixture.window->close();
    QCOMPARE(fixture.window->property("closeFlowState").toInt(), confirming_state);
    QVERIFY(confirmation->isVisible());
    QObject* save = fixture.child(QStringLiteral("closeSaveButton"));
    QObject* discard = fixture.child(QStringLiteral("closeDiscardButton"));
    QObject* cancel = fixture.child(QStringLiteral("closeCancelButton"));
    QVERIFY(save != nullptr);
    QVERIFY(discard != nullptr);
    QVERIFY(cancel != nullptr);
    QCOMPARE(save->property("text").toString(), QStringLiteral("Save"));
    QCOMPARE(discard->property("text").toString(), QStringLiteral("Don't Save"));
    QCOMPARE(cancel->property("text").toString(), QStringLiteral("Cancel"));
    QVERIFY(save->property("x").toReal() < discard->property("x").toReal());
    QVERIFY(discard->property("x").toReal() < cancel->property("x").toReal());
    QVERIFY(fixture.click(QStringLiteral("closeCancelButton")));
    QTRY_VERIFY(!confirmation->isVisible());
    QCOMPARE(fixture.window->property("closeFlowState").toInt(), 0);
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("keep this edit"));
}

void ShellDocumentCloseTest::discardingEditedDocumentClosesWithoutSaving() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    const QString path = fixture.directory.filePath(QStringLiteral("discard.cdoc"));
    QVERIFY(fixture.saveTo(path));
    const QByteArray saved_baseline = documentBytes(path);
    QVERIFY(!saved_baseline.isEmpty());
    QVERIFY(fixture.insert(QStringLiteral(" discard this edit")));
    fixture.window->close();
    QVERIFY(fixture.child(QStringLiteral("closeDocumentConfirmation")) != nullptr);
    QVERIFY(fixture.click(QStringLiteral("closeDiscardButton")));
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->isVisible(), 5000);
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(documentBytes(path), saved_baseline);
}

void ShellDocumentCloseTest::savingEditedDocumentBeforeClosePersistsChanges() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    const QString path = fixture.directory.filePath(QStringLiteral("saved-by-close.cdoc"));
    QVERIFY(fixture.saveTo(path));
    QVERIFY(fixture.insert(QStringLiteral("saved by close")));
    const QString generation = fixture.view_model->documentGeneration();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    QSignalSpy save_results(fixture.view_model, &caret::LayoutViewModel::documentSaveFinished);
    QVERIFY(save_results.isValid());
    bool hidden_after_save = false;
    bool save_hide_observed = false;
    QObject save_close_observation;
    connect(fixture.window, &QWindow::visibleChanged, &save_close_observation,
            [&fixture, &hidden_after_save, &save_hide_observed, &path](bool visible) {
                if (!visible && !save_hide_observed) {
                    save_hide_observed = true;
                    hidden_after_save =
                        !fixture.view_model->documentEdited() &&
                        fixture.view_model->documentSaveStatus() == QStringLiteral("saved") &&
                        documentContains(path, "saved by close");
                }
            });
    fixture.window->close();
    QVERIFY(fixture.click(QStringLiteral("closeSaveButton")));
    QObject* titlebar_save = fixture.child(QStringLiteral("saveButton"));
    QVERIFY(titlebar_save != nullptr);
    QTRY_VERIFY(!titlebar_save->property("enabled").toBool());
    QVERIFY(QMetaObject::invokeMethod(titlebar_save, "clicked", Qt::DirectConnection));
    QTRY_COMPARE_WITH_TIMEOUT(save_results.size(), 1, 10000);
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto saved = operations.takeFirst();
    QVERIFY2(saved.at(0).toBool(), qPrintable(saved.at(1).toString()));
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->isVisible(), 10000);
    QVERIFY(!fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QCOMPARE(fixture.view_model->documentPath(), path);
    QVERIFY(documentContains(path, "saved by close"));
    QVERIFY(hidden_after_save);
}

void ShellDocumentCloseTest::unnamedDocumentSaveAsContinuesCloseAndCancelCanRetry() {
    QString error;
    {
        AppFixture save_fixture;
        if (!save_fixture.directory.isValid() ||
            !save_fixture.initialize(save_fixture.directory.path(), &error)) {
            QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                             : error));
            return;
        }
        QTRY_VERIFY_WITH_TIMEOUT(!save_fixture.view_model->documentFileBusy(), 10000);
        QVERIFY(save_fixture.insert(QStringLiteral("save this unnamed document")));
        save_fixture.window->close();
        QVERIFY(save_fixture.click(QStringLiteral("closeSaveButton")));
        QObject* save_dialog = save_fixture.child(QStringLiteral("saveDocumentDialog"));
        QVERIFY(save_dialog != nullptr);
        QTRY_VERIFY(save_dialog->property("visible").toBool());
        const QString path = save_fixture.directory.filePath(QStringLiteral("saved-as.cdoc"));
        QVERIFY(save_dialog->setProperty("selectedFile", QUrl::fromLocalFile(path)));
        QVERIFY(QMetaObject::invokeMethod(save_dialog, "accepted", Qt::DirectConnection));
        QTRY_VERIFY_WITH_TIMEOUT(!save_fixture.window->isVisible(), 10000);
        QCOMPARE(save_fixture.view_model->documentPath(), path);
        QVERIFY(documentContains(path, "save this unnamed document"));
    }

    AppFixture cancel_fixture;
    if (!cancel_fixture.directory.isValid() ||
        !cancel_fixture.initialize(cancel_fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!cancel_fixture.view_model->documentFileBusy(), 10000);
    QVERIFY(cancel_fixture.insert(QStringLiteral("keep after save-as cancel")));
    cancel_fixture.window->close();
    QVERIFY(cancel_fixture.click(QStringLiteral("closeSaveButton")));
    QObject* cancel_dialog = cancel_fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(cancel_dialog != nullptr);
    QTRY_VERIFY(cancel_dialog->property("visible").toBool());
    const int choosing_path_state = cancel_fixture.window->property("closeFlowState").toInt();
    QSignalSpy recovery_operations(cancel_fixture.view_model,
                                   &caret::LayoutViewModel::documentFileOperationFinished);
    const auto recovery_request = cancel_fixture.view_model->refreshRecoveryEntries();
    QVERIFY(recovery_request.value(QStringLiteral("valid")).toBool());
    QTRY_COMPARE_WITH_TIMEOUT(recovery_operations.size(), 1, 10000);
    QVERIFY(recovery_operations.takeFirst().at(0).toBool());
    QVERIFY(cancel_dialog->property("visible").toBool());
    QCOMPARE(cancel_fixture.window->property("closeFlowState").toInt(), choosing_path_state);
    QObject* auto_save_switch = cancel_fixture.child(QStringLiteral("autoSaveSwitch"));
    QVERIFY(auto_save_switch != nullptr);
    QVERIFY(!auto_save_switch->property("enabled").toBool());
    QVERIFY(auto_save_switch->setProperty("checked", true));
    QVERIFY(QMetaObject::invokeMethod(auto_save_switch, "toggled", Qt::DirectConnection));
    QVERIFY(cancel_dialog->property("visible").toBool());
    QCOMPARE(cancel_fixture.window->property("closeFlowState").toInt(), choosing_path_state);
    QVERIFY(QMetaObject::invokeMethod(cancel_dialog, "rejected", Qt::DirectConnection));
    QVERIFY(QMetaObject::invokeMethod(cancel_dialog, "close", Qt::DirectConnection));
    QTRY_VERIFY(!cancel_dialog->property("visible").toBool());
    QCOMPARE(cancel_fixture.window->property("closeFlowState").toInt(), 0);
    cancel_fixture.window->close();
    QObject* confirmation = cancel_fixture.child(QStringLiteral("closeDocumentConfirmation"));
    QVERIFY(confirmation != nullptr);
    QTRY_VERIFY(confirmation->property("visible").toBool());
    QVERIFY(cancel_fixture.click(QStringLiteral("closeCancelButton")));
    QVERIFY(cancel_fixture.window->isVisible());
}

void ShellDocumentCloseTest::autoSaveFlushCompletesBeforeClosing() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    const QString path = fixture.directory.filePath(QStringLiteral("autosave-close.cdoc"));
    QVERIFY(fixture.saveTo(path));
    auto* preferences = fixture.engine->singletonInstance<caret::PreferencesViewModel*>(
        "Caret.ViewModels", "PreferencesViewModel");
    QVERIFY(preferences != nullptr);
    QVERIFY(preferences->setAutoSaveEnabled(true));
    fixture.view_model->setAutoSaveEnabled(true);
    QVERIFY(fixture.insert(QStringLiteral("flushed on close")));
    const QString generation = fixture.view_model->documentGeneration();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    bool hidden_after_flush = false;
    bool flush_hide_observed = false;
    QObject flush_close_observation;
    connect(fixture.window, &QWindow::visibleChanged, &flush_close_observation,
            [&fixture, &hidden_after_flush, &flush_hide_observed, &path](bool visible) {
                if (!visible && !flush_hide_observed) {
                    flush_hide_observed = true;
                    hidden_after_flush =
                        !fixture.view_model->documentEdited() &&
                        fixture.view_model->documentSaveStatus() == QStringLiteral("saved") &&
                        documentContains(path, "flushed on close");
                }
            });
    fixture.window->close();
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto flushed = operations.takeFirst();
    QVERIFY2(flushed.at(0).toBool(), qPrintable(flushed.at(1).toString()));
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->isVisible(), 10000);
    QVERIFY(!fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QVERIFY(documentContains(path, "flushed on close"));
    QVERIFY(hidden_after_flush);
}

void ShellDocumentCloseTest::autoSaveFailureReopensCloseConfirmation() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->property("pendingRecoveryScan").toBool(), 10000);
    const QString path = fixture.directory.filePath(QStringLiteral("autosave-conflict.cdoc"));
    QVERIFY(fixture.saveTo(path));
    auto* preferences = fixture.engine->singletonInstance<caret::PreferencesViewModel*>(
        "Caret.ViewModels", "PreferencesViewModel");
    QVERIFY(preferences != nullptr);
    QVERIFY(preferences->setAutoSaveEnabled(true));
    fixture.view_model->setAutoSaveEnabled(true);
    QVERIFY(fixture.insert(QStringLiteral("local edit survives")));
    QFile external(path);
    QVERIFY(external.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(external.write("external revision"), qint64(17));
    external.close();

    const QString generation = fixture.view_model->documentGeneration();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    fixture.window->close();
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto failed = operations.takeFirst();
    QVERIFY(!failed.at(0).toBool());
    QVERIFY(failed.at(2).toBool());
    QObject* confirmation = fixture.child(QStringLiteral("closeDocumentConfirmation"));
    QVERIFY(confirmation != nullptr);
    QTRY_VERIFY(confirmation->property("visible").toBool());
    QObject* conflict = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict == nullptr || !conflict->property("visible").toBool());
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QVERIFY(documentContains(path, "external revision"));
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("local edit survives"));
}

void ShellDocumentCloseTest::saveConflictKeepsWindowAndRecoveryCopy() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    const QString path = fixture.directory.filePath(QStringLiteral("conflict-close.cdoc"));
    QVERIFY(fixture.saveTo(path));
    QVERIFY(fixture.insert(QStringLiteral("local revision")));
    const QString generation = fixture.view_model->documentGeneration();
    QFile external(path);
    QVERIFY(external.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(external.write("external revision"), qint64(17));
    external.close();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    fixture.window->close();
    QVERIFY(fixture.click(QStringLiteral("closeSaveButton")));
    QObject* conflict = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict != nullptr);
    QTRY_VERIFY(conflict->property("visible").toBool());
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto conflict_result = operations.takeFirst();
    QVERIFY(!conflict_result.at(0).toBool());
    QVERIFY(conflict_result.at(2).toBool());
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QVERIFY(documentContains(path, "external revision"));
    const QString recovery_root = fixture.directory.path();
    QTRY_VERIFY_WITH_TIMEOUT(!recoveryFiles(recovery_root).isEmpty(), 10000);
    QFile recovery(recovery_root + QStringLiteral("/recovery/") +
                   recoveryFiles(recovery_root).first());
    QVERIFY(recovery.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(recovery.readAll())
                 .object()
                 .value(QStringLiteral("kind"))
                 .toString(),
             QStringLiteral("local_document"));
    recovery.seek(0);
    const QByteArray payload = QJsonDocument::fromJson(recovery.readAll())
                                   .object()
                                   .value(QStringLiteral("payload_base64"))
                                   .toString()
                                   .toLatin1();
    QVERIFY(QByteArray::fromBase64(payload).contains("local revision"));
}

void ShellDocumentCloseTest::recoveryWriteFailureReopensCloseConfirmation() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString blocked_root = directory.filePath(QStringLiteral("recovery-blocker"));
    QFile blocker(blocked_root);
    QVERIFY(blocker.open(QIODevice::WriteOnly));
    QCOMPARE(blocker.write("file"), qint64(4));
    blocker.close();
    AppFixture fixture;
    QString error;
    if (!fixture.initialize(blocked_root, &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    const QString path = directory.filePath(QStringLiteral("recovery-failure.cdoc"));
    QVERIFY(fixture.saveTo(path));
    QVERIFY(fixture.insert(QStringLiteral("preserve local revision")));
    const QString generation = fixture.view_model->documentGeneration();
    QFile external(path);
    QVERIFY(external.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(external.write("external revision"), qint64(17));
    external.close();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    fixture.window->close();
    QVERIFY(fixture.click(QStringLiteral("closeSaveButton")));
    QObject* conflict = fixture.child(QStringLiteral("saveConflictDialog"));
    QVERIFY(conflict != nullptr);
    QTRY_VERIFY(conflict->property("visible").toBool());
    QVERIFY(conflict->property("recoveryWriteFailed").toBool());
    QObject* conflict_message = fixture.child(QStringLiteral("saveConflictMessage"));
    QVERIFY(conflict_message != nullptr);
    const QString conflict_text = conflict_message->property("text").toString();
    QVERIFY(
        conflict_text.contains(QStringLiteral("The recovery operation could not be completed.")));
    QVERIFY(!conflict_text.contains(QStringLiteral("recovery.")));
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto failure = operations.takeFirst();
    QVERIFY(!failure.at(0).toBool());
    QVERIFY(failure.at(2).toBool());
    QVERIFY(failure.at(3).toBool());
    QVERIFY(fixture.click(QStringLiteral("saveConflictCancelButton")));
    QObject* confirmation = fixture.child(QStringLiteral("closeDocumentConfirmation"));
    QVERIFY(confirmation != nullptr);
    QTRY_VERIFY(confirmation->property("visible").toBool());
    QVERIFY(fixture.window->isVisible());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QVERIFY(documentContains(path, "external revision"));
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("preserve local revision"));
}

void ShellDocumentCloseTest::closeRequestDuringFailedSaveDoesNotReopenConfirmation() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    QVERIFY(fixture.insert(QStringLiteral("preserve on failed save")));
    const QString generation = fixture.view_model->documentGeneration();
    const QString blocked_parent = fixture.directory.filePath(QStringLiteral("blocked"));
    QFile blocker(blocked_parent);
    QVERIFY(blocker.open(QIODevice::WriteOnly));
    QCOMPARE(blocker.write("file"), qint64(4));
    blocker.close();
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    const auto request =
        fixture.view_model->saveDocument(blocked_parent + QStringLiteral("/failed.cdoc"));
    QVERIFY(request.value(QStringLiteral("valid")).toBool());
    QVERIFY(fixture.view_model->documentFileBusy());
    fixture.window->close();
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    const auto failed = operations.takeFirst();
    QVERIFY(!failed.at(0).toBool());
    QVERIFY(!failed.at(1).toString().isEmpty());
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
    QObject* operation_error = fixture.child(QStringLiteral("documentOperationError"));
    QVERIFY(operation_error != nullptr);
    QTRY_VERIFY(operation_error->property("visible").toBool());
    const QString error_text = operation_error->property("text").toString();
    QVERIFY(!error_text.contains(QStringLiteral("document.")));
    QCOMPARE(error_text, QStringLiteral("The document operation could not be completed."));
    QVERIFY(fixture.window->isVisible());
    QObject* confirmation = fixture.child(QStringLiteral("closeDocumentConfirmation"));
    QVERIFY(confirmation == nullptr || !confirmation->property("visible").toBool());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));
    QCOMPARE(fixture.view_model->documentGeneration(), generation);
    QCOMPARE(fixture.view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("preserve on failed save"));
}

// 直接投递文件活动信号，隔离验证 QML 对关闭流程与后台操作的关联规则。
void ShellDocumentCloseTest::unrelatedFileFailuresDoNotCorruptCloseSaveState() {
    {
        AppFixture fixture;
        QString error;
        if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
            QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                             : error));
            return;
        }
        QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->property("pendingRecoveryScan").toBool(), 10000);
        QVERIFY(fixture.window->setProperty("closeFlowState", 7)); // 自动保存刷新中
        QVERIFY(QMetaObject::invokeMethod(
            fixture.view_model, "documentFileOperationFinished", Qt::DirectConnection,
            Q_ARG(bool, false), Q_ARG(QString, QStringLiteral("recovery.io_error: background")),
            Q_ARG(bool, false), Q_ARG(bool, false), Q_ARG(QString, QString{})));
        QCOMPARE(fixture.window->property("closeFlowState").toInt(), 7); // 自动保存刷新中
        QObject* confirmation = fixture.child(QStringLiteral("closeDocumentConfirmation"));
        QVERIFY(confirmation == nullptr || !confirmation->property("visible").toBool());
    }

    {
        AppFixture fixture;
        QString error;
        if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
            QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                             : error));
            return;
        }
        QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->property("pendingRecoveryScan").toBool(), 10000);
        QVERIFY(fixture.window->setProperty("closeFlowState", 3)); // 关闭流程保存中
        QVERIFY(QMetaObject::invokeMethod(fixture.view_model, "documentSaveFinished",
                                          Qt::DirectConnection, Q_ARG(bool, true)));
        QCOMPARE(fixture.window->property("closeFlowState").toInt(),
                 4); // 保存成功，等待文件活动结束
        QVERIFY(QMetaObject::invokeMethod(
            fixture.view_model, "documentFileOperationFinished", Qt::DirectConnection,
            Q_ARG(bool, false), Q_ARG(QString, QStringLiteral("recovery.io_error: background")),
            Q_ARG(bool, false), Q_ARG(bool, false), Q_ARG(QString, QString{})));
        QCOMPARE(fixture.window->property("closeFlowState").toInt(), 0); // 就绪
        QVERIFY(fixture.window->isVisible());
    }

    {
        AppFixture fixture;
        QString error;
        if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
            QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                             : error));
            return;
        }
        QTRY_VERIFY_WITH_TIMEOUT(!fixture.window->property("pendingRecoveryScan").toBool(), 10000);
        QVERIFY(fixture.window->setProperty("closeFlowState", 1)); // 等待正在进行的文件操作
        QVERIFY(QMetaObject::invokeMethod(
            fixture.view_model, "documentFileOperationFinished", Qt::DirectConnection,
            Q_ARG(bool, false), Q_ARG(QString, QStringLiteral("document.conflict: changed")),
            Q_ARG(bool, true), Q_ARG(bool, false), Q_ARG(QString, QString{})));
        QCOMPARE(fixture.window->property("closeFlowState").toInt(), 0); // 不保留关闭承诺
        QObject* conflict = fixture.child(QStringLiteral("saveConflictDialog"));
        QVERIFY(conflict != nullptr);
        QVERIFY(conflict->property("visible").toBool());
        QVERIFY(fixture.window->isVisible());
    }
}

#endif
