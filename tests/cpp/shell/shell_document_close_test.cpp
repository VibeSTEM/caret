// 验证真实应用窗口处理未保存文档时的关闭结果。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "application_translator.hpp"
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "preferences_view_model.hpp"
#include "settings_runtime_bridge.hpp"
#include "theme_catalog.hpp"
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
#include <QVariant>
#include <QtCore/Qt>
#include <QtCore/qdebug.h>
#include <QtCore/qobject.h>
#include <QtCore/qstringlist.h>
#include <QtCore/qvariant.h>
#include <QtGui/qtestsupport_gui.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <cstddef>
#include <memory>
#include <string_view>
#include <utility>

namespace {

class AppFixture final {
  public:
    bool initialize(const QString& recovery_root, QString* error) {
        const QByteArray settings_path =
            directory.filePath(QStringLiteral("settings.json")).toUtf8();
        auto settings = caret::ffi::new_settings_runtime(rust::String(
            settings_path.constData(), static_cast<std::size_t>(settings_path.size())));
        if (!caret::install_settings_runtime(std::move(settings))) {
            *error = QStringLiteral("settings runtime install failed");
            return false;
        }
        const rust::String theme_catalog =
            caret::ffi::settings_theme_catalog(*caret::settings_runtime());
        const QByteArray theme_bytes(theme_catalog.data(),
                                     static_cast<qsizetype>(theme_catalog.size()));
        if (!caret::install_theme_catalog(theme_bytes) ||
            !caret::install_application_translator(
                caret::prepare_application_translator(QStringLiteral("en")))) {
            *error = QStringLiteral("theme or translator setup failed");
            return false;
        }
        const QByteArray root = recovery_root.toUtf8();
        if (!caret::ffi::configure_document_recovery(
                 rust::String(root.constData(), static_cast<std::size_t>(root.size())))
                 .empty()) {
            *error = QStringLiteral("recovery storage setup failed");
            return false;
        }
        auto document = caret::ffi::new_blank_document_runtime();
        const rust::String snapshot = caret::ffi::layout_snapshot_json(*document);
        if (std::string_view(snapshot.data(), snapshot.size()).starts_with("error:") ||
            !caret::install_layout_snapshot(
                QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))) ||
            !caret::install_document_runtime(std::move(document))) {
            *error = QStringLiteral("document runtime setup failed");
            return false;
        }

        engine = std::make_unique<QQmlApplicationEngine>();
        caret::set_application_qml_engine(engine.get());
        caret::install_icon_provider(*engine);
        engine->addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
        engine->loadFromModule("Caret.Shell", "App");
        if (engine->rootObjects().size() != 1) {
            *error = engine->rootObjects().isEmpty()
                         ? QStringLiteral("App.qml did not load")
                         : QStringLiteral("unexpected app window count");
            return false;
        }
        window = qobject_cast<QQuickWindow*>(engine->rootObjects().first());
        if (window == nullptr) {
            *error = QStringLiteral("App.qml root is not a window");
            return false;
        }
        view_model = engine->singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels",
                                                                        "LayoutViewModel");
        if (view_model == nullptr) {
            *error = QStringLiteral("LayoutViewModel is unavailable");
            return false;
        }
        if (!QTest::qWaitForWindowExposed(window)) {
            *error = QStringLiteral("App window was not exposed");
            return false;
        }
        return true;
    }

    bool saveTo(const QString& path) {
        QSignalSpy operations(view_model, &caret::LayoutViewModel::documentFileOperationFinished);
        const auto request = view_model->saveDocument(path);
        if (!request.value(QStringLiteral("valid")).toBool())
            return false;
        if (operations.size() != 0 || !operations.wait(10000))
            return false;
        return operations.takeFirst().at(0).toBool();
    }

    bool insert(const QString& text) {
        return view_model->insertText(text).value(QStringLiteral("valid")).toBool();
    }

    QObject* child(const QString& name) const { return window->findChild<QObject*>(name); }

    bool click(const QString& name) const {
        QObject* button = child(name);
        return button != nullptr &&
               QMetaObject::invokeMethod(button, "clicked", Qt::DirectConnection);
    }

    QTemporaryDir directory;
    std::unique_ptr<QQmlApplicationEngine> engine;
    QQuickWindow* window = nullptr;
    caret::LayoutViewModel* view_model = nullptr;
};

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

void ShellLoadTest::closingAnUneditedWindowClosesImmediately() {
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

void ShellLoadTest::cancellingEditedDocumentCloseKeepsChanges() {
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

void ShellLoadTest::discardingEditedDocumentClosesWithoutSaving() {
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

void ShellLoadTest::savingEditedDocumentBeforeClosePersistsChanges() {
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

void ShellLoadTest::autoSaveFlushCompletesBeforeClosing() {
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

void ShellLoadTest::autoSaveFailureReopensCloseConfirmation() {
    AppFixture fixture;
    QString error;
    if (!fixture.directory.isValid() || !fixture.initialize(fixture.directory.path(), &error)) {
        QFAIL(qPrintable(error.isEmpty() ? QStringLiteral("temporary directory is unavailable")
                                         : error));
        return;
    }
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.view_model->documentFileBusy(), 10000);
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

void ShellLoadTest::saveConflictKeepsWindowAndRecoveryCopy() {
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

void ShellLoadTest::recoveryWriteFailureReopensCloseConfirmation() {
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

void ShellLoadTest::closeRequestDuringFailedSaveDoesNotReopenConfirmation() {
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

#endif
