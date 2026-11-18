// 验证 Rust AutoSave 策略经 Qt 单次 deadline 写入当前文档修订。
#include "shell_document_autosave_test.hpp"

#ifdef CARET_ENABLE_APP
#include "layout_view_model.hpp"
#include <QFile>
#include <QIODevice>
#include <QSignalSpy>
#include <QString>
#include <QTemporaryDir>
#include <QTest>
#include <QVariantMap>
#include <QtTest/qtestcase.h>
#include <qcontainerfwd.h>

void ShellDocumentAutoSaveTest::autoSaveWritesTheCurrentRevisionAfterTheDeadline() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    QVERIFY(configureDocumentRecovery(directory.path()));
    QVERIFY(installBlankDocumentRuntime());

    caret::LayoutViewModel view_model;
    QSignalSpy operation_spy(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operation_spy.isValid());
    const QString path = directory.filePath(QStringLiteral("autosave.cdoc"));
    const QVariantMap save = view_model.saveDocument(path);
    QVERIFY2(save.value(QStringLiteral("valid")).toBool(),
             qPrintable(save.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 10000);
    QVERIFY2(operation_spy.takeFirst().at(0).toBool(), "initial document save failed");

    view_model.setAutoSaveEnabled(true);
    const QVariantMap edit = view_model.insertText(QStringLiteral("自动保存快照"));
    QVERIFY2(edit.value(QStringLiteral("valid")).toBool(),
             qPrintable(edit.value(QStringLiteral("error")).toString()));

    QTRY_VERIFY_WITH_TIMEOUT(
        [&path] {
            QFile file(path);
            if (!file.open(QIODevice::ReadOnly)) {
                return false;
            }
            return file.readAll().contains("自动保存快照");
        }(),
        10000);
    QTRY_COMPARE_WITH_TIMEOUT(operation_spy.size(), 1, 5000);
    QVERIFY(operation_spy.takeFirst().at(0).toBool());
    QTRY_COMPARE_WITH_TIMEOUT(view_model.documentSaveStatus(), QStringLiteral("saved"), 5000);
}
#endif
