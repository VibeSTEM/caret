// Qt/QML Shell、设置和通用资源行为的集成测试入口。
#include "shell_load_test.hpp"

#include "document_units.hpp"
#include "icon_provider.hpp"
#include "preferences_view_model.hpp"
#include "settings_dialog_controller.hpp"
#include "theme_view_model.hpp"
#include <cstddef>
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "settings_runtime_bridge.hpp"
#endif
#include <QByteArray>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QIODevice>
#include <QImage>
#include <QMetaObject>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickWindow>
#include <QRegularExpression>
#include <QString>
#include <QTemporaryDir>
#include <QVariant>
#include <QtCore/Qt>
#include <QtGui/qtestsupport_gui.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/QTest>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <QtTest/qtestkeyboard.h>
#include <qcontainerfwd.h>
#include <utility>

Q_IMPORT_QML_PLUGIN(Caret_AdaptersPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)
Q_IMPORT_QML_PLUGIN(Caret_CanvasPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ViewModelsPlugin)

namespace {} // namespace

namespace caret::shell_test {

QVariantMap snapshot_from_theme_facade(const QString& path, const QColor& background, int body) {
    QFile file(path);
    QVariantMap tokens;
    if (!file.open(QIODevice::ReadOnly)) {
        return tokens;
    }
    const QString source = QString::fromUtf8(file.readAll());
    static const QRegularExpression declaration(
        QStringLiteral("readonly property (color|int|real|string) (\\w+)"));
    auto match = declaration.globalMatch(source);
    while (match.hasNext()) {
        const auto found = match.next();
        const QString kind = found.captured(1);
        const QString name = found.captured(2);
        if (kind == QStringLiteral("color")) {
            tokens.insert(name, QColor(QStringLiteral("#808080")));
        } else if (kind == QStringLiteral("real")) {
            tokens.insert(name, 0.5);
        } else if (kind == QStringLiteral("string")) {
            tokens.insert(name, QStringLiteral("caret"));
        } else {
            tokens.insert(name, 16);
        }
    }
    tokens.insert(QStringLiteral("colorBackground"), background);
    tokens.insert(QStringLiteral("colorPanel"), background);
    tokens.insert(QStringLiteral("colorPanelLine"), QColor(QStringLiteral("#c9c9c9")));
    tokens.insert(QStringLiteral("colorCaret"), QColor(QStringLiteral("#1c1c1c")));
    tokens.insert(QStringLiteral("caretWidthCentipixels"), 100);
    tokens.insert(QStringLiteral("fontBody"), body);
    return tokens;
}

} // namespace caret::shell_test

void ShellLoadTest::initTestCase() { QQuickStyle::setStyle(QStringLiteral("Basic")); }

void ShellLoadTest::moduleImportsAndThemeBindingTracksSnapshot() {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme, QColor(QStringLiteral("#ffffff")), 16));
    themes.insert(QStringLiteral("dark"), caret::shell_test::snapshot_from_theme_facade(
                                              theme, QColor(QStringLiteral("#111111")), 18));
    QVERIFY(themes.value(QStringLiteral("light")).contains(QStringLiteral("colorBackground")));
    caret::install_theme_snapshots(themes, QStringLiteral("light"));

    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "Item {\n"
                          "    objectName: \"probe\"\n"
                          "    property color background: Theme.colorBackground\n"
                          "    property int body: Theme.fontBody\n"
                          "    ThemedLabel { objectName: \"label\"; text: \"Caret\" }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY(probe != nullptr);
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#ffffff")));
    QCOMPARE(probe->property("body").toInt(), 16);

    auto* view_model =
        engine.singletonInstance<caret::ThemeViewModel*>("Caret.ViewModels", "ThemeViewModel");
    QVERIFY(view_model != nullptr);
    QCOMPARE(view_model->activeTheme(), QStringLiteral("light"));
    QVERIFY(view_model->availableThemes().contains(QStringLiteral("dark")));
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#ffffff")));
    delete probe;
}

void ShellLoadTest::settingsDialogDraftCancelAndFailure() {
    caret::PreferencesViewModel preferences;
    caret::SettingsDialogController controller;
    controller.setPreferences(&preferences);
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    const bool original = preferences.caretBlinkEnabled();
    controller.editCaretBlinkEnabled(!original);
    QCOMPARE(controller.draftCaretBlinkEnabled(), !original);
    QCOMPARE(preferences.caretBlinkEnabled(), original);
    controller.cancel();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    QCOMPARE(controller.draftCaretBlinkEnabled(), original);
    controller.cancel();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);

    caret::SettingsDialogController unavailable;
    unavailable.open();
    QTRY_COMPARE(unavailable.state(), caret::SettingsDialogController::Editing);
    unavailable.editCaretBlinkEnabled(false);
    unavailable.apply();
    QTRY_COMPARE(unavailable.state(), caret::SettingsDialogController::Failed);
    QVERIFY(!unavailable.draftCaretBlinkEnabled());
    QVERIFY(!unavailable.error().isEmpty());
    unavailable.editCaretBlinkEnabled(true);
    QTRY_COMPARE(unavailable.state(), caret::SettingsDialogController::Editing);
    QVERIFY(unavailable.error().isEmpty());
    unavailable.cancel();
    QTRY_COMPARE(unavailable.state(), caret::SettingsDialogController::Closed);
}

#ifdef CARET_ENABLE_APP
void ShellLoadTest::settingsDialogCommitsPreferencesThroughRust() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QByteArray path = directory.filePath(QStringLiteral("settings.json")).toUtf8();
    auto runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(runtime)));
    caret::PreferencesViewModel preferences;
    QVERIFY(preferences.error().isEmpty());
    caret::SettingsDialogController controller;
    controller.setPreferences(&preferences);
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    controller.editCaretBlinkEnabled(false);
    controller.apply();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);
    QVERIFY(!preferences.caretBlinkEnabled());
    QVERIFY(QFile::exists(QString::fromUtf8(path)));
    auto reloaded_runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(reloaded_runtime)));
    caret::PreferencesViewModel reloaded;
    QVERIFY(!reloaded.caretBlinkEnabled());
}
#endif

void ShellLoadTest::unknownIconRefusesColoring() {
    QQmlEngine engine;
    caret::install_icon_provider(engine);
    auto* provider = dynamic_cast<QQuickImageProvider*>(engine.imageProvider("caret-icons"));
    QVERIFY(provider != nullptr);
    QSize size;
    const QImage known =
        provider->requestImage(QStringLiteral("close/ff112233"), &size, QSize(16, 16));
    QVERIFY(!known.isNull());
    const QImage zoom_out =
        provider->requestImage(QStringLiteral("zoom-out/ff112233"), &size, QSize(16, 16));
    QVERIFY(!zoom_out.isNull());
    const QImage zoom_in =
        provider->requestImage(QStringLiteral("zoom-in/ff112233"), &size, QSize(16, 16));
    QVERIFY(!zoom_in.isNull());
    const QImage unknown =
        provider->requestImage(QStringLiteral("mesh-gate/ff112233"), &size, QSize(16, 16));
    QVERIFY(unknown.isNull());
}

void ShellLoadTest::documentModeSelectorTracksLocalChoice() {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme, QColor(QStringLiteral("#ffffff")), 16));
    caret::install_theme_snapshots(themes, QStringLiteral("light"));

    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.Shell\n"
                                        "DocumentModeSelector { objectName: \"modeSelector\" }\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* selector = component.create();
    QVERIFY2(selector != nullptr, qPrintable(component.errorString()));
    QQuickWindow window;
    auto* selector_item = qobject_cast<QQuickItem*>(selector);
    QVERIFY(selector_item != nullptr);
    selector_item->setParentItem(window.contentItem());
    window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("editing"));
    auto* button = selector->findChild<QObject*>(QStringLiteral("editingButton"));
    QVERIFY(button != nullptr);
    QCOMPARE(button->property("text").toString(), QStringLiteral("Editing"));

    auto* button_item = qobject_cast<QQuickItem*>(button);
    QVERIFY(button_item != nullptr);
    button_item->forceActiveFocus();
    QTest::keyClick(&window, Qt::Key_Space);
    auto* mode_menu = selector->findChild<QObject*>(QStringLiteral("documentModeMenu"));
    QVERIFY(mode_menu != nullptr);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::keyClick(&window, Qt::Key_Down);
    QTest::keyClick(&window, Qt::Key_Enter);
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("review"));
    QTest::keyClick(&window, Qt::Key_Space);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::keyClick(&window, Qt::Key_Down);
    QTest::keyClick(&window, Qt::Key_Enter);
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("view"));

    QVERIFY(QMetaObject::invokeMethod(selector, "modeSelected", Qt::DirectConnection,
                                      Q_ARG(QString, QStringLiteral("review"))));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("review"));
    QCOMPARE(button->property("text").toString(), QStringLiteral("Review"));

    QVERIFY(QMetaObject::invokeMethod(selector, "modeSelected", Qt::DirectConnection,
                                      Q_ARG(QString, QStringLiteral("view"))));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("view"));
    QCOMPARE(button->property("text").toString(), QStringLiteral("View"));
    delete selector;
}

void ShellLoadTest::newDocumentDialogHasNoProcessFields() {
    const QString root = QString::fromUtf8(CARET_SOURCE_DIR);
    QDir dialogs(root + QStringLiteral("/qml/Dialogs"));
    const QStringList names =
        dialogs.entryList(QStringList{QStringLiteral("*.qml")}, QDir::Files, QDir::Name);
    QCOMPARE(names, QStringList({QStringLiteral("LanguageSettingsPage.qml"),
                                 QStringLiteral("NewProjectDialog.qml"),
                                 QStringLiteral("PreferencesSettingsPage.qml"),
                                 QStringLiteral("SettingsDialog.qml"),
                                 QStringLiteral("UnsavedChangesDialog.qml")}));
    QFile source(dialogs.filePath(QStringLiteral("NewProjectDialog.qml")));
    QVERIFY(source.open(QIODevice::ReadOnly));
    const QString text = QString::fromUtf8(source.readAll());
    QVERIFY(!text.contains(QStringLiteral("mold")));
    QVERIFY(!text.contains(QStringLiteral("gate")));
    QVERIFY(!text.contains(QStringLiteral("material")));
    QVERIFY(!text.contains(QStringLiteral("panta")));
    QVERIFY(!text.contains(QStringLiteral("Bridge")));
    QVERIFY(text.contains(QStringLiteral("document.empty_name")));

    const QString theme = root + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme, QColor(QStringLiteral("#ffffff")), 16));
    caret::install_theme_snapshots(themes, QStringLiteral("light"));
    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.Shell\n"
                                        "NewProjectDialog { objectName: \"newDocument\" }\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* dialog = component.create();
    QVERIFY(dialog != nullptr);
    QVERIFY(!dialog->property("moldTemperature").isValid());
    QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
    QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
    QCOMPARE(dialog->property("errorCode").toString(), QStringLiteral("document.empty_name"));
    delete dialog;
}

void ShellLoadTest::unsavedChangesDialogUsesStyledActionsInRequestedOrder() {
    const QString root = QString::fromUtf8(CARET_SOURCE_DIR);
    QFile source(root + QStringLiteral("/qml/Dialogs/UnsavedChangesDialog.qml"));
    QVERIFY(source.open(QIODevice::ReadOnly));
    const QString text = QString::fromUtf8(source.readAll());
    QVERIFY(text.contains(QStringLiteral("DialogWindow")));
    QVERIFY(text.contains(QStringLiteral("closeButtonVisible: false")));
    QVERIFY(text.contains(QStringLiteral("Save your changes before closing this document?")));

    const qsizetype save = text.indexOf(QStringLiteral("objectName: \"closeSaveButton\""));
    const qsizetype discard = text.indexOf(QStringLiteral("objectName: \"closeDiscardButton\""));
    const qsizetype cancel = text.indexOf(QStringLiteral("objectName: \"closeCancelButton\""));
    QVERIFY(save >= 0);
    QVERIFY(discard > save);
    QVERIFY(cancel > discard);
    QVERIFY(text.contains(QStringLiteral("text: qsTr(\"Save\")")));
    QVERIFY(text.contains(QStringLiteral("text: qsTr(\"Don't Save\")")));
    QVERIFY(text.contains(QStringLiteral("text: qsTr(\"Cancel\")")));
}

void ShellLoadTest::closeFailureDoesNotReopenPendingConfirmation() {
    const QString root = QString::fromUtf8(CARET_SOURCE_DIR);
    QFile source(root + QStringLiteral("/qml/App.qml"));
    QVERIFY(source.open(QIODevice::ReadOnly));
    const QString text = QString::fromUtf8(source.readAll());
    const qsizetype failure_start = text.indexOf(QStringLiteral("if (!succeeded)"));
    const qsizetype success_start =
        text.indexOf(QStringLiteral("if (window.closeAfterSaveRequested"), failure_start);
    QVERIFY(failure_start >= 0);
    QVERIFY(success_start > failure_start);
    const QString failure_branch = text.mid(failure_start, success_start - failure_start);
    QVERIFY(failure_branch.contains(QStringLiteral("waitForCloseAfterFileOperation = false")));
    QVERIFY(!failure_branch.contains(QStringLiteral("resumeCloseAfterFileOperation()")));
}

void ShellLoadTest::documentUnitsConvertThroughIntegerCentipixels() {
    caret::DocumentUnits units;
    QCOMPARE(units.pointsToLogicalCentipixels(72.0, 100), qint64{9600});
    QCOMPARE(units.centipixelsToLogicalPixels(125), qreal{1.25});
    QCOMPARE(units.percentToFactor(125), qreal{1.25});
    QCOMPARE(units.factorToPercent(1.25), 125);
    QCOMPARE(units.pointsToLogicalCentipixels(-1.0, 100), qint64{0});
    QCOMPARE(units.pointsToLogicalCentipixels(12.0, -1), qint64{0});
}

int main(int argc, char* argv[]) {
    QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
    const QGuiApplication app(argc, argv);
    ShellLoadTest test;
    return QTest::qExec(&test, argc, argv);
}
