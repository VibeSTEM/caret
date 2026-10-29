// Qt/QML Shell、设置和通用资源行为的集成测试入口。
#include "shell_load_test.hpp"

#include "document_units.hpp"
#include "icon_provider.hpp"
#include "preferences_view_model.hpp"
#include "settings_dialog_controller.hpp"
#include "settings_view_model.hpp"
#include "theme_view_model.hpp"
#include <cstddef>
#ifdef CARET_ENABLE_APP
#include "application_translator.hpp"
#include "caret_ffi.h"
#include "settings_runtime_bridge.hpp"
#include "theme_catalog.hpp"
#endif
#include <QByteArray>
#include <QColor>
#include <QCoreApplication>
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
#include <QSignalSpy>
#include <QString>
#include <QTemporaryDir>
#include <QVariant>
#include <QtCore/Qt>
#include <QtCore/qtestsupport_core.h>
#include <QtGui/qtestsupport_gui.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/QTest>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <QtTest/qtestkeyboard.h>
#include <QtTest/qtestmouse.h>
#include <limits>
#include <memory>
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
    tokens.insert(QStringLiteral("colorPanel"), background);
    tokens.insert(QStringLiteral("colorPanelLine"), QColor(QStringLiteral("#c9c9c9")));
    tokens.insert(QStringLiteral("colorTransparent"), QColor(Qt::transparent));
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
    QVERIFY(themes.value(QStringLiteral("light")).contains(QStringLiteral("colorPanel")));
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
                          "    property color background: Theme.colorPanel\n"
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

void ShellLoadTest::autoScrollTextMovesOnlyWhenActiveAndOverflowing() {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme, QColor(QStringLiteral("#ffffff")), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));

    QQmlApplicationEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "Item {\n"
                          "    width: 180; height: 40\n"
                          "    CommandCandidateItem {\n"
                          "        objectName: \"candidateRow\"\n"
                          "        width: 160; height: 28\n"
                          "        candidate: ({ alias: \"\\\\subsection\", available: true })\n"
                          "        label: \"A long localized candidate label\"\n"
                          "        selected: false\n"
                          "        aliasColumnWidth: 40\n"
                          "        labelColumnWidth: 40\n"
                          "    }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY2(probe != nullptr, qPrintable(component.errorString()));
    QQuickItem* candidate_row = probe->findChild<QQuickItem*>(QStringLiteral("candidateRow"));
    QVERIFY(candidate_row != nullptr);
    QList<QQuickItem*> scroll_texts;
    for (QQuickItem* item : candidate_row->findChildren<QQuickItem*>()) {
        if (item->property("scrollActive").isValid()) {
            scroll_texts.push_back(item);
        }
    }
    QCOMPARE(scroll_texts.size(), 2);
    for (QQuickItem* text : scroll_texts) {
        QTRY_VERIFY(text->property("overflowDistance").toReal() > 0);
        QVERIFY(!text->property("shouldScroll").toBool());
    }

    QTest::qWait(250);
    for (QQuickItem* text : scroll_texts) {
        QCOMPARE(text->property("contentX").toReal(), 0.0);
    }

    QVERIFY(candidate_row->setProperty("selected", true));
    for (QQuickItem* text : scroll_texts) {
        QTRY_VERIFY(text->property("shouldScroll").toBool());
        QTRY_VERIFY_WITH_TIMEOUT(text->property("contentX").toReal() > 0, 1500);
    }

    QVERIFY(candidate_row->setProperty("selected", false));
    for (QQuickItem* text : scroll_texts) {
        QTRY_COMPARE_WITH_TIMEOUT(text->property("contentX").toReal(), 0.0, 1000);
    }
    delete probe;
}

void ShellLoadTest::autoScrollTextRepeatsWithTheDocumentedPauseCycle() {
    QQmlApplicationEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.Shell\n"
                                        "Item {\n"
                                        "    width: 500; height: 32\n"
                                        "    AutoScrollText {\n"
                                        "        id: autoText\n"
                                        "        objectName: \"autoText\"\n"
                                        "        width: naturalTextWidth - 16\n"
                                        "        height: 24\n"
                                        "        text: \"Long scrolling text\"\n"
                                        "        textColor: \"black\"\n"
                                        "        fontPixelSize: 16\n"
                                        "        scrollActive: true\n"
                                        "    }\n"
                                        "}\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY2(probe != nullptr, qPrintable(component.errorString()));
    QQuickItem* auto_text = probe->findChild<QQuickItem*>(QStringLiteral("autoText"));
    QVERIFY(auto_text != nullptr);
    QTRY_VERIFY(auto_text->property("overflowDistance").toReal() > 15.0);
    QCOMPARE(auto_text->property("pauseDurationMilliseconds").toInt(), 1000);

    const qreal overflow = auto_text->property("overflowDistance").toReal();
    QTRY_VERIFY_WITH_TIMEOUT(auto_text->property("contentX").toReal() >= overflow - 0.5, 1500);
    QTest::qWait(500);
    QVERIFY(auto_text->property("contentX").toReal() >= overflow - 0.5);
    QTRY_COMPARE_WITH_TIMEOUT(auto_text->property("contentX").toReal(), 0.0, 1500);
    QTest::qWait(500);
    QCOMPARE(auto_text->property("contentX").toReal(), 0.0);
    QTRY_VERIFY_WITH_TIMEOUT(auto_text->property("contentX").toReal() > 0.0, 1500);
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
void ShellLoadTest::preferencesWithoutRuntimeRejectChangesAndKeepDefaults() {
    if (caret::settings_runtime() != nullptr) {
        QSKIP("该用例由独立进程运行，避免依赖共享测试进程的执行顺序");
    }
    caret::PreferencesViewModel preferences;
    QVERIFY(!preferences.refresh());
    QVERIFY(!preferences.setCaretBlinkEnabled(false));
    QVERIFY(!preferences.setAutoSaveEnabled(true));
    QVERIFY(preferences.caretBlinkEnabled());
    QVERIFY(preferences.commandSuggestionsEnabled());
    QVERIFY(!preferences.autoSaveEnabled());
}

void ShellLoadTest::settingsDialogCommitsPreferencesThroughRust() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QByteArray path = directory.filePath(QStringLiteral("settings.json")).toUtf8();
    auto runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(runtime)));
    caret::PreferencesViewModel preferences;
    QVERIFY(preferences.error().isEmpty());
    caret::SettingsViewModel settings;
    QVERIFY(settings.error().isEmpty());
    caret::SettingsDialogController controller;
    controller.setPreferences(&preferences);
    controller.setSettingsObject(&settings);
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    controller.editCaretBlinkEnabled(false);
    controller.editCommandSuggestionsEnabled(false);
    controller.apply();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);
    QVERIFY(!preferences.caretBlinkEnabled());
    QVERIFY(!preferences.commandSuggestionsEnabled());
    QVERIFY(QFile::exists(QString::fromUtf8(path)));
    auto reloaded_runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(reloaded_runtime)));
    caret::PreferencesViewModel reloaded;
    QVERIFY(!reloaded.caretBlinkEnabled());
    QVERIFY(!reloaded.commandSuggestionsEnabled());
}

void ShellLoadTest::preferencesPersistAndRefreshUserFacingSwitches() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QByteArray path = directory.filePath(QStringLiteral("settings.json")).toUtf8();
    auto runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(runtime)));
    caret::PreferencesViewModel preferences;
    QVERIFY(preferences.error().isEmpty());
    QVERIFY(!preferences.autoSaveEnabled());
    QVERIFY(preferences.caretBlinkEnabled());

    QSignalSpy snapshot_changes(&preferences, &caret::PreferencesViewModel::snapshotChanged);
    QVERIFY(snapshot_changes.isValid());
    QVERIFY(preferences.setAutoSaveEnabled(true));
    QVERIFY(preferences.autoSaveEnabled());
    QCOMPARE(snapshot_changes.size(), 1);
    QVERIFY(preferences.setCaretBlinkEnabled(false));
    QVERIFY(!preferences.caretBlinkEnabled());
    QCOMPARE(snapshot_changes.size(), 2);
    QVERIFY(preferences.refresh());
    QCOMPARE(snapshot_changes.size(), 3);

    auto reloaded_runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(reloaded_runtime)));
    caret::PreferencesViewModel reloaded;
    QVERIFY(reloaded.error().isEmpty());
    QVERIFY(reloaded.autoSaveEnabled());
    QVERIFY(!reloaded.caretBlinkEnabled());
}

void ShellLoadTest::preferencesKeepTheLastSnapshotWhenPersistenceIsUnavailable() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    QFile blocker(directory.filePath(QStringLiteral("not-a-directory")));
    QVERIFY(blocker.open(QIODevice::WriteOnly));
    QCOMPARE(blocker.write(QByteArrayLiteral("file")), qint64(4));
    blocker.close();
    const QByteArray path = (blocker.fileName() + QStringLiteral("/settings.json")).toUtf8();
    auto runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(runtime)));
    caret::PreferencesViewModel preferences;
    QVERIFY(!preferences.error().isEmpty());

    QVERIFY(!preferences.setCaretBlinkEnabled(false));
    QVERIFY(preferences.caretBlinkEnabled());
    QVERIFY(!preferences.error().isEmpty());
    QVERIFY(!preferences.setAutoSaveEnabled(true));
    QVERIFY(!preferences.autoSaveEnabled());
    QVERIFY(!preferences.error().isEmpty());
    QVERIFY(preferences.refresh());
    QVERIFY(preferences.error().isEmpty());
    QVERIFY(preferences.caretBlinkEnabled());
    QVERIFY(!preferences.autoSaveEnabled());
}

void ShellLoadTest::settingsSelectionUpdatesThemeLanguageAndPersistsAtomically() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QByteArray path = directory.filePath(QStringLiteral("settings.json")).toUtf8();
    auto runtime = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(runtime)));
    const rust::String initial_theme_json =
        caret::ffi::settings_theme_catalog(*caret::settings_runtime());
    QVERIFY(caret::install_theme_catalog(
        QByteArray(initial_theme_json.data(), static_cast<qsizetype>(initial_theme_json.size()))));
    QVERIFY(caret::install_application_translator(
        caret::prepare_application_translator(QStringLiteral("en"))));

    QQmlEngine translation_engine;
    caret::set_application_qml_engine(&translation_engine);
    translation_engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent translation_component(&translation_engine);
    translation_component.setData(
        QByteArrayLiteral(
            "import QtQml\n"
            "QtObject {\n"
            "    property string settingsTitle: qsTranslate(\"SettingsDialog\", \"Settings\")\n"
            "    property string languageLabel: qsTranslate(\"SettingsLanguagePage\", "
            "\"Application language\")\n"
            "    property string cancelLabel: qsTranslate(\"DialogAction\", \"Cancel\")\n"
            "    property string untitledLabel: qsTranslate(\"WindowTitleBar\", \"Untitled\")\n"
            "    property string savingLabel: qsTranslate(\"WindowTitleBar\", \"Saving\")\n"
            "    property string editedLabel: qsTranslate(\"WindowTitleBar\", \"Edited\")\n"
            "    property string savedLabel: qsTranslate(\"WindowTitleBar\", \"Saved\")\n"
            "}\n"),
        QUrl());
    QVERIFY2(translation_component.isReady(), qPrintable(translation_component.errorString()));
    std::unique_ptr<QObject> translation_probe(translation_component.create());
    QVERIFY2(translation_probe != nullptr, qPrintable(translation_component.errorString()));
    QCOMPARE(translation_probe->property("settingsTitle").toString(), QStringLiteral("Settings"));
    QCOMPARE(translation_probe->property("languageLabel").toString(),
             QStringLiteral("Application language"));
    QCOMPARE(translation_probe->property("cancelLabel").toString(), QStringLiteral("Cancel"));
    QCOMPARE(translation_probe->property("untitledLabel").toString(), QStringLiteral("Untitled"));
    QCOMPARE(translation_probe->property("savingLabel").toString(), QStringLiteral("Saving"));
    QCOMPARE(translation_probe->property("editedLabel").toString(), QStringLiteral("Edited"));
    QCOMPARE(translation_probe->property("savedLabel").toString(), QStringLiteral("Saved"));

    QQmlComponent close_dialog_component(&translation_engine);
    close_dialog_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "UnsavedChangesDialog { objectName: \"closeDialogProbe\" }\n"),
        QUrl());
    QVERIFY2(close_dialog_component.isReady(), qPrintable(close_dialog_component.errorString()));
    std::unique_ptr<QObject> close_dialog(close_dialog_component.create());
    QVERIFY2(close_dialog != nullptr, qPrintable(close_dialog_component.errorString()));
    const qreal english_dialog_height = close_dialog->property("height").toReal();
    QObject* close_prompt = close_dialog->findChild<QObject*>(QStringLiteral("closePrompt"));
    QVERIFY(close_prompt != nullptr);
    QCOMPARE(close_prompt->property("text").toString(),
             QStringLiteral("Save your changes before closing this document?"));

    caret::SettingsViewModel settings;
    caret::ThemeViewModel theme_projection;
    QSignalSpy theme_changes(&theme_projection, &caret::ThemeViewModel::tokensChanged);
    QVERIFY(theme_changes.isValid());
    QCOMPARE(settings.availableLanguages().size(), 4);
    QVERIFY(settings.availableThemes().contains(QStringLiteral("dark")));
    QVERIFY(settings.availableThemes().contains(QStringLiteral("light")));
    QCOMPARE(settings.activeTheme(), QStringLiteral("light"));
    QCOMPARE(settings.language(), QStringLiteral("en"));
    const QVariant initial_panel_color =
        theme_projection.tokens().value(QStringLiteral("colorPanel"));

    caret::PreferencesViewModel preferences;
    caret::SettingsDialogController controller;
    controller.setPreferences(&preferences);
    controller.setSettingsObject(&settings);
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    controller.editThemeId(QStringLiteral("dark"));
    controller.editLanguage(QStringLiteral("ja"));
    controller.editCaretBlinkEnabled(false);
    controller.cancel();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);
    QCOMPARE(settings.activeTheme(), QStringLiteral("light"));
    QCOMPARE(settings.language(), QStringLiteral("en"));
    QVERIFY(preferences.caretBlinkEnabled());

    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    controller.editThemeId(QStringLiteral("dark"));
    controller.editLanguage(QStringLiteral("ja"));
    controller.editCaretBlinkEnabled(false);
    controller.apply();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Closed);
    QCOMPARE(settings.activeTheme(), QStringLiteral("dark"));
    QCOMPARE(settings.language(), QStringLiteral("ja"));
    QCOMPARE(translation_probe->property("settingsTitle").toString(), QStringLiteral("設定"));
    QCOMPARE(translation_probe->property("languageLabel").toString(),
             QStringLiteral("アプリケーションの言語"));
    QCOMPARE(translation_probe->property("cancelLabel").toString(), QStringLiteral("キャンセル"));
    QCOMPARE(translation_probe->property("untitledLabel").toString(), QStringLiteral("無題"));
    QCOMPARE(translation_probe->property("savingLabel").toString(), QStringLiteral("保存中"));
    QCOMPARE(translation_probe->property("editedLabel").toString(), QStringLiteral("編集済み"));
    QCOMPARE(translation_probe->property("savedLabel").toString(), QStringLiteral("保存済み"));
    QVERIFY(!preferences.caretBlinkEnabled());
    QCOMPARE(theme_projection.activeTheme(), QStringLiteral("dark"));
    QVERIFY(theme_projection.tokens().value(QStringLiteral("colorPanel")) != initial_panel_color);
    QVERIFY(theme_changes.count() > 0);
    QCOMPARE(QCoreApplication::translate("SettingsDialog", "Settings"), QStringLiteral("設定"));
    const caret::ffi::SettingsSnapshot committed =
        caret::ffi::settings_snapshot(*caret::settings_runtime());
    QVERIFY(committed.error.empty());
    QVERIFY(!committed.caret_blink_enabled);

    auto reopened = caret::ffi::new_settings_runtime(
        rust::String(path.constData(), static_cast<std::size_t>(path.size())));
    const caret::ffi::SettingsSnapshot restored = caret::ffi::settings_snapshot(*reopened);
    QCOMPARE(QString::fromUtf8(restored.theme_id.data(),
                               static_cast<qsizetype>(restored.theme_id.size())),
             QStringLiteral("dark"));
    QCOMPARE(QString::fromUtf8(restored.language.data(),
                               static_cast<qsizetype>(restored.language.size())),
             QStringLiteral("ja"));
    QVERIFY(!restored.caret_blink_enabled);

    QVERIFY(settings.applySettings(QStringLiteral("dark"), QStringLiteral("zh-Hans"), false, true));
    QCOMPARE(translation_probe->property("settingsTitle").toString(), QStringLiteral("设置"));
    QCOMPARE(translation_probe->property("languageLabel").toString(), QStringLiteral("应用语言"));
    QCOMPARE(translation_probe->property("cancelLabel").toString(), QStringLiteral("取消"));
    QCOMPARE(translation_probe->property("untitledLabel").toString(), QStringLiteral("未命名"));
    QCOMPARE(translation_probe->property("savingLabel").toString(), QStringLiteral("保存中"));
    QCOMPARE(translation_probe->property("editedLabel").toString(), QStringLiteral("已编辑"));
    QCOMPARE(translation_probe->property("savedLabel").toString(), QStringLiteral("已保存"));
    QCOMPARE(close_prompt->property("text").toString(), QStringLiteral("关闭文档前要保存更改吗？"));
    QVERIFY(close_dialog->property("height").toReal() < english_dialog_height);
    QVERIFY(settings.applySettings(QStringLiteral("dark"), QStringLiteral("ja"), false, true));

    const qsizetype theme_change_count = theme_changes.count();
    controller.open();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Editing);
    controller.editLanguage(QStringLiteral("fr"));
    controller.apply();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Failed);
    QCOMPARE(theme_changes.count(), theme_change_count);
    QCOMPARE(settings.language(), QStringLiteral("ja"));
    QCOMPARE(QCoreApplication::translate("SettingsDialog", "Settings"), QStringLiteral("設定"));

    controller.editLanguage(QStringLiteral("zh-Hant"));
    controller.editThemeId(QStringLiteral("unsupported"));
    controller.apply();
    QTRY_COMPARE(controller.state(), caret::SettingsDialogController::Failed);
    QCOMPARE(theme_changes.count(), theme_change_count);
    QCOMPARE(settings.language(), QStringLiteral("ja"));
    QCOMPARE(settings.activeTheme(), QStringLiteral("dark"));
    QCOMPARE(QCoreApplication::translate("SettingsDialog", "Settings"), QStringLiteral("設定"));
    QVERIFY(!controller.error().isEmpty());
    const caret::ffi::SettingsSnapshot after_failure =
        caret::ffi::settings_snapshot(*caret::settings_runtime());
    QCOMPARE(QString::fromUtf8(after_failure.theme_id.data(),
                               static_cast<qsizetype>(after_failure.theme_id.size())),
             QStringLiteral("dark"));
    QCOMPARE(QString::fromUtf8(after_failure.language.data(),
                               static_cast<qsizetype>(after_failure.language.size())),
             QStringLiteral("ja"));

    const QByteArray reset_path =
        directory.filePath(QStringLiteral("reset-settings.json")).toUtf8();
    auto reset_runtime = caret::ffi::new_settings_runtime(
        rust::String(reset_path.constData(), static_cast<std::size_t>(reset_path.size())));
    QVERIFY(caret::install_settings_runtime(std::move(reset_runtime)));
    const rust::String theme_json = caret::ffi::settings_theme_catalog(*caret::settings_runtime());
    QVERIFY(caret::install_theme_catalog(
        QByteArray(theme_json.data(), static_cast<qsizetype>(theme_json.size()))));
    QVERIFY(caret::install_application_translator(
        caret::prepare_application_translator(QStringLiteral("en"))));
    caret::set_application_qml_engine(nullptr);
}

void ShellLoadTest::dialogSizesTrackLocalizedAndOptionalContent() {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme, QColor(QStringLiteral("#ffffff")), 13));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));

    QQmlApplicationEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    caret::install_icon_provider(engine);
    caret::set_application_qml_engine(&engine);
    QVERIFY(caret::install_application_translator(
        caret::prepare_application_translator(QStringLiteral("en"))));

    QQmlComponent unsaved_component(&engine);
    unsaved_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "UnsavedChangesDialog { objectName: \"localizedCloseDialog\" }\n"),
        QUrl());
    QVERIFY2(unsaved_component.isReady(), qPrintable(unsaved_component.errorString()));
    std::unique_ptr<QObject> unsaved_dialog(unsaved_component.create());
    QVERIFY2(unsaved_dialog != nullptr, qPrintable(unsaved_component.errorString()));
    auto* unsaved_window = qobject_cast<QQuickWindow*>(unsaved_dialog.get());
    QVERIFY(unsaved_window != nullptr);
    unsaved_window->show();
    QVERIFY(QTest::qWaitForWindowExposed(unsaved_window));
    auto* prompt = unsaved_dialog->findChild<QObject*>(QStringLiteral("closePrompt"));
    auto* close_prompt_item = qobject_cast<QQuickItem*>(prompt);
    auto* close_actions_item = qobject_cast<QQuickItem*>(
        unsaved_dialog->findChild<QObject*>(QStringLiteral("closeDialogActions")));
    auto* close_content_item = qobject_cast<QQuickItem*>(
        unsaved_dialog->findChild<QObject*>(QStringLiteral("closeDialogContent")));
    QVERIFY(close_prompt_item != nullptr);
    QVERIFY(close_actions_item != nullptr);
    QVERIFY(close_content_item != nullptr);
    QVERIFY(close_prompt_item->height() > 0);
    QVERIFY(close_prompt_item->height() + 0.5 >= close_prompt_item->implicitHeight());
    QVERIFY(close_actions_item->height() > 0);
    QTRY_VERIFY(close_prompt_item->y() + close_prompt_item->height() <= close_actions_item->y());
    QTRY_VERIFY(close_actions_item->y() + close_actions_item->height() <=
                close_content_item->height());
    QTRY_VERIFY(close_content_item->height() - close_actions_item->y() -
                    close_actions_item->height() <
                close_actions_item->y() - close_prompt_item->y() - close_prompt_item->height());
    const qreal english_height = unsaved_dialog->property("height").toReal();

    QQmlComponent conflict_component(&engine);
    conflict_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "SaveConflictDialog { conflictedFileName: \"draft.cdoc\" }\n"),
        QUrl());
    QVERIFY2(conflict_component.isReady(), qPrintable(conflict_component.errorString()));
    std::unique_ptr<QObject> conflict_dialog(conflict_component.create());
    QVERIFY2(conflict_dialog != nullptr, qPrintable(conflict_component.errorString()));
    auto* conflict_window = qobject_cast<QQuickWindow*>(conflict_dialog.get());
    QVERIFY(conflict_window != nullptr);
    conflict_window->show();
    QVERIFY(QTest::qWaitForWindowExposed(conflict_window));
    auto* conflict_message_item = qobject_cast<QQuickItem*>(
        conflict_dialog->findChild<QObject*>(QStringLiteral("saveConflictMessage")));
    auto* conflict_actions_item = qobject_cast<QQuickItem*>(
        conflict_dialog->findChild<QObject*>(QStringLiteral("saveConflictActions")));
    auto* conflict_content_item = qobject_cast<QQuickItem*>(
        conflict_dialog->findChild<QObject*>(QStringLiteral("saveConflictContent")));
    QVERIFY(conflict_message_item != nullptr);
    QVERIFY(conflict_actions_item != nullptr);
    QVERIFY(conflict_content_item != nullptr);
    QVERIFY(conflict_message_item->height() > 0);
    QVERIFY(conflict_message_item->height() + 0.5 >= conflict_message_item->implicitHeight());
    QVERIFY(conflict_actions_item->height() > 0);
    QTRY_VERIFY(conflict_message_item->y() + conflict_message_item->height() <=
                conflict_actions_item->y());
    QTRY_VERIFY(conflict_actions_item->y() + conflict_actions_item->height() <=
                conflict_content_item->height());
    QTRY_VERIFY(conflict_content_item->height() - conflict_actions_item->y() -
                    conflict_actions_item->height() <
                conflict_actions_item->y() - conflict_message_item->y() -
                    conflict_message_item->height());
    const qreal concise_conflict_height = conflict_dialog->property("height").toReal();
    conflict_dialog->setProperty("recoveryWriteFailed", true);
    conflict_dialog->setProperty("recoveryErrorDetail", QString(300, QLatin1Char('x')));
    QTRY_VERIFY(conflict_dialog->property("height").toReal() > concise_conflict_height);
    QTRY_VERIFY(conflict_message_item->y() + conflict_message_item->height() <=
                conflict_actions_item->y());
    QTRY_VERIFY(conflict_actions_item->y() + conflict_actions_item->height() <=
                conflict_content_item->height());
    QTRY_VERIFY(conflict_message_item->height() + 0.5 >= conflict_message_item->implicitHeight());
    QTRY_VERIFY(conflict_content_item->height() - conflict_actions_item->y() -
                    conflict_actions_item->height() <
                conflict_actions_item->y() - conflict_message_item->y() -
                    conflict_message_item->height());

    QVERIFY(caret::install_application_translator(
        caret::prepare_application_translator(QStringLiteral("zh-Hans"))));
    QCOMPARE(prompt->property("text").toString(), QStringLiteral("关闭文档前要保存更改吗？"));
    QTRY_VERIFY(unsaved_dialog->property("height").toReal() < english_height);
    QTRY_VERIFY(close_prompt_item->height() + 0.5 >= close_prompt_item->implicitHeight());
    QTRY_VERIFY(close_prompt_item->y() + close_prompt_item->height() <= close_actions_item->y());
    QTRY_VERIFY(close_actions_item->y() + close_actions_item->height() <=
                close_content_item->height());
    QTRY_VERIFY(close_content_item->height() - close_actions_item->y() -
                    close_actions_item->height() <
                close_actions_item->y() - close_prompt_item->y() - close_prompt_item->height());

    QVERIFY(caret::install_application_translator(
        caret::prepare_application_translator(QStringLiteral("en"))));
    unsaved_window->close();
    conflict_window->close();
    caret::set_application_qml_engine(nullptr);
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
                                        "Item {\n"
                                        "    width: 800; height: 600; focus: true\n"
                                        "    property int bubbledEscapeCount: 0\n"
                                        "    Keys.onEscapePressed: event => {\n"
                                        "        bubbledEscapeCount += 1; event.accepted = true;\n"
                                        "    }\n"
                                        "    DocumentModeSelector {\n"
                                        "        objectName: \"modeSelector\"\n"
                                        "    }\n"
                                        "}\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY2(probe != nullptr, qPrintable(component.errorString()));
    QObject* selector = probe->findChild<QObject*>(QStringLiteral("modeSelector"));
    QVERIFY(selector != nullptr);
    QQuickWindow window;
    auto* probe_item = qobject_cast<QQuickItem*>(probe);
    QVERIFY(probe_item != nullptr);
    probe_item->setParentItem(window.contentItem());
    window.resize(800, 600);
    window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("editing"));
    auto* button = selector->findChild<QObject*>(QStringLiteral("documentModeButton"));
    QVERIFY(button != nullptr);
    QCOMPARE(button->property("text").toString(), QStringLiteral("Edit"));

    auto* button_item = qobject_cast<QQuickItem*>(button);
    QVERIFY(button_item != nullptr);
    const qreal button_width = button_item->width();
    auto* icon = button->findChild<QObject*>(QStringLiteral("titleBarPillButtonIcon"));
    QVERIFY(icon != nullptr);
    auto* icon_item = qobject_cast<QQuickItem*>(icon);
    QVERIFY(icon_item != nullptr);
    const qreal icon_x = icon_item->mapToItem(button_item, QPointF(0, 0)).x();
    auto* caret = button->findChild<QObject*>(QStringLiteral("titleBarPillButtonCaret"));
    QVERIFY(caret != nullptr);
    auto* caret_item = qobject_cast<QQuickItem*>(caret);
    QVERIFY(caret_item != nullptr);
    const qreal caret_x = caret_item->mapToItem(button_item, QPointF(0, 0)).x();
    const auto button_center =
        button_item->mapToScene(button_item->boundingRect().center()).toPoint();
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, button_center);
    auto* mode_menu = selector->findChild<QObject*>(QStringLiteral("documentModeMenu"));
    QVERIFY(mode_menu != nullptr);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::keyClick(&window, Qt::Key_Escape);
    QTRY_VERIFY(!mode_menu->property("visible").toBool());
    QCOMPARE(probe->property("bubbledEscapeCount").toInt(), 0);
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("editing"));
    button_item->forceActiveFocus();
    QTest::keyClick(&window, Qt::Key_Escape);
    QTRY_COMPARE(probe->property("bubbledEscapeCount").toInt(), 1);
    probe->setProperty("bubbledEscapeCount", 0);
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, button_center);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    const qreal menu_x = mode_menu->property("x").toReal();
    const qreal menu_y = mode_menu->property("y").toReal();
    QVERIFY(menu_x >= 0);
    QVERIFY(menu_y >= 0);
    QVERIFY(menu_x + mode_menu->property("width").toReal() <= window.width());
    QVERIFY(menu_y + mode_menu->property("height").toReal() <= window.height());
    auto* button_background =
        button->findChild<QObject*>(QStringLiteral("titleBarPillButtonBackground"));
    QVERIFY(button_background != nullptr);
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, button_center);
    QTRY_VERIFY(button->property("pressed").toBool());
    QCOMPARE(button_background->property("color").value<QColor>().alpha(), 0);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, button_center);
    QTRY_VERIFY(!mode_menu->property("visible").toBool());
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, button_center);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier,
                      QPoint(window.width() - 8, window.height() - 8));
    QTRY_VERIFY(!mode_menu->property("visible").toBool());

    for (int attempt = 0; attempt < 8; ++attempt) {
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, button_center);
        QTRY_VERIFY(mode_menu->property("visible").toBool());
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, button_center);
        QTRY_VERIFY(!mode_menu->property("visible").toBool());
    }

    button_item->forceActiveFocus();
    QTest::keyClick(&window, Qt::Key_Space);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::keyClick(&window, Qt::Key_Down);
    QTest::keyClick(&window, Qt::Key_Enter);
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("review"));
    QCOMPARE(button_item->width(), button_width);
    QCOMPARE(icon_item->mapToItem(button_item, QPointF(0, 0)).x(), icon_x);
    QCOMPARE(caret_item->mapToItem(button_item, QPointF(0, 0)).x(), caret_x);
    QTest::keyClick(&window, Qt::Key_Space);
    QTRY_VERIFY(mode_menu->property("visible").toBool());
    QTest::keyClick(&window, Qt::Key_Down);
    QTest::keyClick(&window, Qt::Key_Enter);
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("view"));
    QCOMPARE(button_item->width(), button_width);
    QCOMPARE(icon_item->mapToItem(button_item, QPointF(0, 0)).x(), icon_x);
    QCOMPARE(caret_item->mapToItem(button_item, QPointF(0, 0)).x(), caret_x);

    QVERIFY(QMetaObject::invokeMethod(selector, "modeSelected", Qt::DirectConnection,
                                      Q_ARG(QString, QStringLiteral("review"))));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("review"));
    QCOMPARE(button->property("text").toString(), QStringLiteral("Review"));

    QVERIFY(QMetaObject::invokeMethod(selector, "modeSelected", Qt::DirectConnection,
                                      Q_ARG(QString, QStringLiteral("view"))));
    QCOMPARE(selector->property("mode").toString(), QStringLiteral("view"));
    QCOMPARE(button->property("text").toString(), QStringLiteral("View"));
    delete probe;
}

void ShellLoadTest::newDocumentDialogHasNoProcessFields() {
    const QString root = QString::fromUtf8(CARET_SOURCE_DIR);
    QDir dialogs(root + QStringLiteral("/qml/Dialogs"));
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
    const qsizetype failure_start = text.indexOf(QStringLiteral(
        "if (!succeeded) {\n                window.waitForCloseAfterFileOperation = false;"));
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
    QCOMPARE(units.pointsToLogicalCentipixels(std::numeric_limits<qreal>::infinity(), 100),
             qint64{0});
    QCOMPARE(units.pointsToLogicalCentipixels(std::numeric_limits<qreal>::max(), 100),
             std::numeric_limits<qint64>::max());
    QCOMPARE(units.factorToPercent(std::numeric_limits<qreal>::infinity()), 0);
    QCOMPARE(units.factorToPercent(std::numeric_limits<qreal>::max()), 0);
}

int main(int argc, char* argv[]) {
    QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
    const QGuiApplication app(argc, argv);
    ShellLoadTest test;
    return QTest::qExec(&test, argc, argv);
}
