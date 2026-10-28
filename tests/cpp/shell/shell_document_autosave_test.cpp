// 验证 Rust AutoSave 策略经 Qt 单次 deadline 写入当前文档修订。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model.hpp"
#include <QByteArray>
#include <QFile>
#include <QIODevice>
#include <QSignalSpy>
#include <QString>
#include <QTemporaryDir>
#include <QTest>
#include <QVariantMap>
#include <QtTest/qtestcase.h>
#include <cstddef>
#include <qcontainerfwd.h>
#include <string_view>
#include <utility>

void ShellLoadTest::autoSaveWritesTheCurrentRevisionAfterTheDeadline() {
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
