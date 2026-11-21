// 验证 Rust AutoSave 策略经 Qt 单次 deadline 写入当前文档修订。
#include "shell_document_autosave_test.hpp"

#ifdef CARET_ENABLE_APP
#include "layout_view_model.hpp"
#include "preferences_view_model.hpp"
#include "shell_test_support.hpp"
#include <QFile>
#include <QIODevice>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMetaObject>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QString>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QVariantMap>
#include <QtCore/qnamespace.h>
#include <QtTest/qtestcase.h>
#include <qcontainerfwd.h>

namespace {

caret::PreferencesViewModel* preferencesOf(AppFixture& fixture) {
    if (fixture.engine == nullptr) {
        return nullptr;
    }
    return fixture.engine->singletonInstance<caret::PreferencesViewModel*>("Caret.ViewModels",
                                                                           "PreferencesViewModel");
}

// 通过标题栏开关的公开 checked 属性与信号触发一次用户切换，与既有窗口测试保持一致。
bool requestAutoSaveToggle(AppFixture& fixture, bool enabled) {
    QObject* auto_save_switch = fixture.child(QStringLiteral("autoSaveSwitch"));
    if (auto_save_switch == nullptr || !auto_save_switch->setProperty("checked", enabled)) {
        return false;
    }
    return QMetaObject::invokeMethod(auto_save_switch, "toggled", Qt::DirectConnection);
}

bool persistedAutoSaveEnabled(const AppFixture& fixture) {
    QFile settings(fixture.directory.filePath(QStringLiteral("settings.json")));
    if (!settings.open(QIODevice::ReadOnly)) {
        return false;
    }
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(settings.readAll(), &parse_error);
    return parse_error.error == QJsonParseError::NoError && document.isObject() &&
           document.object().value(QStringLiteral("autoSaveEnabled")).toBool();
}

bool writeStartupSettings(AppFixture& fixture) {
    QFile settings(fixture.directory.filePath(QStringLiteral("settings.json")));
    if (!settings.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return false;
    }
    const QByteArray json =
        QByteArrayLiteral("{\"schemaVersion\":1,\"themeId\":\"light\",\"language\":\"en\","
                          "\"windowWidth\":1280,\"windowHeight\":720,\"windowMaximized\":true,"
                          "\"caretBlinkEnabled\":true,\"autoSaveEnabled\":true,"
                          "\"recentProjectPaths\":[]}");
    return settings.write(json) == qint64(json.size()) && settings.flush();
}

} // namespace

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

void ShellDocumentAutoSaveTest::enablingAutoSaveForANamedDocumentArmsWithoutAskingForAPath() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    const QString path = fixture.directory.filePath(QStringLiteral("named-autosave.cdoc"));
    QVERIFY(fixture.saveTo(path));

    auto* preferences = preferencesOf(fixture);
    QVERIFY(preferences != nullptr);
    QVERIFY(!preferences->autoSaveEnabled());
    QObject* auto_save_switch = fixture.child(QStringLiteral("autoSaveSwitch"));
    QVERIFY(auto_save_switch != nullptr);
    QVERIFY(!auto_save_switch->property("checked").toBool());

    QVERIFY(requestAutoSaveToggle(fixture, true));

    QVERIFY(preferences->autoSaveEnabled());
    QVERIFY(auto_save_switch->property("checked").toBool());
    QObject* save_dialog = fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(save_dialog != nullptr);
    QVERIFY(!save_dialog->property("visible").toBool());
    QCOMPARE(fixture.view_model->documentPath(), path);
    QVERIFY(persistedAutoSaveEnabled(fixture));
}

void ShellDocumentAutoSaveTest::
    savingAnUnnamedDocumentOnEnableBindsThePathAndReportsEditedThenSaved() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    QVERIFY(fixture.insert(QStringLiteral("未命名的自动保存正文")));
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));
    QVERIFY(fixture.view_model->documentPath().isEmpty());

    QObject* auto_save_switch = fixture.child(QStringLiteral("autoSaveSwitch"));
    QVERIFY(auto_save_switch != nullptr);
    QVERIFY(requestAutoSaveToggle(fixture, true));
    QObject* save_dialog = fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(save_dialog != nullptr);
    QTRY_VERIFY_WITH_TIMEOUT(save_dialog->property("visible").toBool(), 10000);

    auto* preferences = preferencesOf(fixture);
    QVERIFY(preferences != nullptr);
    QVERIFY(!preferences->autoSaveEnabled());
    QVERIFY(!auto_save_switch->property("checked").toBool());

    const QString path = fixture.directory.filePath(QStringLiteral("bound-autosave.cdoc"));
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    QVERIFY(save_dialog->setProperty("selectedFile", QUrl::fromLocalFile(path)));
    QVERIFY(QMetaObject::invokeMethod(save_dialog, "accepted", Qt::DirectConnection));
    QTRY_COMPARE_WITH_TIMEOUT(operations.size(), 1, 10000);
    QVERIFY2(operations.takeFirst().at(0).toBool(), "binding the AutoSave path must succeed");
    QTRY_COMPARE_WITH_TIMEOUT(fixture.view_model->documentPath(), path, 10000);
    QTRY_VERIFY_WITH_TIMEOUT(preferences->autoSaveEnabled(), 10000);
    QTRY_VERIFY_WITH_TIMEOUT(auto_save_switch->property("checked").toBool(), 10000);
    QVERIFY(persistedAutoSaveEnabled(fixture));

    QVERIFY(fixture.insert(QStringLiteral(" 第二次编辑")));
    QCOMPARE(fixture.window->property("documentStatusLabel").toString(), QStringLiteral("Edited"));
    QTRY_VERIFY_WITH_TIMEOUT(
        [&path] {
            QFile file(path);
            if (!file.open(QIODevice::ReadOnly)) {
                return false;
            }
            return file.readAll().contains(" 第二次编辑");
        }(),
        10000);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.window->property("documentStatusLabel").toString(),
                              QStringLiteral("Saved"), 10000);
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("saved"));
    QVERIFY(!fixture.view_model->documentEdited());
}

void ShellDocumentAutoSaveTest::cancellingTheUnnamedSaveRequestLeavesAutoSaveOff() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    QVERIFY(fixture.insert(QStringLiteral("取消路径选择后仍为未命名文档")));
    QObject* auto_save_switch = fixture.child(QStringLiteral("autoSaveSwitch"));
    QVERIFY(auto_save_switch != nullptr);
    QSignalSpy operations(fixture.view_model,
                          &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());

    QVERIFY(requestAutoSaveToggle(fixture, true));
    QObject* save_dialog = fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(save_dialog != nullptr);
    QTRY_VERIFY_WITH_TIMEOUT(save_dialog->property("visible").toBool(), 10000);
    auto* preferences = preferencesOf(fixture);
    QVERIFY(preferences != nullptr);
    QVERIFY(!preferences->autoSaveEnabled());
    QVERIFY(!persistedAutoSaveEnabled(fixture));

    QVERIFY(QMetaObject::invokeMethod(save_dialog, "rejected", Qt::DirectConnection));
    QVERIFY(QMetaObject::invokeMethod(save_dialog, "close", Qt::DirectConnection));
    QTRY_VERIFY_WITH_TIMEOUT(!save_dialog->property("visible").toBool(), 10000);
    QVERIFY(!preferences->autoSaveEnabled());
    QVERIFY(!auto_save_switch->property("checked").toBool());
    QCOMPARE(operations.size(), 0);
    QVERIFY(fixture.view_model->documentPath().isEmpty());
    QVERIFY(fixture.view_model->documentEdited());
    QCOMPARE(fixture.view_model->documentSaveStatus(), QStringLiteral("edited"));
}

void ShellDocumentAutoSaveTest::startupWithAutoSaveEnabledRequestsAPathAndCancelPersistsOff() {
    AppFixture fixture;
    if (!fixture.directory.isValid() || !writeStartupSettings(fixture)) {
        QFAIL("temporary settings file is unavailable");
        return;
    }
    QString error;
    if (!fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error));
        return;
    }
    QVERIFY(waitForStartupScan(fixture));
    auto* preferences = preferencesOf(fixture);
    QVERIFY(preferences != nullptr);
    QVERIFY(preferences->autoSaveEnabled());
    QObject* auto_save_switch = fixture.child(QStringLiteral("autoSaveSwitch"));
    QVERIFY(auto_save_switch != nullptr);
    QVERIFY(auto_save_switch->property("checked").toBool());
    QVERIFY(fixture.view_model->documentPath().isEmpty());

    QObject* save_dialog = fixture.child(QStringLiteral("saveDocumentDialog"));
    QVERIFY(save_dialog != nullptr);
    QTRY_VERIFY_WITH_TIMEOUT(save_dialog->property("visible").toBool(), 10000);
    QVERIFY(QMetaObject::invokeMethod(save_dialog, "rejected", Qt::DirectConnection));
    QVERIFY(QMetaObject::invokeMethod(save_dialog, "close", Qt::DirectConnection));

    QTRY_VERIFY_WITH_TIMEOUT(!preferences->autoSaveEnabled(), 10000);
    QVERIFY(!auto_save_switch->property("checked").toBool());
    QVERIFY(fixture.view_model->documentPath().isEmpty());
    QVERIFY(!fixture.view_model->documentEdited());
    QVERIFY(!persistedAutoSaveEnabled(fixture));
}
#endif
