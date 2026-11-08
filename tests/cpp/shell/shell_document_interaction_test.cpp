// Qt/QML 文档命中、输入法与编辑交互的集成测试。
#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "keyboard_modifier_policy.hpp"
#include "layout_view_model.hpp"
#include "shell_load_test.hpp"
#include "theme_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif
#include <QByteArray>
#include <QChar>
#include <QClipboard>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QImage>
#include <QInputMethodEvent>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeyEvent>
#include <QList>
#include <QMouseEvent>
#include <QObject>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickWindow>
#include <QRect>
#include <QRectF>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QSize>
#include <QString>
#include <QStyleHints>
#include <QTemporaryDir>
#include <QVariantList>
#include <QVariantMap>
#include <QWindow>
#include <Qt>
#include <QtCore/qglobal.h>
#include <QtCore/qobject.h>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qvariant.h>
#include <QtMath>
#include <QtQml/qqmlextensionplugin.h>
#include <QtTest/QTest>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <QtTest/qtestkeyboard.h>
#include <QtTest/qtestmouse.h>
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <limits>
#include <qcontainerfwd.h>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#ifdef CARET_ENABLE_APP
namespace caret {
struct LayoutViewModelTestProbe final {
    static bool installInteraction(LayoutViewModel& view_model, const QJsonObject& interaction) {
        return view_model.installInteraction(interaction);
    }
    static bool installSnapshot(LayoutViewModel& view_model, const QJsonObject& snapshot) {
        return view_model.installSnapshot(QJsonDocument(snapshot));
    }
    static bool installSnapshotJson(LayoutViewModel& view_model, const QString& snapshot_json,
                                    bool replace_document) {
        return view_model.installSnapshot(snapshot_json, replace_document);
    }
    static bool installDocumentFileStatus(LayoutViewModel& view_model, const QJsonObject& status) {
        return view_model.installDocumentFileStatus(status);
    }
    static QVariantMap installInteractionResponse(LayoutViewModel& view_model,
                                                  const QByteArray& response) {
        return view_model.installInteractionResponse(response);
    }
    static QVariantMap installEditResponse(LayoutViewModel& view_model,
                                           const QByteArray& response) {
        return view_model.installEditResponse(response);
    }
    static bool autoSaveTimerActive(const LayoutViewModel& view_model) {
        return view_model.m_auto_save_timer.isActive();
    }
    static void stopAutoSaveTimer(LayoutViewModel& view_model) {
        view_model.m_auto_save_timer.stop();
    }
};
} // namespace caret

void ShellLoadTest::mathFontFallbackDiagnosticProjectsToViewModel() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot_json = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot_json.data(), snapshot_json.size()).starts_with("error:"));
    QJsonObject snapshot =
        QJsonDocument::fromJson(
            QByteArray(snapshot_json.data(), static_cast<qsizetype>(snapshot_json.size())))
            .object();
    QVERIFY(!snapshot.isEmpty());
    snapshot.insert(
        QStringLiteral("mathFontDiagnostic"),
        QJsonObject{
            {QStringLiteral("code"), QStringLiteral("math.font.family_missing")},
            {QStringLiteral("requestedFamily"), QStringLiteral("Unavailable Math Family")}});
    snapshot.insert(
        QStringLiteral("textDiagnostics"),
        QJsonArray{QJsonObject{{QStringLiteral("nodeId"), QStringLiteral("math-test-id")},
                               {QStringLiteral("nodeKind"), QStringLiteral("math")},
                               {QStringLiteral("code"), QStringLiteral("math.glyph.missing")},
                               {QStringLiteral("detail"), QStringLiteral("U+10FFFF")}}});

    caret::LayoutViewModel view_model;
    QSignalSpy layout_changed(&view_model, &caret::LayoutViewModel::layoutChanged);
    QVERIFY(caret::LayoutViewModelTestProbe::installSnapshot(view_model, snapshot));
    QCOMPARE(view_model.mathFontDiagnostic().value(QStringLiteral("code")).toString(),
             QStringLiteral("math.font.family_missing"));
    QCOMPARE(view_model.mathFontDiagnostic().value(QStringLiteral("requestedFamily")).toString(),
             QStringLiteral("Unavailable Math Family"));
    QCOMPARE(view_model.textDiagnostics().size(), 1);
    QCOMPARE(view_model.textDiagnostics().front().toMap().value(QStringLiteral("code")).toString(),
             QStringLiteral("math.glyph.missing"));
    QJsonObject invalid = snapshot;
    invalid.insert(
        QStringLiteral("textDiagnostics"),
        QJsonArray{QJsonObject{{QStringLiteral("nodeId"), QStringLiteral("bad")},
                               {QStringLiteral("nodeKind"), QStringLiteral("other")},
                               {QStringLiteral("code"), QStringLiteral("text.glyph.missing")},
                               {QStringLiteral("detail"), QString{}}}});
    QVERIFY(!caret::LayoutViewModelTestProbe::installSnapshot(view_model, invalid));
    QCOMPARE(view_model.textDiagnostics().size(), 1);
    const auto install_partial = [&](const rust::String& response, const QJsonArray* diagnostics) {
        const QByteArray response_bytes(response.data(), static_cast<qsizetype>(response.size()));
        QJsonObject partial = QJsonDocument::fromJson(response_bytes).object();
        if (!partial.value(QStringLiteral("partial")).toBool())
            return false;
        if (diagnostics != nullptr)
            partial.insert(QStringLiteral("textDiagnostics"), *diagnostics);
        const QString partial_json =
            QString::fromUtf8(QJsonDocument(partial).toJson(QJsonDocument::Compact));
        return caret::LayoutViewModelTestProbe::installSnapshotJson(view_model, partial_json,
                                                                    false);
    };
    const QJsonArray updated_diagnostics{
        QJsonObject{{QStringLiteral("nodeId"), QStringLiteral("paragraph-test-id")},
                    {QStringLiteral("nodeKind"), QStringLiteral("paragraph")},
                    {QStringLiteral("code"), QStringLiteral("text.font.family_missing")},
                    {QStringLiteral("detail"), QStringLiteral("Unavailable Body Family")}}};
    const rust::String update_response = caret::ffi::insert_text_json(*runtime, rust::String("x"));
    QVERIFY2(install_partial(update_response, &updated_diagnostics),
             qPrintable(view_model.error()));
    QCOMPARE(view_model.textDiagnostics().size(), 1);
    QCOMPARE(view_model.textDiagnostics().front().toMap().value(QStringLiteral("code")).toString(),
             QStringLiteral("text.font.family_missing"));
    const rust::String clear_response = caret::ffi::insert_text_json(*runtime, rust::String("y"));
    const QByteArray clear_response_bytes(clear_response.data(),
                                          static_cast<qsizetype>(clear_response.size()));
    QVERIFY(!QJsonDocument::fromJson(clear_response_bytes)
                 .object()
                 .contains(QStringLiteral("textDiagnostics")));
    QVERIFY2(install_partial(clear_response, nullptr), qPrintable(view_model.error()));
    QVERIFY(view_model.textDiagnostics().isEmpty());
    QCOMPARE(layout_changed.count(), 3);
}

void ShellLoadTest::textFontWarningStatusBarTracksLayoutDiagnostics() {
    const QString theme_path =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"), caret::shell_test::snapshot_from_theme_facade(
                                               theme_path, QColor(QStringLiteral("#ffffff")), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));

    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot_json = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot_json.data(), snapshot_json.size()).starts_with("error:"));
    QJsonObject snapshot =
        QJsonDocument::fromJson(
            QByteArray(snapshot_json.data(), static_cast<qsizetype>(snapshot_json.size())))
            .object();
    snapshot.insert(QStringLiteral("textDiagnostics"),
                    QJsonArray{QJsonObject{
                        {QStringLiteral("nodeId"), QStringLiteral("paragraph-test-id")},
                        {QStringLiteral("nodeKind"), QStringLiteral("paragraph")},
                        {QStringLiteral("code"), QStringLiteral("text.font.family_missing")},
                        {QStringLiteral("detail"), QStringLiteral("Unavailable Body Family")}}});
    const QString snapshot_text =
        QString::fromUtf8(QJsonDocument(snapshot).toJson(QJsonDocument::Compact));
    QVERIFY(caret::install_layout_snapshot(snapshot_text));

    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(R"(import QtQuick
import Caret.Shell
Item { WindowStatusBar { objectName: "statusBar" } }
)",
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* root = component.create();
    QVERIFY2(root != nullptr, qPrintable(component.errorString()));
    QScopeGuard root_guard([root] { delete root; });
    QObject* warning = root->findChild<QObject*>(QStringLiteral("textFontWarningLabel"));
    QVERIFY(warning != nullptr);
    QVERIFY(warning->property("visible").toBool());
    QVERIFY(
        warning->property("text").toString().contains(QStringLiteral("Unavailable Body Family")));

    auto* view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(view_model != nullptr);
    snapshot.insert(QStringLiteral("textDiagnostics"), QJsonArray{});
    QVERIFY(caret::LayoutViewModelTestProbe::installSnapshot(*view_model, snapshot));
    QVERIFY(!warning->property("visible").toBool());
    QVERIFY(warning->property("text").toString().isEmpty());
}

void ShellLoadTest::environmentHighlightSnapshotSelectsAndClearsActiveRects() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QJsonObject root = QJsonDocument::fromJson(
                           QByteArray(snapshot.data(), static_cast<qsizetype>(snapshot.size())))
                           .object();
    QVERIFY(!root.isEmpty());
    root.insert(QStringLiteral("documentGeneration"), QStringLiteral("10"));
    const QJsonObject focus_rect{{QStringLiteral("pageIndex"), 0},
                                 {QStringLiteral("x0Pt"), 1.0},
                                 {QStringLiteral("y0Pt"), 1.0},
                                 {QStringLiteral("x1Pt"), 2.0},
                                 {QStringLiteral("y1Pt"), 2.0}};
    const QJsonObject inline_rect{{QStringLiteral("pageIndex"), 0},
                                  {QStringLiteral("x0Pt"), 3.0},
                                  {QStringLiteral("y0Pt"), 3.0},
                                  {QStringLiteral("x1Pt"), 4.0},
                                  {QStringLiteral("y1Pt"), 4.0}};
    root.insert(QStringLiteral("blockEnvironmentHighlightRects"), QJsonArray{focus_rect});
    root.insert(QStringLiteral("inlineEnvironmentHighlightRects"), QJsonArray{inline_rect});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));
    caret::LayoutViewModel inline_view_model;
    QCOMPARE(inline_view_model.environmentHighlightKind(), QStringLiteral("inline"));
    QCOMPARE(inline_view_model.environmentHighlightRects().size(), 1);
    QCOMPARE(
        inline_view_model.environmentHighlightRects().first().toMap().value(QStringLiteral("x0Pt")),
        3.0);

    const QString serialized_snapshot =
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact));
    QVERIFY(caret::LayoutViewModelTestProbe::installSnapshotJson(inline_view_model,
                                                                 serialized_snapshot, true));
    const QVariantList highlight_before_invalid_json =
        inline_view_model.environmentHighlightRects();
    const QVariantList snapshot_before_invalid_json = inline_view_model.pageLayouts();
    const QVariantMap input_context_before_invalid_json = inline_view_model.inputMethodContext();
    QCOMPARE(highlight_before_invalid_json.size(), 1);
    QVERIFY(!snapshot_before_invalid_json.isEmpty());
    QVERIFY(input_context_before_invalid_json.contains(QStringLiteral("enabled")));
    QCOMPARE(input_context_before_invalid_json.value(QStringLiteral("enabled")).toBool(), true);
    QSignalSpy invalid_snapshot_layout_changed(&inline_view_model,
                                               &caret::LayoutViewModel::layoutChanged);
    QSignalSpy invalid_snapshot_interaction_changed(&inline_view_model,
                                                    &caret::LayoutViewModel::interactionChanged);
    QVERIFY(!caret::LayoutViewModelTestProbe::installSnapshotJson(inline_view_model,
                                                                  QStringLiteral("{"), true));
    QCOMPARE(inline_view_model.environmentHighlightRects(), highlight_before_invalid_json);
    QCOMPARE(inline_view_model.pageLayouts(), snapshot_before_invalid_json);
    QCOMPARE(inline_view_model.inputMethodContext(), input_context_before_invalid_json);
    QCOMPARE(invalid_snapshot_layout_changed.size(), 0);
    QCOMPARE(invalid_snapshot_interaction_changed.size(), 0);
    QCOMPARE(inline_view_model.error(), QStringLiteral("layout.invalid_snapshot"));

    const QVariantList visible_environment_rects = inline_view_model.environmentHighlightRects();
    const QVariantList visible_pages = inline_view_model.pageLayouts();
    QSignalSpy interaction_changed(&inline_view_model, &caret::LayoutViewModel::interactionChanged);
    const QJsonObject stale_malformed_interaction{
        {QStringLiteral("documentGeneration"), QStringLiteral("9")},
        {QStringLiteral("fontGeneration"), QStringLiteral("0")},
        {QStringLiteral("selectionRects"), QStringLiteral("invalid")},
        {QStringLiteral("inputMethodContext"), QJsonArray{}},
        {QStringLiteral("caretGeometry"), false}};
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(inline_view_model,
                                                                 stale_malformed_interaction));
    QCOMPARE(inline_view_model.environmentHighlightRects(), visible_environment_rects);
    QCOMPARE(inline_view_model.pageLayouts(), visible_pages);
    QCOMPARE(interaction_changed.size(), 0);

    const QVariantMap previous_input_method_context = inline_view_model.inputMethodContext();
    const QJsonObject invalid_input_method_context{
        {QStringLiteral("surroundingText"), QStringLiteral("")},
        {QStringLiteral("cursorPosition"), 0},
        {QStringLiteral("anchorPosition"), 0},
        {QStringLiteral("enabled"), QStringLiteral("false")}};
    const QJsonObject malformed_input_context_interaction{
        {QStringLiteral("documentGeneration"), inline_view_model.documentGeneration()},
        {QStringLiteral("fontGeneration"), inline_view_model.fontGeneration()},
        {QStringLiteral("selectionRects"), QJsonArray{}},
        {QStringLiteral("inputMethodContext"), invalid_input_method_context},
        {QStringLiteral("caretGeometry"), QJsonObject{{QStringLiteral("pageIndex"), 0},
                                                      {QStringLiteral("xPt"), 0.0},
                                                      {QStringLiteral("y0Pt"), 0.0},
                                                      {QStringLiteral("y1Pt"), 1.0}}}};
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(
        inline_view_model, malformed_input_context_interaction));
    QCOMPARE(inline_view_model.environmentHighlightRects(), visible_environment_rects);
    QCOMPARE(inline_view_model.inputMethodContext(), previous_input_method_context);
    QCOMPARE(interaction_changed.size(), 0);

    QJsonObject missing_input_context_snapshot = root;
    missing_input_context_snapshot.remove(QStringLiteral("inputMethodContext"));
    QVERIFY(!caret::LayoutViewModelTestProbe::installSnapshot(inline_view_model,
                                                              missing_input_context_snapshot));
    QCOMPARE(inline_view_model.environmentHighlightRects(), visible_environment_rects);
    QCOMPARE(inline_view_model.inputMethodContext(), previous_input_method_context);
    QCOMPARE(inline_view_model.pageLayouts(), visible_pages);
    QCOMPARE(interaction_changed.size(), 0);

    const QJsonObject invalid_current_interaction{
        {QStringLiteral("documentGeneration"), inline_view_model.documentGeneration()},
        {QStringLiteral("fontGeneration"), inline_view_model.fontGeneration()},
        {QStringLiteral("selectionRects"), QStringLiteral("invalid")},
        {QStringLiteral("inputMethodContext"), QJsonObject{}},
        {QStringLiteral("caretGeometry"), QJsonObject{}}};
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(inline_view_model,
                                                                 invalid_current_interaction));
    QVERIFY(inline_view_model.environmentHighlightRects().isEmpty());
    QCOMPARE(interaction_changed.size(), 1);

    root.insert(QStringLiteral("inlineEnvironmentHighlightRects"), QJsonArray{});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    caret::LayoutViewModel block_view_model;
    QCOMPARE(block_view_model.environmentHighlightKind(), QStringLiteral("block"));
    QCOMPARE(block_view_model.environmentHighlightRects().size(), 1);

    root.insert(QStringLiteral("blockEnvironmentHighlightRects"), QJsonArray{});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    caret::LayoutViewModel empty_view_model;
    QVERIFY(empty_view_model.environmentHighlightRects().isEmpty());
    QVERIFY(empty_view_model.environmentHighlightKind().isEmpty());
}

void ShellLoadTest::commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    QQmlApplicationEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.ViewModels\n"
                                        "Item { objectName: \"commandLineProbe\" }\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY2(probe != nullptr, qPrintable(component.errorString()));
    auto* view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(view_model != nullptr);
    QSignalSpy interaction_changed(view_model, SIGNAL(interactionChanged()));
    QVERIFY(interaction_changed.isValid());

    QVariantMap wrong_thread;
    std::thread worker(
        [&]() { wrong_thread = view_model->commandLineCommand(QStringLiteral("begin")); });
    worker.join();
    QVERIFY(!wrong_thread.value(QStringLiteral("valid")).toBool());
    QCOMPARE(wrong_thread.value(QStringLiteral("error")).toString(),
             QStringLiteral("cmdline.wrong_thread"));

    const QVariantMap begin = view_model->commandLineCommand(QStringLiteral("begin"));
    QVERIFY(begin.value(QStringLiteral("valid")).toBool());
    QVERIFY(view_model->commandLine().value(QStringLiteral("active")).toBool());
    QCOMPARE(interaction_changed.size(), 1);

    const QVariantMap type =
        view_model->commandLineCommand(QStringLiteral("type"), QStringLiteral("sec"));
    QVERIFY(type.value(QStringLiteral("valid")).toBool());
    const QVariantMap command_line = view_model->commandLine();
    const QVariantList candidates = command_line.value(QStringLiteral("candidates")).toList();
    QCOMPARE(candidates.size(), 1);
    const QVariantMap candidate = candidates.first().toMap();
    QCOMPARE(candidate.value(QStringLiteral("alias")).toString(), QStringLiteral("\\section"));
    QVERIFY(!candidate.contains(QStringLiteral("label")));
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));

    const QVariantMap complete = view_model->commandLineCommand(QStringLiteral("tab"));
    QVERIFY(complete.value(QStringLiteral("valid")).toBool());
    QCOMPARE(view_model->commandLine().value(QStringLiteral("query")).toString(),
             QStringLiteral("section"));
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));

    const QVariantMap invalid = view_model->commandLineCommand(QStringLiteral("unknown"));
    QVERIFY(!invalid.value(QStringLiteral("valid")).toBool());
    QVERIFY(!invalid.value(QStringLiteral("error")).toString().isEmpty());
    QVERIFY(view_model->commandLine().value(QStringLiteral("active")).toBool());

    const QVariantMap cancel = view_model->commandLineCommand(QStringLiteral("cancel"));
    QVERIFY(cancel.value(QStringLiteral("valid")).toBool());
    QVERIFY(!view_model->commandLine().value(QStringLiteral("active")).toBool());
    QCOMPARE(interaction_changed.size(), 4);

    QVERIFY(view_model->commandLineCommand(QStringLiteral("begin"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model->commandLineCommand(QStringLiteral("type"), QStringLiteral("font"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QVariantMap open_font_picker = view_model->handleEditorKey(QStringLiteral("enter"));
    QVERIFY2(open_font_picker.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_font_picker.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model->commandLine().value(QStringLiteral("mode")).toString(),
             QStringLiteral("fontFamily"));
    QCOMPARE(view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
             QStringLiteral("\\font "));
    QVERIFY(view_model->commandLineCommand(QStringLiteral("type"), QStringLiteral("思源"))
                .value(QStringLiteral("valid"))
                .toBool());
    QCOMPARE(view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
             QStringLiteral("\\font 思源"));
    QVERIFY(view_model->commandLineCommand(QStringLiteral("cancel"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(!view_model->commandLine().value(QStringLiteral("active")).toBool());
    QCOMPARE(view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
             QString());

    QVERIFY(view_model->commandLineCommand(QStringLiteral("begin"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model->commandLineCommand(QStringLiteral("type"), QStringLiteral("newline"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QVariantMap newline = view_model->commandLineCommand(QStringLiteral("accept"));
    QVERIFY2(newline.value(QStringLiteral("valid")).toBool(),
             qPrintable(newline.value(QStringLiteral("error")).toString()));
    QVERIFY(!view_model->commandLine().value(QStringLiteral("active")).toBool());
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("1"));
    QCOMPARE(
        view_model->paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
        QStringLiteral("\n"));
    QVERIFY(view_model->commandLineCommand(QStringLiteral("begin"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model->commandLineCommand(QStringLiteral("tab"))
                .value(QStringLiteral("valid"))
                .toBool());
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("2"));
    QCOMPARE(
        view_model->paragraphLayouts().first().toMap().value(QStringLiteral("text")).toString(),
        QStringLiteral("\n\\"));
    delete probe;
}

void ShellLoadTest::cancelledInputMethodStateAllowsTheNextCommit() {
    caret::DocumentCanvas canvas;
    canvas.setInputMethodContext({{QStringLiteral("enabled"), true}});
    QSignalSpy input_method_spy(&canvas, &caret::DocumentCanvas::inputMethodEventRequested);
    QVERIFY(input_method_spy.isValid());

    QInputMethodEvent preedit(QStringLiteral("ni"), {});
    QCoreApplication::sendEvent(&canvas, &preedit);
    QCOMPARE(input_method_spy.size(), 1);
    QCOMPARE(canvas.preeditText(), QStringLiteral("ni"));

    canvas.cancelPreedit();
    const qsizetype cancellation_count = input_method_spy.size();
    QVERIFY(cancellation_count >= 2);

    QCoreApplication::processEvents();
    QInputMethodEvent fresh_commit;
    fresh_commit.setCommitString(QStringLiteral("new"));
    QCoreApplication::sendEvent(&canvas, &fresh_commit);
    QCOMPARE(input_method_spy.size(), cancellation_count + 1);
    QCOMPARE(input_method_spy.last().at(1).toString(), QStringLiteral("new"));
}

void ShellLoadTest::documentCanvasRoutesCommandKeysAndKeepsNormalEditingBehavior() {
    QQuickWindow window;
    window.resize(320, 180);
    auto* canvas = new caret::DocumentCanvas(window.contentItem());
    canvas->setSize(QSizeF(320, 180));
    canvas->setFocus(true);
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);

    QSignalSpy editor_keys(canvas, &caret::DocumentCanvas::editorKeyRequested);
    QSignalSpy text_input(canvas, &caret::DocumentCanvas::textInputRequested);
    QSignalSpy navigation(canvas, &caret::DocumentCanvas::visualNavigationRequested);
    QSignalSpy vertical_navigation(canvas, &caret::DocumentCanvas::verticalNavigationRequested);
    QSignalSpy edge_navigation(canvas, &caret::DocumentCanvas::lineEdgeNavigationRequested);
    QVERIFY(editor_keys.isValid());
    QVERIFY(text_input.isValid());
    QVERIFY(navigation.isValid());
    QVERIFY(vertical_navigation.isValid());
    QVERIFY(edge_navigation.isValid());
    QCOMPARE(canvas->inputMethodContext(), QVariantMap{});
    QVERIFY(!canvas->inputMethodQuery(Qt::ImEnabled).toBool());
    const QVariantMap input_method_context{
        {QStringLiteral("surroundingText"), QStringLiteral("caret")},
        {QStringLiteral("cursorPosition"), 5},
        {QStringLiteral("anchorPosition"), 0},
        {QStringLiteral("enabled"), true},
    };
    canvas->setInputMethodContext(input_method_context);
    QCOMPARE(canvas->inputMethodContext(), input_method_context);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImSurroundingText).toString(), QStringLiteral("caret"));
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorPosition).toInt(), 5);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImAnchorPosition).toInt(), 0);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCurrentSelection).toString(), QStringLiteral("caret"));
    QVERIFY(!canvas->inputMethodQuery(Qt::ImHints).isValid());

    canvas->setCommandMode(true);
    QTest::keyClick(&window, Qt::Key_A);
    QCOMPARE(text_input.size(), 1);
    QCOMPARE(text_input.takeFirst().at(0).toString(), QStringLiteral("a"));
    const QList<QPair<Qt::Key, QString>> command_key_cases = {
        {Qt::Key_Tab, QStringLiteral("tab")},
    };
    for (const auto& [key, expected] : command_key_cases) {
        QTest::keyClick(&window, key);
        QCOMPARE(editor_keys.size(), 1);
        QCOMPARE(editor_keys.takeFirst().at(0).toString(), expected);
    }
    QTest::keyClick(&window, Qt::Key_Space);
    QCOMPARE(text_input.size(), 1);
    QCOMPARE(text_input.takeFirst().at(0).toString(), QStringLiteral(" "));
    QTest::keyClick(&window, Qt::Key_Escape);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("escape"));
    QTest::keyClick(&window, Qt::Key_Return);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QTest::keyClick(&window, Qt::Key_Enter, Qt::ShiftModifier);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("shift-enter"));
    QTest::keyClick(&window, Qt::Key_Backspace);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTest::keyClick(&window, Qt::Key_Delete);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("delete"));
    QTest::keyClick(&window, Qt::Key_Left);
    QCOMPARE(navigation.size(), 1);
    QCOMPARE(navigation.takeFirst(), (QList<QVariant>{false, false}));
    QTest::keyClick(&window, Qt::Key_Right);
    QCOMPARE(navigation.size(), 1);
    QCOMPARE(navigation.takeFirst(), (QList<QVariant>{true, false}));
    QTest::keyClick(&window, Qt::Key_Up);
    QCOMPARE(vertical_navigation.size(), 1);
    QCOMPARE(vertical_navigation.takeFirst(), (QList<QVariant>{-1, false}));
    QTest::keyClick(&window, Qt::Key_Down);
    QCOMPARE(vertical_navigation.size(), 1);
    QCOMPARE(vertical_navigation.takeFirst(), (QList<QVariant>{1, false}));
    canvas->setCommandMode(false);
    QTest::keyClick(&window, Qt::Key_Tab);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("tab"));
    QInputMethodEvent preedit(QStringLiteral("ni"), {});
    QCoreApplication::sendEvent(canvas, &preedit);
    QCOMPARE(canvas->preeditText(), QStringLiteral("ni"));
    QTest::keyClick(&window, Qt::Key_Tab);
    QVERIFY2(editor_keys.isEmpty(), "Tab during IME preedit must not insert a document space");
    canvas->setCommandMode(true);
    QTest::keyClick(&window, Qt::Key_Left);
    QTest::keyClick(&window, Qt::Key_Backspace);
    QVERIFY2(navigation.isEmpty(), "IME preedit keys must not move the document caret");
    QVERIFY2(text_input.isEmpty(), "IME preedit keys must not commit fallback key text");
    canvas->setCommandMode(false);
    QInputMethodEvent cancel_preedit;
    QCoreApplication::sendEvent(canvas, &cancel_preedit);
    QVERIFY(canvas->preeditText().isEmpty());
    QTest::keyClick(&window, Qt::Key_A);
    QCOMPARE(text_input.size(), 1);
    QCOMPARE(text_input.takeFirst().at(0).toString(), QStringLiteral("a"));
    QTest::keyClick(&window, Qt::Key_Left);
    QCOMPARE(navigation.size(), 1);
    QCOMPARE(navigation.takeFirst(), (QList<QVariant>{false, false}));
    QTest::keyClick(&window, Qt::Key_Right, Qt::ShiftModifier);
    QCOMPARE(navigation.size(), 1);
    QCOMPARE(navigation.takeFirst(), (QList<QVariant>{true, true}));
    QTest::keyClick(&window, Qt::Key_Space, caret::keyboard::physicalControlModifier());
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("expand-structure"));
    QTest::keyClick(&window, Qt::Key_Up, caret::keyboard::structureNavigationModifier());
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("structure-previous"));
    QTest::keyClick(&window, Qt::Key_Space, caret::keyboard::physicalControlModifier());
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("expand-structure"));
    QTest::keyClick(&window, Qt::Key_Up);
    QCOMPARE(vertical_navigation.size(), 1);
    QCOMPARE(vertical_navigation.takeFirst(), (QList<QVariant>{-1, false}));
    QTest::keyClick(&window, Qt::Key_Down, Qt::ShiftModifier);
    QCOMPARE(vertical_navigation.size(), 1);
    QCOMPARE(vertical_navigation.takeFirst(), (QList<QVariant>{1, true}));
    QTest::keyClick(&window, Qt::Key_Home);
    QCOMPARE(edge_navigation.size(), 1);
    QCOMPARE(edge_navigation.takeFirst(), (QList<QVariant>{false, false}));
    QTest::keyClick(&window, Qt::Key_End, Qt::ShiftModifier);
    QCOMPARE(edge_navigation.size(), 1);
    QCOMPARE(edge_navigation.takeFirst(), (QList<QVariant>{true, true}));
    QTest::keyClick(&window, Qt::Key_Return);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QTest::keyClick(&window, Qt::Key_Enter, Qt::ShiftModifier);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("shift-enter"));
    QTest::keyClick(&window, Qt::Key_Backspace);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTest::keyClick(&window, Qt::Key_Backspace, caret::keyboard::modModifier());
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("mod-backspace"));
    QTest::keyClick(&window, Qt::Key_C, Qt::ControlModifier);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("mod-c"));
    QTest::keyClick(&window, Qt::Key_V, caret::keyboard::modModifier());
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("mod-v"));
    QKeyEvent altgr_event(QEvent::KeyPress, Qt::Key_2, Qt::ControlModifier | Qt::AltModifier,
                          QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_event);
    QCOMPARE(text_input.size(), 1);
    QCOMPARE(text_input.takeFirst().at(0).toString(), QStringLiteral("@"));
}

void ShellLoadTest::documentHitTestReturnsRustPosition() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item {\n"
                          "    property var hit: LayoutViewModel.hitTestDocument(0, 73, 73)\n"
                          "    property var miss: LayoutViewModel.hitTestDocument(-1, 73, 73)\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY(probe != nullptr);
    const QVariantMap hit = probe->property("hit").toMap();
    QVERIFY(hit.value(QStringLiteral("valid")).toBool());
    QVERIFY(!probe->property("miss").toMap().value(QStringLiteral("valid")).toBool());
    QCOMPARE(hit.value(QStringLiteral("documentGeneration")).toString(), QStringLiteral("0"));
    const QVariantMap position = hit.value(QStringLiteral("position")).toMap();
    QCOMPARE(position.value(QStringLiteral("paragraphId")).toString().isEmpty(), false);
    QCOMPARE(position.value(QStringLiteral("offsetUtf8")).toString(), QStringLiteral("0"));
    QCOMPARE(position.value(QStringLiteral("affinity")).toString(), QStringLiteral("downstream"));
    QCOMPARE(
        position.value(QStringLiteral("story")).toMap().value(QStringLiteral("kind")).toString(),
        QStringLiteral("body"));
    delete probe;

    QQmlComponent surface_component(&engine);
    surface_component.setData(QByteArrayLiteral("import QtQuick\n"
                                                "import Caret.Shell\n"
                                                "PageSurface { width: 500; height: 500 }\n"),
                              QUrl());
    QVERIFY2(surface_component.isReady(), qPrintable(surface_component.errorString()));
    auto* surface = qobject_cast<QQuickItem*>(surface_component.create());
    QVERIFY(surface != nullptr);
    auto* canvas = surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(canvas != nullptr);

    QQuickWindow window;
    window.resize(500, 500);
    surface->setParentItem(window.contentItem());
    QSignalSpy position_spy(surface, SIGNAL(documentPositionHit(QVariant)));
    QVERIFY(position_spy.isValid());
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 3000);
    const QSizeF page_size = canvas->pageSizesPt().front().toSizeF();
    const QPointF page_center(std::min(100.0, page_size.width()), 140.0);
    const QPointF content_point = canvas->documentToContent(0, page_center);
    const QVariantMap page_hit = canvas->hitTestPage(content_point);
    QCOMPARE(page_hit.value(QStringLiteral("pageIndex")).toInt(), 0);
    auto* layout_view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(layout_view_model != nullptr);
    const QVariantMap invalid_glyph_request =
        layout_view_model->rasterizePageGlyphs(-1, 1.0, 0.0, 0.0, 0, 0, 0, 255);
    QVERIFY(!invalid_glyph_request.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid_glyph_request.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_request"));
    const QVariantMap rust_hit =
        layout_view_model->hitTestDocument(page_hit.value(QStringLiteral("pageIndex")).toInt(),
                                           page_hit.value(QStringLiteral("xPt")).toReal(),
                                           page_hit.value(QStringLiteral("yPt")).toReal());
    QVERIFY2(rust_hit.value(QStringLiteral("valid")).toBool(),
             qPrintable(rust_hit.value(QStringLiteral("error")).toString()));
    const QPointF window_point = canvas->mapToItem(window.contentItem(), content_point);
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(position_spy.size(), 1, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
    const QVariantMap clicked_hit = position_spy.takeFirst().at(0).toMap();
    QVERIFY(clicked_hit.value(QStringLiteral("valid")).toBool());
    QCOMPARE(clicked_hit.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("0"));
    const qreal initial_caret_x = canvas->caretGeometry().value(QStringLiteral("xPt")).toReal();
    QSignalSpy text_input_spy(canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editor_key_spy(canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QTest::keyClick(&window, Qt::Key_X);
    QTRY_COMPARE_WITH_TIMEOUT(text_input_spy.size(), 1, 3000);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("x"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("1"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));
    QTRY_VERIFY_WITH_TIMEOUT(
        canvas->caretGeometry().value(QStringLiteral("xPt")).toReal() > initial_caret_x, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->glyphFrames().isEmpty() &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("1") &&
                                 !canvas->glyphFrames()
                                      .first()
                                      .toMap()
                                      .value(QStringLiteral("placements"))
                                      .toList()
                                      .isEmpty(),
                             3000);
    const QVariantMap updated_glyph_frame = canvas->glyphFrames().first().toMap();
    QCOMPARE(updated_glyph_frame.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("1"));
    QVERIFY(!updated_glyph_frame.value(QStringLiteral("placements")).toList().isEmpty());

    const auto rendered_glyph_has_ink = [&](qsizetype placement_index) {
        const QImage frame = window.grabWindow();
        if (frame.isNull()) {
            return false;
        }
        const QVariantMap current_frame = canvas->glyphFrames().first().toMap();
        const QVariantList images = current_frame.value(QStringLiteral("images")).toList();
        const QVariantList placements = current_frame.value(QStringLiteral("placements")).toList();
        if (images.isEmpty() || placement_index < 0 || placement_index >= placements.size()) {
            return false;
        }
        const QVariantMap placement = placements.at(placement_index).toMap();
        const quint32 image_index = placement.value(QStringLiteral("imageIndex")).toUInt();
        if (image_index >= static_cast<quint32>(images.size())) {
            return false;
        }
        const QVariantMap image = images.at(static_cast<qsizetype>(image_index)).toMap();
        const bool phase_applied = placement.value(QStringLiteral("pixelPhaseApplied")).toBool();
        const qreal phase_x =
            phase_applied ? placement.value(QStringLiteral("pixelPhaseX")).toReal() : 0.0;
        const qreal phase_y =
            phase_applied ? placement.value(QStringLiteral("pixelPhaseY")).toReal() : 0.0;
        const qreal image_scale = canvas->zoom() * 96.0 / 72.0 /
                                  current_frame.value(QStringLiteral("rasterPxPerPt")).toReal();
        const QPointF baseline = canvas->mapToItem(
            window.contentItem(),
            canvas->documentToContent(
                0, QPointF(placement.value(QStringLiteral("xPt")).toReal(),
                           placement.value(QStringLiteral("baselineYPt")).toReal())));
        const QRectF glyph_rect(
            baseline.x() + (image.value(QStringLiteral("left")).toReal() - phase_x) * image_scale,
            baseline.y() + (-image.value(QStringLiteral("top")).toReal() - phase_y) * image_scale,
            image.value(QStringLiteral("width")).toReal() * image_scale,
            image.value(QStringLiteral("height")).toReal() * image_scale);
        const qreal pixels_per_x = static_cast<qreal>(frame.width()) / window.width();
        const qreal pixels_per_y = static_cast<qreal>(frame.height()) / window.height();
        const QRect pixel_rect(
            qFloor(glyph_rect.left() * pixels_per_x), qFloor(glyph_rect.top() * pixels_per_y),
            qCeil(glyph_rect.width() * pixels_per_x), qCeil(glyph_rect.height() * pixels_per_y));
        const QRect clipped = pixel_rect.intersected(frame.rect());
        if (clipped.isEmpty()) {
            return false;
        }
        for (int y = clipped.top(); y <= clipped.bottom(); ++y) {
            for (int x = clipped.left(); x <= clipped.right(); ++x) {
                if (frame.pixelColor(x, y).lightness() < 240) {
                    return true;
                }
            }
        }
        return false;
    };
    QTRY_VERIFY_WITH_TIMEOUT(rendered_glyph_has_ink(0), 3000);

    const qreal latin_baseline_before_fallback =
        updated_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    const QVariantMap fullwidth_insert = layout_view_model->insertText(QStringLiteral("！"));
    QVERIFY2(fullwidth_insert.value(QStringLiteral("valid")).toBool(),
             qPrintable(fullwidth_insert.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("2"), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->glyphFrames().isEmpty() &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("2") &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("placements"))
                                         .toList()
                                         .size() >= 2,
                             3000);
    const QVariantMap mixed_glyph_frame = canvas->glyphFrames().first().toMap();
    const qreal latin_baseline_after_fallback =
        mixed_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    QCOMPARE(latin_baseline_after_fallback, latin_baseline_before_fallback);
    QTRY_VERIFY_WITH_TIMEOUT(rendered_glyph_has_ink(1), 3000);

    QTest::keyClick(&window, Qt::Key_Return);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("3"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts().size(), 2);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(0)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());

    const qulonglong composition_revision = layout_view_model->layoutRevision();
    QList<QInputMethodEvent::Attribute> composition_attributes;
    composition_attributes.push_back(
        QInputMethodEvent::Attribute(QInputMethodEvent::Cursor, 2, 0, {}));
    QInputMethodEvent composition_event(QStringLiteral("ni hao"), composition_attributes);
    QCoreApplication::sendEvent(canvas, &composition_event);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->layoutRevision(), composition_revision + 1, 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("ni hao"));
    QVERIFY(!layout_view_model->compositionRects().isEmpty());
    QQuickItem* underline = nullptr;
    for (QQuickItem* item : canvas->parentItem()->childItems()) {
        if (item->objectName() == QStringLiteral("compositionUnderline")) {
            underline = item;
            break;
        }
    }
    QVERIFY(underline != nullptr);
    const QVariantMap composition_rect = layout_view_model->compositionRects().first().toMap();
    const auto expected_underline_start = [&]() {
        return canvas->documentToContent(
            composition_rect.value(QStringLiteral("pageIndex")).toInt(),
            QPointF(composition_rect.value(QStringLiteral("x0Pt")).toReal(),
                    composition_rect.value(QStringLiteral("y0Pt")).toReal()));
    };
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(underline->y(), expected_underline_start().y(), 3000);
    surface->setProperty("pageScale", 1.5);
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(underline->y(), expected_underline_start().y(), 3000);
    window.resize(window.width() + 100, window.height());
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    surface->setProperty("pageScale", 1.0);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->glyphFrames().isEmpty() &&
                                 !canvas->glyphFrames()
                                      .first()
                                      .toMap()
                                      .value(QStringLiteral("placements"))
                                      .toList()
                                      .isEmpty(),
                             3000);

    QInputMethodEvent mixed_composition(QStringLiteral("hello中文hello"), {});
    QCoreApplication::sendEvent(canvas, &mixed_composition);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->layoutRevision(), composition_revision + 2, 3000);
    const QVariantList mixed_rects = layout_view_model->compositionRects();
    QVERIFY(!mixed_rects.isEmpty());
    QList<QQuickItem*> underlines;
    for (QQuickItem* item : canvas->parentItem()->childItems()) {
        if (item->objectName() == QStringLiteral("compositionUnderline")) {
            underlines.push_back(item);
        }
    }
    QCOMPARE(underlines.size(), mixed_rects.size());
    for (qsizetype index = 0; index < mixed_rects.size(); ++index) {
        const QVariantMap rect = mixed_rects.at(index).toMap();
        const QPointF start =
            canvas->documentToContent(rect.value(QStringLiteral("pageIndex")).toInt(),
                                      QPointF(rect.value(QStringLiteral("x0Pt")).toReal(),
                                              rect.value(QStringLiteral("y0Pt")).toReal()));
        QCOMPARE(underlines.at(index)->x(), start.x());
        QCOMPARE(underlines.at(index)->y(), start.y());
    }

    QTest::keyClick(&window, Qt::Key_Escape);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->preeditText().isEmpty(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(layout_view_model->compositionRects().isEmpty(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("escape"));
    QVERIFY(layout_view_model->layoutRevision() >= composition_revision + 3);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());
    QVERIFY(layout_view_model->compositionRects().isEmpty());
    const QVariantMap idle_ime_response =
        layout_view_model->inputMethodEvent(QString(), QString(), 0, 0, 0);
    QVERIFY2(idle_ime_response.value(QStringLiteral("valid")).toBool(),
             qPrintable(idle_ime_response.value(QStringLiteral("error")).toString()));
    QCOMPARE(layout_view_model->documentGeneration(), QStringLiteral("3"));

    QKeyEvent command_start(QEvent::KeyPress, Qt::Key_Backslash, Qt::NoModifier,
                            QStringLiteral("\\"));
    QCoreApplication::sendEvent(canvas, &command_start);
    QTRY_VERIFY_WITH_TIMEOUT(
        layout_view_model->commandLine().value(QStringLiteral("active")).toBool(), 3000);
    QTest::keyClick(&window, Qt::Key_Escape);
    QTRY_VERIFY_WITH_TIMEOUT(
        !layout_view_model->commandLine().value(QStringLiteral("active")).toBool(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("escape"));
    QCOMPARE(layout_view_model->documentGeneration(), QStringLiteral("3"));

    QTest::keyClick(&window, Qt::Key_Left);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
        QStringLiteral("x！"), 3000);
    QCOMPARE(
        layout_view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 2);
    QTest::keyClick(&window, Qt::Key_Right);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
        QString(), 3000);

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("4"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts().size(), 1);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("5"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("6"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());

    QTest::keyClick(&window, Qt::Key_Return, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("shift-enter"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("7"), 3000);

    const QVariantMap drag_text = layout_view_model->insertText(QStringLiteral("abc"));
    QVERIFY2(drag_text.value(QStringLiteral("valid")).toBool(),
             qPrintable(drag_text.value(QStringLiteral("error")).toString()));
    QCOMPARE(layout_view_model->documentGeneration(), QStringLiteral("8"));
    QTest::keyClick(&window, Qt::Key_Left, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("anchorPosition")).toInt(), 4,
        3000);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 3,
        3000);
    QTRY_VERIFY_WITH_TIMEOUT(!layout_view_model->selectionRects().isEmpty(), 3000);
    QTest::keyClick(&window, Qt::Key_Right, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 4,
        3000);
    QTRY_VERIFY_WITH_TIMEOUT(layout_view_model->selectionRects().isEmpty(), 3000);

    const QVariantMap hit_map = layout_view_model->dragHitMap(0);
    QVERIFY2(hit_map.value(QStringLiteral("valid")).toBool(),
             qPrintable(hit_map.value(QStringLiteral("error")).toString()));
    const QVariantList stops = hit_map.value(QStringLiteral("stops")).toList();
    QVariantMap drag_start;
    QVariantMap drag_preview_start;
    QVariantMap drag_end;
    for (const QVariant& value : stops) {
        const QVariantMap stop = value.toMap();
        const QString offset = stop.value(QStringLiteral("position"))
                                   .toMap()
                                   .value(QStringLiteral("offsetUtf8"))
                                   .toString();
        if (offset == QStringLiteral("1") && drag_start.isEmpty())
            drag_start = stop;
        if (offset == QStringLiteral("0") && drag_preview_start.isEmpty())
            drag_preview_start = stop;
        if (offset == QStringLiteral("4"))
            drag_end = stop;
    }
    QVERIFY(!drag_start.isEmpty());
    QVERIFY(!drag_preview_start.isEmpty());
    QVERIFY(!drag_end.isEmpty());
    const auto stop_point = [canvas](const QVariantMap& stop) {
        return canvas->documentToContent(
            stop.value(QStringLiteral("pageIndex")).toInt(),
            QPointF(stop.value(QStringLiteral("xPt")).toReal(),
                    stop.value(QStringLiteral("y0Pt")).toReal() + 3.0));
    };
    QObject* scroller = surface->findChild<QObject*>(QStringLiteral("documentScroller"));
    QVERIFY(scroller != nullptr);
    QObject* selection_drag = surface->findChild<QObject*>(QStringLiteral("documentSelectionDrag"));
    QVERIFY(selection_drag != nullptr);
    QObject* pointer_handler =
        surface->findChild<QObject*>(QStringLiteral("documentCanvasPointerHandler"));
    QVERIFY(pointer_handler != nullptr);
    const qreal drag_scroll_y = scroller->property("contentY").toReal();
    const QPointF drag_preview_start_window =
        canvas->mapToItem(window.contentItem(), stop_point(drag_preview_start));
    const QPointF drag_start_window =
        canvas->mapToItem(window.contentItem(), stop_point(drag_start));
    const QPointF drag_end_window = canvas->mapToItem(window.contentItem(), stop_point(drag_end));
    const QRectF ime_caret_before_drag = canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, drag_preview_start_window.toPoint());
    const QPoint drag_end_point = drag_end_window.toPoint();
    QMouseEvent drag_move(QEvent::MouseMove, drag_end_window, drag_end_window,
                          window.mapToGlobal(drag_end_point), Qt::NoButton, Qt::LeftButton,
                          Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    const QVariantMap preview_anchor = pointer_handler->property("dragAnchor").toMap();
    const QVariantMap preview_focus = pointer_handler->property("dragFocus").toMap();
    QVERIFY(!preview_anchor.isEmpty());
    QVERIFY(!preview_focus.isEmpty());
    QVERIFY(preview_anchor.value(QStringLiteral("position"))
                .toMap()
                .value(QStringLiteral("offsetUtf8")) !=
            preview_focus.value(QStringLiteral("position"))
                .toMap()
                .value(QStringLiteral("offsetUtf8")));
    QTRY_COMPARE_WITH_TIMEOUT(canvas->caretPreviewGeometry().value(QStringLiteral("xPt")).toReal(),
                              preview_focus.value(QStringLiteral("xPt")).toReal(), 3000);
    QCOMPARE(canvas->caretPreviewGeometry().value(QStringLiteral("pageIndex")).toInt(),
             preview_focus.value(QStringLiteral("pageIndex")).toInt());
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(), ime_caret_before_drag);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->selectionPreviewRects().isEmpty(), 3000);
    const QVariantList preview_rectangles = canvas->selectionPreviewRects();
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!layout_view_model->selectionRects().isEmpty(), 3000);
    const QVariantList committed_rectangles = layout_view_model->selectionRects();
    const auto selection_bounds = [](const QVariantList& rectangles) {
        QRectF bounds;
        for (const QVariant& value : rectangles) {
            const QVariantMap rectangle = value.toMap();
            const QRectF current(rectangle.value(QStringLiteral("x0Pt")).toDouble(),
                                 rectangle.value(QStringLiteral("y0Pt")).toDouble(),
                                 rectangle.value(QStringLiteral("x1Pt")).toDouble() -
                                     rectangle.value(QStringLiteral("x0Pt")).toDouble(),
                                 rectangle.value(QStringLiteral("y1Pt")).toDouble() -
                                     rectangle.value(QStringLiteral("y0Pt")).toDouble());
            bounds = bounds.isNull() ? current : bounds.united(current);
        }
        return bounds;
    };
    QCOMPARE(selection_bounds(preview_rectangles), selection_bounds(committed_rectangles));
    QVERIFY(canvas->selectionPreviewRects().isEmpty());
    QVERIFY(canvas->caretPreviewGeometry().isEmpty());
    QCOMPARE(scroller->property("contentY").toReal(), drag_scroll_y);
    QCOMPARE(layout_view_model->documentGeneration(), QStringLiteral("8"));
    QCOMPARE(
        layout_view_model->inputMethodContext().value(QStringLiteral("anchorPosition")).toInt(), 0);
    QCOMPARE(
        layout_view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 4);

    const QRectF ime_caret_before_reverse_drag =
        canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QMouseEvent reverse_drag_move(QEvent::MouseMove, drag_preview_start_window,
                                  drag_preview_start_window,
                                  window.mapToGlobal(drag_preview_start_window.toPoint()),
                                  Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &reverse_drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    const QVariantMap reverse_focus = pointer_handler->property("dragFocus").toMap();
    QCOMPARE(reverse_focus.value(QStringLiteral("position"))
                 .toMap()
                 .value(QStringLiteral("offsetUtf8"))
                 .toString(),
             QStringLiteral("0"));
    QTRY_COMPARE_WITH_TIMEOUT(canvas->caretPreviewGeometry().value(QStringLiteral("xPt")).toReal(),
                              reverse_focus.value(QStringLiteral("xPt")).toReal(), 3000);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(),
             ime_caret_before_reverse_drag);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier,
                        drag_preview_start_window.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("anchorPosition")).toInt(), 4,
        3000);
    QTRY_COMPARE_WITH_TIMEOUT(
        layout_view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 0,
        3000);
    QVERIFY(canvas->caretPreviewGeometry().isEmpty());
    QCOMPARE(canvas->caretGeometry().value(QStringLiteral("xPt")).toReal(),
             reverse_focus.value(QStringLiteral("xPt")).toReal());

    QSignalSpy copy_key_spy(canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(copy_key_spy.isValid());
    QTest::keyClick(&window, Qt::Key_C, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(copy_key_spy.size(), 1, 3000);
    QCOMPARE(copy_key_spy.takeFirst().at(0).toString(), QStringLiteral("mod-c"));
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("\nabc"));
    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::keyClick(&window, Qt::Key_C, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(copy_key_spy.size(), 1, 3000);
    QCOMPARE(copy_key_spy.takeFirst().at(0).toString(), QStringLiteral("mod-c"));
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("abc"));
    QTest::keyClick(&window, Qt::Key_V, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("9"), 3000);

    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, drag_start_window.toPoint());
    QCoreApplication::sendEvent(&window, &drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(pointer_handler->property("selectionDragActive").toBool(), 3000);
    QVERIFY(!layout_view_model->selectionRects().isEmpty());
    QVERIFY(canvas->selectionRects().isEmpty());
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!pointer_handler->property("selectionDragActive").toBool(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->selectionRects().isEmpty(), 3000);

    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, drag_start_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(layout_view_model->selectionRects().isEmpty(), 3000);

    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, QPoint(1, 1));
    QTRY_VERIFY_WITH_TIMEOUT(layout_view_model->selectionRects().isEmpty(), 3000);

    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    const QString text_before_multiline_paste = layout_view_model->paragraphLayouts()
                                                    .at(0)
                                                    .toMap()
                                                    .value(QStringLiteral("text"))
                                                    .toString();
    const auto drag_start_offset = drag_start.value(QStringLiteral("position"))
                                       .toMap()
                                       .value(QStringLiteral("offsetUtf8"))
                                       .toString()
                                       .toUInt();
    const QString expected_first_paragraph =
        QString::fromUtf8(text_before_multiline_paste.toUtf8().first(drag_start_offset)) +
        QStringLiteral("left");
    QGuiApplication::clipboard()->setText(QStringLiteral("left\nright"));
    QTest::keyClick(&window, Qt::Key_V, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("10"), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->paragraphLayouts().size(), 2, 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(0)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             expected_first_paragraph);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("right"));

    const QVariantMap replacement_hit_map = layout_view_model->dragHitMap(0);
    QVERIFY(replacement_hit_map.value(QStringLiteral("valid")).toBool());
    const QString replacement_source = layout_view_model->paragraphLayouts()
                                           .first()
                                           .toMap()
                                           .value(QStringLiteral("text"))
                                           .toString();
    const QString replacement_end = QString::number(replacement_source.toUtf8().size());
    QVariantMap replacement_start_stop;
    QVariantMap replacement_end_stop;
    for (const QVariant& stop_value : replacement_hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = stop_value.toMap();
        const QString offset = stop.value(QStringLiteral("position"))
                                   .toMap()
                                   .value(QStringLiteral("offsetUtf8"))
                                   .toString();
        if (offset == QStringLiteral("0")) {
            replacement_start_stop = stop;
        }
        if (offset == replacement_end) {
            replacement_end_stop = stop;
        }
    }
    QVERIFY(!replacement_start_stop.isEmpty());
    QVERIFY(!replacement_end_stop.isEmpty());
    QVERIFY(layout_view_model->setSelection(replacement_start_stop, replacement_end_stop)
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(!layout_view_model->selectionRects().isEmpty());
    QSignalSpy interaction_changed_spy(layout_view_model,
                                       &caret::LayoutViewModel::interactionChanged);
    QVERIFY(interaction_changed_spy.isValid());
    QVERIFY(layout_view_model->insertText(QStringLiteral("replacement"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(interaction_changed_spy.size() > 0);
    QVERIFY(layout_view_model->selectionRects().isEmpty());
    QCOMPARE(
        layout_view_model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
        QStringLiteral("replacement"));

    // 指针从第一页直接跳到第三页时，中间页也必须出现在拖选预览中。
    QVERIFY(layout_view_model->insertText(QStringLiteral("word ").repeated(4000))
                .value(QStringLiteral("valid"))
                .toBool());
    QTRY_VERIFY_WITH_TIMEOUT(canvas->pageSizesPt().size() >= 3, 3000);
    const QVariantList first_stops =
        layout_view_model->dragHitMap(0).value(QStringLiteral("stops")).toList();
    const QVariantList third_stops =
        layout_view_model->dragHitMap(2).value(QStringLiteral("stops")).toList();
    QVERIFY(!first_stops.isEmpty());
    QVERIFY(!third_stops.isEmpty());
    pointer_handler->setProperty(
        "dragSnapshot",
        QVariantMap{{QStringLiteral("documentGeneration"), layout_view_model->documentGeneration()},
                    {QStringLiteral("fontGeneration"), layout_view_model->fontGeneration()},
                    {QStringLiteral("layoutRevision"),
                     QString::number(layout_view_model->layoutRevision())}});
    const QVariant first_point = QVariant::fromValue(stop_point(first_stops.first().toMap()));
    const QVariantMap third_stop = third_stops.first().toMap();
    const QVariant third_point = QVariant::fromValue(canvas->documentToContent(
        2, QPointF(third_stop.value(QStringLiteral("xPt")).toReal(),
                   third_stop.value(QStringLiteral("y0Pt")).toReal() + 3.0)));
    QCOMPARE(canvas->hitTestPage(third_point.toPointF()).value(QStringLiteral("pageIndex")).toInt(),
             2);
    QVERIFY(QMetaObject::invokeMethod(pointer_handler, "updateDragPreview",
                                      Q_ARG(QVariant, first_point),
                                      Q_ARG(QVariant, QVariant(true))));
    QVERIFY(QMetaObject::invokeMethod(pointer_handler, "updateDragPreview",
                                      Q_ARG(QVariant, third_point),
                                      Q_ARG(QVariant, QVariant(false))));
    QVERIFY(!pointer_handler->property("dragAnchor").toMap().isEmpty());
    const QVariantList cross_page_preview = canvas->selectionPreviewRects();
    QVERIFY(std::any_of(
        cross_page_preview.cbegin(), cross_page_preview.cend(), [](const QVariant& rectangle) {
            return rectangle.toMap().value(QStringLiteral("pageIndex")).toInt() == 1;
        }));
    pointer_handler->setProperty("selectionDragActive", true);
    QVERIFY(canvas->selectionPreviewRects().size() > 0);
    QVERIFY(!canvas->caretPreviewGeometry().isEmpty());
    QVERIFY(
        layout_view_model->insertText(QStringLiteral("x")).value(QStringLiteral("valid")).toBool());
    QTRY_VERIFY_WITH_TIMEOUT(pointer_handler->property("dragFocus").toMap().isEmpty(), 3000);
    QVERIFY(canvas->selectionPreviewRects().isEmpty());
    QVERIFY(canvas->caretPreviewGeometry().isEmpty());
    delete surface;
}

void ShellLoadTest::mathTextDragUsesSharedPreviewAndCommitProtocol() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString path = directory.filePath(QStringLiteral("math-drag.cdoc"));
    QFile document_file(path);
    QVERIFY(document_file.open(QIODevice::WriteOnly | QIODevice::Text));
    const QByteArray document = QByteArrayLiteral(R"json({
        "format":"caret","version":1,
        "requiredFeatures":["caret.math.content-flow.v2","caret.math.equation","caret.math.inline",
            "caret.math.syntax.v1","caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings","caret.text.default-font-size"],
        "media":[],"document":{
            "globalSettings":{},"body":[{"type":"paragraph","id":"paragraph","inlines":[
                {"type":"text","runs":[{"text":"before "}]},
                {"type":"inlineMath","id":"inline-text-math","mathTree":{
                    "schemaVersion":3,"root":{"id":"inline-root","content":[
                        {"type":"math","id":"inline-fraction","kind":"fraction",
                         "attributes":{"style":"frac"},"children":[
                            {"id":"numerator","content":[
                                {"type":"text","runs":[{"text":"xabcdef"}]}
                            ]},
                            {"id":"denominator","content":[]}
                        ]}
                    ]}
                }},
                {"type":"text","runs":[{"text":" after"}]},
                {"type":"inlineMath","id":"inline-structure-math","mathTree":{
                    "schemaVersion":3,"root":{"id":"structure-root","content":[
                        {"type":"math","id":"empty-fraction","kind":"fraction",
                         "attributes":{"style":"frac"},"children":[
                            {"id":"structure-numerator","content":[]},
                            {"id":"structure-denominator","content":[]}
                        ]}
                    ]}
                }}
            ]}],
            "pageSections":[{"id":"page-section","props":{}}]
        }
    })json");
    QCOMPARE(document_file.write(document), document.size());
    document_file.close();

    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    QQmlApplicationEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.Shell\n"
                                        "PageSurface { width: 600; height: 700 }\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    auto* surface = qobject_cast<QQuickItem*>(component.create());
    QVERIFY(surface != nullptr);
    QQuickWindow window;
    surface->setParentItem(window.contentItem());
    surface->setSize(QSizeF(600, 700));
    window.resize(600, 700);
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 3000);

    auto* view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(view_model != nullptr);
    QSignalSpy open_finished(view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(open_finished.isValid());
    const QVariantMap open_result = view_model->openDocument(path);
    QVERIFY2(open_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_result.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(open_finished.size(), 1, 5000);
    QVERIFY2(open_finished.first().at(0).toBool(),
             qPrintable(open_finished.first().at(1).toString()));

    auto* canvas = surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(canvas != nullptr);
    QObject* selection_drag = surface->findChild<QObject*>(QStringLiteral("documentSelectionDrag"));
    QObject* pointer_handler =
        surface->findChild<QObject*>(QStringLiteral("documentCanvasPointerHandler"));
    QVERIFY(selection_drag != nullptr);
    QVERIFY(pointer_handler != nullptr);

    const QVariantMap hit_map = view_model->dragHitMap(0);
    QVERIFY2(hit_map.value(QStringLiteral("valid")).toBool(),
             qPrintable(hit_map.value(QStringLiteral("error")).toString()));
    QVariantMap start;
    QVariantMap end;
    for (const QVariant& value : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("kind")).toString() != QStringLiteral("math"))
            continue;
        const QString order = position.value(QStringLiteral("orderUtf8")).toString();
        if (order == QStringLiteral("7") &&
            stop.value(QStringLiteral("visualAffinity")).toString() == QStringLiteral("downstream"))
            end = stop;
    }
    for (const QVariant& value : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("kind")).toString() == QStringLiteral("math") &&
            position.value(QStringLiteral("orderUtf8")).toString() == QStringLiteral("1") &&
            stop.value(QStringLiteral("domainId")).toString() ==
                end.value(QStringLiteral("domainId")).toString() &&
            stop.value(QStringLiteral("visualAffinity")).toString() ==
                QStringLiteral("downstream")) {
            start = stop;
            break;
        }
    }
    QVERIFY(!start.isEmpty());
    QVERIFY(!end.isEmpty());
    qreal math_left = std::numeric_limits<qreal>::max();
    qreal math_right = std::numeric_limits<qreal>::lowest();
    QVariantMap body_left;
    QVariantMap body_right;
    const QVariantList drag_stops = hit_map.value(QStringLiteral("stops")).toList();
    for (const QVariant& value : drag_stops) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        const qreal x = stop.value(QStringLiteral("xPt")).toReal();
        if (position.value(QStringLiteral("kind")).toString() == QStringLiteral("math")) {
            math_left = std::min(math_left, x);
            math_right = std::max(math_right, x);
        }
    }
    for (const QVariant& value : drag_stops) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        const qreal x = stop.value(QStringLiteral("xPt")).toReal();
        if (position.value(QStringLiteral("kind")).toString() == QStringLiteral("body")) {
            if (x < math_left - 3.0 &&
                (body_left.isEmpty() || x > body_left.value(QStringLiteral("xPt")).toReal())) {
                body_left = stop;
            }
            if (x > math_right + 3.0 &&
                (body_right.isEmpty() || x < body_right.value(QStringLiteral("xPt")).toReal())) {
                body_right = stop;
            }
        }
    }
    QVERIFY(math_right > math_left);
    QVERIFY(!body_left.isEmpty());
    QVERIFY(!body_right.isEmpty());
    qreal math_text_left = std::numeric_limits<qreal>::max();
    qreal math_text_right = std::numeric_limits<qreal>::lowest();
    qreal math_top = std::numeric_limits<qreal>::max();
    qreal math_bottom = std::numeric_limits<qreal>::lowest();
    for (const QVariant& value : drag_stops) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("kind")).toString() != QStringLiteral("math") ||
            stop.value(QStringLiteral("domainId")).toString() !=
                start.value(QStringLiteral("domainId")).toString())
            continue;
        math_text_left = std::min(math_text_left, stop.value(QStringLiteral("xPt")).toReal());
        math_text_right = std::max(math_text_right, stop.value(QStringLiteral("xPt")).toReal());
        math_top = std::min(math_top, stop.value(QStringLiteral("y0Pt")).toReal());
        math_bottom = std::max(math_bottom, stop.value(QStringLiteral("y1Pt")).toReal());
    }
    QVERIFY(math_text_right > math_text_left);
    QVERIFY(math_bottom > math_top);
    const QVariantMap start_domain =
        view_model->dragStartDomain(0, start.value(QStringLiteral("xPt")).toReal(),
                                    (start.value(QStringLiteral("y0Pt")).toReal() +
                                     start.value(QStringLiteral("y1Pt")).toReal()) /
                                        2.0);
    QVERIFY2(start_domain.value(QStringLiteral("valid")).toBool(),
             qPrintable(start_domain.value(QStringLiteral("error")).toString()));
    QCOMPARE(start_domain.value(QStringLiteral("domainId")).toString(),
             start.value(QStringLiteral("domainId")).toString());
    for (const QVariantMap& body_stop : {body_left, body_right}) {
        const QVariantMap body_domain =
            view_model->dragStartDomain(0, body_stop.value(QStringLiteral("xPt")).toReal(),
                                        (body_stop.value(QStringLiteral("y0Pt")).toReal() +
                                         body_stop.value(QStringLiteral("y1Pt")).toReal()) /
                                            2.0);
        QVERIFY2(body_domain.value(QStringLiteral("valid")).toBool(),
                 qPrintable(body_domain.value(QStringLiteral("error")).toString()));
        QCOMPARE(body_domain.value(QStringLiteral("domainId")).toString(), QStringLiteral("body"));
    }
    const auto stop_point = [canvas](const QVariantMap& stop) {
        return canvas->documentToContent(stop.value(QStringLiteral("pageIndex")).toInt(),
                                         QPointF(stop.value(QStringLiteral("xPt")).toReal(),
                                                 (stop.value(QStringLiteral("y0Pt")).toReal() +
                                                  stop.value(QStringLiteral("y1Pt")).toReal()) /
                                                     2.0));
    };
    const QString initial_generation = view_model->documentGeneration();
    for (const qreal zoom : {0.5, 1.0, 2.0}) {
        canvas->setZoom(zoom);
        const QPointF end_window = canvas->mapToItem(window.contentItem(), stop_point(end));
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, end_window.toPoint());
        QTRY_VERIFY_WITH_TIMEOUT(
            view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt() != 1,
            3000);
        QVERIFY(canvas->caretGeometry().value(QStringLiteral("xPt")).toReal() !=
                start.value(QStringLiteral("xPt")).toReal());

        const QPointF click_window = canvas->mapToItem(window.contentItem(), stop_point(start));
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, click_window.toPoint());
        QTRY_COMPARE_WITH_TIMEOUT(
            view_model->inputMethodContext().value(QStringLiteral("cursorPosition")).toInt(), 1,
            3000);
        const QVariantMap caret = canvas->caretGeometry();
        QCOMPARE(caret.value(QStringLiteral("xPt")).toReal(),
                 start.value(QStringLiteral("xPt")).toReal());
        const QRectF ime_caret = canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
        QVERIFY(!ime_caret.isEmpty());
        const QPointF ime_caret_document = canvas->contentToDocument(
            start.value(QStringLiteral("pageIndex")).toInt(), ime_caret.topLeft());
        QVERIFY(std::abs(ime_caret_document.x() - start.value(QStringLiteral("xPt")).toReal()) <
                0.02);
        QVERIFY(std::abs(ime_caret_document.y() - caret.value(QStringLiteral("y0Pt")).toReal()) <
                0.02);

        const QImage frame = window.grabWindow().convertToFormat(QImage::Format_ARGB32);
        QVERIFY(!frame.isNull());
        const qreal scale_x = static_cast<qreal>(frame.width()) / window.width();
        const qreal scale_y = static_cast<qreal>(frame.height()) / window.height();
        const QRectF caret_window =
            canvas->mapRectToItem(window.contentItem(), canvas->caretVisualRect());
        const QRect caret_pixels(
            qFloor(caret_window.left() * scale_x) - 2, qFloor(caret_window.top() * scale_y) - 2,
            qCeil(caret_window.width() * scale_x) + 4, qCeil(caret_window.height() * scale_y) + 4);
        const int page_index = start.value(QStringLiteral("pageIndex")).toInt();
        const QPointF origin = canvas->documentToContent(page_index, QPointF(0.0, 0.0));
        const QPointF unit_x = canvas->documentToContent(page_index, QPointF(1.0, 0.0));
        const qreal logical_scale = unit_x.x() - origin.x();
        QVERIFY(logical_scale > 0.0);
        qsizetype source_ink_pixels = 0;
        qsizetype rendered_ink_pixels = 0;
        for (const QVariant& frame_value : canvas->glyphFrames()) {
            const QVariantMap glyph_frame = frame_value.toMap();
            if (glyph_frame.value(QStringLiteral("pageIndex")).toInt() != page_index)
                continue;
            const qreal image_to_logical_scale =
                logical_scale / glyph_frame.value(QStringLiteral("rasterPxPerPt")).toReal();
            const QVariantList images = glyph_frame.value(QStringLiteral("images")).toList();
            for (const QVariant& placement_value :
                 glyph_frame.value(QStringLiteral("placements")).toList()) {
                const QVariantMap placement = placement_value.toMap();
                const qreal x_pt = placement.value(QStringLiteral("xPt")).toReal();
                const qreal baseline_y_pt = placement.value(QStringLiteral("baselineYPt")).toReal();
                if (x_pt < math_text_left - 1.0 || x_pt > math_text_right + 1.0 ||
                    baseline_y_pt < math_top - 20.0 || baseline_y_pt > math_bottom + 20.0)
                    continue;
                const int image_index = placement.value(QStringLiteral("imageIndex")).toInt();
                QVERIFY(image_index >= 0 && image_index < images.size());
                const QVariantMap image = images.at(image_index).toMap();
                const int image_width = image.value(QStringLiteral("width")).toInt();
                const int image_height = image.value(QStringLiteral("height")).toInt();
                const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
                QCOMPARE(pixels.size(), image_width * image_height * 4);
                bool source_has_ink = false;
                for (qsizetype offset = 0; offset + 3 < pixels.size(); offset += 4) {
                    const auto* rgba = reinterpret_cast<const uchar*>(pixels.constData() + offset);
                    if (rgba[3] > 0 && rgba[0] < 100 && rgba[1] < 100 && rgba[2] < 100) {
                        ++source_ink_pixels;
                        source_has_ink = true;
                    }
                }
                if (!source_has_ink || image_width <= 0 || image_height <= 0)
                    continue;

                const QPointF baseline =
                    canvas->documentToContent(page_index, QPointF(x_pt, baseline_y_pt));
                const bool phase_applied =
                    placement.value(QStringLiteral("pixelPhaseApplied")).toBool();
                const QPointF phase =
                    phase_applied ? QPointF(placement.value(QStringLiteral("pixelPhaseX")).toReal(),
                                            placement.value(QStringLiteral("pixelPhaseY")).toReal())
                                  : QPointF{};
                const QPointF glyph_top_left(
                    baseline.x() + (image.value(QStringLiteral("left")).toReal() - phase.x()) *
                                       image_to_logical_scale,
                    baseline.y() + (-image.value(QStringLiteral("top")).toReal() - phase.y()) *
                                       image_to_logical_scale);
                const QPointF window_top_left =
                    canvas->mapToItem(window.contentItem(), glyph_top_left);
                const qreal raster_to_frame_x = image_to_logical_scale * scale_x;
                const qreal raster_to_frame_y = image_to_logical_scale * scale_y;
                for (int source_y = 0; source_y < image_height; ++source_y) {
                    for (int source_x = 0; source_x < image_width; ++source_x) {
                        const qsizetype offset =
                            (static_cast<qsizetype>(source_y) * image_width + source_x) * 4;
                        if (static_cast<uchar>(pixels.at(offset + 3)) == 0)
                            continue;
                        const int x0 =
                            qFloor(window_top_left.x() * scale_x + source_x * raster_to_frame_x);
                        const int x1 = qCeil(window_top_left.x() * scale_x +
                                             (source_x + 1) * raster_to_frame_x) -
                                       1;
                        const int y0 =
                            qFloor(window_top_left.y() * scale_y + source_y * raster_to_frame_y);
                        const int y1 = qCeil(window_top_left.y() * scale_y +
                                             (source_y + 1) * raster_to_frame_y) -
                                       1;
                        for (int y = y0; y <= y1; ++y) {
                            for (int x = x0; x <= x1; ++x) {
                                if (!frame.rect().contains(x, y) || caret_pixels.contains(x, y))
                                    continue;
                                const QColor pixel = frame.pixelColor(x, y);
                                if (pixel.alpha() > 0 && pixel != QColor(Qt::white))
                                    ++rendered_ink_pixels;
                            }
                        }
                    }
                }
            }
        }
        QVERIFY2(source_ink_pixels > 0,
                 "the selected math placements must contain actual rasterized glyph ink");
        QVERIFY2(rendered_ink_pixels > 0,
                 "math glyph ink must render outside the current caret at each zoom level");
        QCOMPARE(view_model->documentGeneration(), initial_generation);
    }
    canvas->setZoom(1.0);
    const QPointF start_window = canvas->mapToItem(window.contentItem(), stop_point(start));
    const QPointF end_window = canvas->mapToItem(window.contentItem(), stop_point(end));
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, start_window.toPoint());
    const QPoint end_point = end_window.toPoint();
    QMouseEvent drag_move(QEvent::MouseMove, end_window, end_window, window.mapToGlobal(end_point),
                          Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    const QVariantMap drag_anchor = pointer_handler->property("dragAnchor").toMap();
    const QVariantMap drag_focus = pointer_handler->property("dragFocus").toMap();
    QVERIFY(!drag_anchor.isEmpty());
    QVERIFY(!drag_focus.isEmpty());
    QCOMPARE(drag_anchor.value(QStringLiteral("domainId")).toString(),
             start.value(QStringLiteral("domainId")).toString());
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->selectionPreviewRects().isEmpty(), 3000);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));
    const QVariantMap input_context = view_model->inputMethodContext();
    QCOMPARE(input_context.value(QStringLiteral("surroundingText")).toString(),
             QStringLiteral("xabcdef"));
    QCOMPARE(input_context.value(QStringLiteral("anchorPosition")).toInt(), 1);
    QCOMPARE(input_context.value(QStringLiteral("cursorPosition")).toInt(), 7);
    QVERIFY(input_context.value(QStringLiteral("enabled")).toBool());
    QVERIFY(view_model->copySelectionToClipboard().value(QStringLiteral("valid")).toBool());
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("abcdef"));

    QVERIFY(view_model->clearSelection().value(QStringLiteral("valid")).toBool());
    QVERIFY(view_model->selectionRects().isEmpty());
    QVariantMap structural_point;
    const QSizeF page_size = canvas->pageSizesPt().first().toSizeF();
    for (int y = 40; static_cast<qreal>(y) < std::min<qreal>(260.0, page_size.height()); y += 2) {
        for (int x = 40; static_cast<qreal>(x) < std::min<qreal>(page_size.width() - 20.0, 580.0);
             x += 2) {
            const qreal x_pt = static_cast<qreal>(x);
            const qreal y_pt = static_cast<qreal>(y);
            const QVariantMap placed = view_model->placeCaretAtPage(0, x_pt, y_pt);
            if (placed.value(QStringLiteral("valid")).toBool() &&
                placed.value(QStringLiteral("mathTree")).toBool()) {
                structural_point = {{QStringLiteral("xPt"), x_pt}, {QStringLiteral("yPt"), y_pt}};
                break;
            }
        }
        if (!structural_point.isEmpty()) {
            break;
        }
    }
    QVERIFY(!structural_point.isEmpty());
    const QVariantMap structural_domain =
        view_model->dragStartDomain(0, structural_point.value(QStringLiteral("xPt")).toReal(),
                                    structural_point.value(QStringLiteral("yPt")).toReal());
    QVERIFY2(structural_domain.value(QStringLiteral("valid")).toBool(),
             qPrintable(structural_domain.value(QStringLiteral("error")).toString()));
    QVERIFY(structural_domain.value(QStringLiteral("domainId")).toString().isEmpty());
    QVERIFY(view_model->selectionRects().isEmpty());

    const QPointF structural_window =
        canvas->mapToItem(window.contentItem(),
                          canvas->documentToContent(
                              0, QPointF(structural_point.value(QStringLiteral("xPt")).toReal(),
                                         structural_point.value(QStringLiteral("yPt")).toReal())));
    const QPointF body_left_window = canvas->mapToItem(window.contentItem(), stop_point(body_left));
    const QPointF body_right_window =
        canvas->mapToItem(window.contentItem(), stop_point(body_right));
    const auto distance_from_structural = [&structural_window](const QPointF& point) {
        return std::abs(point.x() - structural_window.x()) +
               std::abs(point.y() - structural_window.y());
    };
    const QPointF body_window =
        distance_from_structural(body_left_window) > distance_from_structural(body_right_window)
            ? body_left_window
            : body_right_window;
    QVERIFY(distance_from_structural(body_window) > 40.0);
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, structural_window.toPoint());
    QMouseEvent structural_to_text_move(QEvent::MouseMove, body_window, body_window,
                                        window.mapToGlobal(body_window.toPoint()), Qt::NoButton,
                                        Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &structural_to_text_move);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    QVERIFY(pointer_handler->property("dragAnchor").toMap().isEmpty());
    QVERIFY(canvas->selectionPreviewRects().isEmpty());
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, body_window.toPoint());
    QVERIFY(view_model->selectionRects().isEmpty());
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));
    delete surface;
}

void ShellLoadTest::documentInputDeltaKeepsUntouchedPagesAndParagraphs() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    QString source;
    for (int index = 0; index < 100; ++index) {
        if (index != 0) {
            source.append(QLatin1Char('\n'));
        }
        source.append(QStringLiteral("Paragraph %1 contains enough words to fill the page. ")
                          .arg(index)
                          .repeated(3));
    }
    const QByteArray source_utf8 = source.toUtf8();
    const rust::String prepared = caret::ffi::paste_text_json(
        *runtime, rust::String(std::string(source_utf8.constData(),
                                           static_cast<size_t>(source_utf8.size()))));
    QVERIFY(!std::string_view(prepared.data(), prepared.size()).starts_with("error:"));
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral(
            "import QtQuick\n"
            "import Caret.ViewModels\n"
            "Item {\n"
            "    property var result\n"
            "    property var preeditResult\n"
            "    property var updatedPreeditResult\n"
            "    property var commitResult\n"
            "    property var crossSelectionResult\n"
            "    property var crossPreeditResult\n"
            "    property var crossCancelResult\n"
            "    property int preeditParagraphCount: 0\n"
            "    property int preeditRectCount: 0\n"
            "    property int crossPreeditParagraphCount: 0\n"
            "    property int crossCancelParagraphCount: 0\n"
            "    property var pageSizes: LayoutViewModel.pageSizesPt\n"
            "    property var pageGlyphRevisions: LayoutViewModel.pageGlyphRevisions\n"
            "    property var pageLayouts: LayoutViewModel.pageLayouts\n"
            "    property var paragraphs: LayoutViewModel.paragraphLayouts\n"
            "    property var initialPageGlyphRevisions\n"
            "    property var postInsertPageGlyphRevisions\n"
            "    Component.onCompleted: {\n"
            "        const initialParagraphList = LayoutViewModel.paragraphLayouts\n"
            "        const lastParagraph = initialParagraphList[initialParagraphList.length - 1]\n"
            "        const lastPage = LayoutViewModel.pageSizesPt.length - 1\n"
            "        const stops = LayoutViewModel.dragHitMap(lastPage).stops\n"
            "        const endOffset = String(lastParagraph.text.length)\n"
            "        let endStop = null\n"
            "        for (let index = 0; index < stops.length; ++index) {\n"
            "            const position = stops[index].position\n"
            "            if (position.paragraphId === lastParagraph.paragraphId &&\n"
            "                    position.offsetUtf8 === endOffset) {\n"
            "                endStop = stops[index]\n"
            "                break\n"
            "            }\n"
            "        }\n"
            "        if (endStop === null)\n"
            "            throw new Error(\"Unable to find the final caret stop\")\n"
            "        const selectionResult = LayoutViewModel.setSelection(endStop, endStop)\n"
            "        if (!selectionResult.valid)\n"
            "            throw new Error(selectionResult.error)\n"
            "        initialPageGlyphRevisions = LayoutViewModel.pageGlyphRevisions.slice()\n"
            "        result = LayoutViewModel.insertText(\"x\")\n"
            "        postInsertPageGlyphRevisions = LayoutViewModel.pageGlyphRevisions.slice()\n"
            "        preeditResult = LayoutViewModel.inputMethodEvent(\"n\", \"\", 0, 0, 1)\n"
            "        preeditParagraphCount = LayoutViewModel.paragraphLayouts.length\n"
            "        preeditRectCount = LayoutViewModel.compositionRects.length\n"
            "        updatedPreeditResult = LayoutViewModel.inputMethodEvent(\"ni\", \"\", 0, 0, "
            "2)\n"
            "        commitResult = LayoutViewModel.inputMethodEvent(\"\", \"你\", 0, 0, 0)\n"
            "        const paragraphs = LayoutViewModel.paragraphLayouts\n"
            "        crossSelectionResult = LayoutViewModel.setSelection(\n"
            "            { position: { paragraphId: paragraphs[98].paragraphId, offsetUtf8: \"0\", "
            "affinity: \"downstream\" }, visualAffinity: \"downstream\" },\n"
            "            { position: { paragraphId: paragraphs[99].paragraphId, offsetUtf8: \"0\", "
            "affinity: \"upstream\" }, visualAffinity: \"upstream\" })\n"
            "        crossPreeditResult = LayoutViewModel.inputMethodEvent(\"x\", \"\", 0, 0, 1)\n"
            "        crossPreeditParagraphCount = LayoutViewModel.paragraphLayouts.length\n"
            "        crossCancelResult = LayoutViewModel.inputMethodEvent(\"\", \"\", 0, 0, 0)\n"
            "        crossCancelParagraphCount = LayoutViewModel.paragraphLayouts.length\n"
            "    }\n"
            "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY(probe != nullptr);
    const QVariantMap result = probe->property("result").toMap();
    QVERIFY2(result.value(QStringLiteral("valid")).toBool(),
             qPrintable(result.value(QStringLiteral("error")).toString()));
    const QVariantMap preedit_result = probe->property("preeditResult").toMap();
    QVERIFY2(preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(preedit_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(probe->property("preeditParagraphCount").toInt(), 100);
    QVERIFY(probe->property("preeditRectCount").toInt() > 0);
    const QVariantMap updated_preedit_result = probe->property("updatedPreeditResult").toMap();
    QVERIFY2(updated_preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(updated_preedit_result.value(QStringLiteral("error")).toString()));
    const QVariantMap commit_result = probe->property("commitResult").toMap();
    QVERIFY2(commit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(commit_result.value(QStringLiteral("error")).toString()));
    const QVariantMap cross_selection_result = probe->property("crossSelectionResult").toMap();
    QVERIFY2(cross_selection_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_selection_result.value(QStringLiteral("error")).toString()));
    const QVariantMap cross_preedit_result = probe->property("crossPreeditResult").toMap();
    QVERIFY2(cross_preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_preedit_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(probe->property("crossPreeditParagraphCount").toInt(), 99);
    const QVariantMap cross_cancel_result = probe->property("crossCancelResult").toMap();
    QVERIFY2(cross_cancel_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_cancel_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(probe->property("crossCancelParagraphCount").toInt(), 100);
    const QVariantList pages = probe->property("pageSizes").toList();
    const QVariantList initial_glyph_revisions =
        probe->property("initialPageGlyphRevisions").toList();
    const QVariantList glyph_revisions = probe->property("pageGlyphRevisions").toList();
    const QVariantList post_insert_glyph_revisions =
        probe->property("postInsertPageGlyphRevisions").toList();
    const QVariantList page_layouts = probe->property("pageLayouts").toList();
    const QVariantList paragraphs = probe->property("paragraphs").toList();
    QVERIFY(pages.size() > 1);
    QCOMPARE(initial_glyph_revisions.size(), pages.size());
    QCOMPARE(post_insert_glyph_revisions.size(), pages.size());
    QCOMPARE(glyph_revisions.size(), pages.size());
    QCOMPARE(post_insert_glyph_revisions.first(), initial_glyph_revisions.first());
    QVERIFY(post_insert_glyph_revisions.last() != initial_glyph_revisions.last());
    QCOMPARE(page_layouts.size(), pages.size());
    QCOMPARE(paragraphs.size(), 100);
    QVERIFY(paragraphs.first()
                .toMap()
                .value(QStringLiteral("text"))
                .toString()
                .startsWith(QStringLiteral("Paragraph 0")));
    QVERIFY(paragraphs.last()
                .toMap()
                .value(QStringLiteral("text"))
                .toString()
                .endsWith(QStringLiteral("x你")));
    delete probe;
}

void ShellLoadTest::invalidDocumentFileStatusPreservesThePublishedProjection() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    QVERIFY(caret::install_document_runtime(std::move(runtime)));
    caret::LayoutViewModel view_model;
    QSignalSpy changes(&view_model, &caret::LayoutViewModel::documentFileChanged);
    QVERIFY(changes.isValid());
    const QJsonObject valid_status{{QStringLiteral("path"), QStringLiteral("/tmp/math.cdoc")},
                                   {QStringLiteral("fileName"), QStringLiteral("math.cdoc")},
                                   {QStringLiteral("dirty"), true},
                                   {QStringLiteral("mathSourceDraftPending"), true},
                                   {QStringLiteral("canUndo"), true},
                                   {QStringLiteral("canRedo"), false}};
    QVERIFY(caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, valid_status));
    QVERIFY(view_model.documentMathSourceDraftPending());
    QCOMPARE(view_model.documentPath(), QStringLiteral("/tmp/math.cdoc"));
    QCOMPARE(changes.size(), 1);

    QJsonObject malformed_status = valid_status;
    malformed_status.insert(QStringLiteral("mathSourceDraftPending"), QStringLiteral("true"));
    QVERIFY(
        !caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, malformed_status));
    QVERIFY(view_model.documentMathSourceDraftPending());
    QCOMPARE(view_model.documentPath(), QStringLiteral("/tmp/math.cdoc"));
    QCOMPARE(changes.size(), 1);

    malformed_status = valid_status;
    malformed_status.remove(QStringLiteral("mathSourceDraftPending"));
    QVERIFY(
        !caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, malformed_status));
    QVERIFY(view_model.documentMathSourceDraftPending());
    QCOMPARE(changes.size(), 1);

    QJsonObject resolved_status = valid_status;
    resolved_status.insert(QStringLiteral("mathSourceDraftPending"), false);
    QVERIFY(
        caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, resolved_status));
    QVERIFY(!view_model.documentMathSourceDraftPending());
    QVERIFY(view_model.documentEdited());
    QCOMPARE(changes.size(), 2);
    QVERIFY(
        caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, resolved_status));
    QCOMPARE(changes.size(), 2);

    QVERIFY(caret::LayoutViewModelTestProbe::installDocumentFileStatus(view_model, valid_status));
    QVERIFY(view_model.documentMathSourceDraftPending());
    QVERIFY(view_model.documentEdited());
    QCOMPARE(changes.size(), 3);

    QSignalSpy operations(&view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(operations.isValid());
    const QVariantMap save_result = view_model.saveDocument();
    QVERIFY(!save_result.value(QStringLiteral("valid")).toBool());
    QVERIFY(!save_result.value(QStringLiteral("needsSaveAs")).toBool());
    QCOMPARE(save_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("document.save_incomplete_math_source_draft"));
    QCOMPARE(operations.size(), 1);
    QCOMPARE(operations.first().at(0).toBool(), false);
    QCOMPARE(operations.first().at(1).toString(),
             QStringLiteral("document.save_incomplete_math_source_draft"));
}

void ShellLoadTest::interactionResponseErrorsRefreshAutoSaveDeadline() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    QVERIFY(caret::install_document_runtime(std::move(runtime)));
    caret::LayoutViewModel view_model;
    QVERIFY(view_model.insertText(QStringLiteral("dirty document"))
                .value(QStringLiteral("valid"))
                .toBool());
    view_model.setAutoSaveEnabled(true);
    QVERIFY(caret::LayoutViewModelTestProbe::autoSaveTimerActive(view_model));

    caret::LayoutViewModelTestProbe::stopAutoSaveTimer(view_model);
    const rust::String interaction = caret::ffi::interaction_state_json(*caret::document_runtime());
    QVERIFY(!std::string_view(interaction.data(), interaction.size()).starts_with("error:"));
    const QByteArray interaction_bytes(interaction.data(),
                                       static_cast<qsizetype>(interaction.size()));
    const QVariantMap installed =
        caret::LayoutViewModelTestProbe::installInteractionResponse(view_model, interaction_bytes);
    QVERIFY(installed.value(QStringLiteral("valid")).toBool());
    QVERIFY(caret::LayoutViewModelTestProbe::autoSaveTimerActive(view_model));

    caret::LayoutViewModelTestProbe::stopAutoSaveTimer(view_model);
    const QVariantMap failed = caret::LayoutViewModelTestProbe::installInteractionResponse(
        view_model, QByteArrayLiteral("error:layout.test_failure"));
    QVERIFY(!failed.value(QStringLiteral("valid")).toBool());
    QCOMPARE(failed.value(QStringLiteral("error")).toString(),
             QStringLiteral("error:layout.test_failure"));
    QVERIFY(caret::LayoutViewModelTestProbe::autoSaveTimerActive(view_model));

    caret::LayoutViewModelTestProbe::stopAutoSaveTimer(view_model);
    const QVariantMap invalid = caret::LayoutViewModelTestProbe::installEditResponse(
        view_model, QByteArrayLiteral("error:layout.test_failure"));
    QVERIFY(!invalid.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid.value(QStringLiteral("error")).toString(),
             QStringLiteral("error:layout.test_failure"));
    QVERIFY(caret::LayoutViewModelTestProbe::autoSaveTimerActive(view_model));
}
#endif
