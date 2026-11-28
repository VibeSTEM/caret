// Qt/QML 文档命中、输入法与编辑交互的集成测试。
#include "shell_document_interaction_test.hpp"

#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "keyboard_modifier_policy.hpp"
#include "layout_view_model.hpp"
#include "shell_test_support.hpp"
#include "theme_view_model.hpp"
#include <tuple>
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "theme_catalog.hpp"
#endif
#include <QByteArray>
#include <QChar>
#include <QClipboard>
#include <QColor>
#include <QDir>
#include <QElapsedTimer>
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
#include <QMimeData>
#include <QMouseEvent>
#include <QObject>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QRect>
#include <QRectF>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSet>
#include <QShortcut>
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
#include <QtCore/qtestsupport_core.h>
#include <QtCore/qvariant.h>
#include <QtGui/qtestsupport_gui.h>
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
#include <memory>
#include <qcontainerfwd.h>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#ifdef CARET_ENABLE_APP
namespace caret {
// 测试通过私有入口注入异常协议载荷，并检查计时器是否激活；随后停止计时器，避免测试期间实际触发自动保存。
// 这些操作没有对应的公开 API。
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

void ShellDocumentInteractionTest::mathFontFallbackDiagnosticProjectsToViewModel() {
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

void ShellDocumentInteractionTest::textFontWarningStatusBarTracksLayoutDiagnostics() {
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
    const QString initial_session_id = view_model->sessionId();
    const QString initial_document_generation = view_model->documentGeneration();
    const QString initial_font_generation = view_model->fontGeneration();
    snapshot.insert(QStringLiteral("textDiagnostics"), QJsonArray{});
    QVERIFY(caret::LayoutViewModelTestProbe::installSnapshot(*view_model, snapshot));
    QVERIFY(!warning->property("visible").toBool());
    QVERIFY(warning->property("text").toString().isEmpty());
}

void ShellDocumentInteractionTest::environmentHighlightSnapshotSelectsAndClearsActiveRects() {
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
    const QJsonObject math_block_rect{{QStringLiteral("pageIndex"), 0},
                                      {QStringLiteral("x0Pt"), 5.0},
                                      {QStringLiteral("y0Pt"), 5.0},
                                      {QStringLiteral("x1Pt"), 6.0},
                                      {QStringLiteral("y1Pt"), 6.0}};
    const QJsonObject math_text_rect{{QStringLiteral("pageIndex"), 0},
                                     {QStringLiteral("x0Pt"), 7.0},
                                     {QStringLiteral("y0Pt"), 7.0},
                                     {QStringLiteral("x1Pt"), 8.0},
                                     {QStringLiteral("y1Pt"), 8.0}};
    root.insert(QStringLiteral("blockEnvironmentHighlightRects"), QJsonArray{focus_rect});
    root.insert(QStringLiteral("inlineEnvironmentHighlightRects"), QJsonArray{inline_rect});
    root.insert(QStringLiteral("mathBlockHighlightRects"), QJsonArray{math_block_rect});
    root.insert(QStringLiteral("mathTextHighlightRects"), QJsonArray{math_text_rect});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));
    caret::LayoutViewModel inline_view_model;
    QCOMPARE(inline_view_model.blockHighlightRects().size(), 2);
    QCOMPARE(inline_view_model.inlineHighlightRects().size(), 2);
    QCOMPARE(inline_view_model.inlineHighlightRects().first().toMap().value(QStringLiteral("x0Pt")),
             3.0);

    const QString serialized_snapshot =
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact));
    QVERIFY(caret::LayoutViewModelTestProbe::installSnapshotJson(inline_view_model,
                                                                 serialized_snapshot, true));
    const QVariantList block_highlight_before_invalid_json =
        inline_view_model.blockHighlightRects();
    const QVariantList inline_highlight_before_invalid_json =
        inline_view_model.inlineHighlightRects();
    const QVariantList snapshot_before_invalid_json = inline_view_model.pageLayouts();
    const QVariantMap input_context_before_invalid_json = inline_view_model.inputMethodContext();
    QCOMPARE(block_highlight_before_invalid_json.size(), 2);
    QCOMPARE(inline_highlight_before_invalid_json.size(), 2);
    QVERIFY(!snapshot_before_invalid_json.isEmpty());
    QVERIFY(input_context_before_invalid_json.contains(QStringLiteral("enabled")));
    QCOMPARE(input_context_before_invalid_json.value(QStringLiteral("enabled")).toBool(), true);
    QSignalSpy invalid_snapshot_layout_changed(&inline_view_model,
                                               &caret::LayoutViewModel::layoutChanged);
    QSignalSpy invalid_snapshot_interaction_changed(&inline_view_model,
                                                    &caret::LayoutViewModel::interactionChanged);
    QVERIFY(!caret::LayoutViewModelTestProbe::installSnapshotJson(inline_view_model,
                                                                  QStringLiteral("{"), true));
    QCOMPARE(inline_view_model.blockHighlightRects(), block_highlight_before_invalid_json);
    QCOMPARE(inline_view_model.inlineHighlightRects(), inline_highlight_before_invalid_json);
    QCOMPARE(inline_view_model.pageLayouts(), snapshot_before_invalid_json);
    QCOMPARE(inline_view_model.inputMethodContext(), input_context_before_invalid_json);
    QCOMPARE(invalid_snapshot_layout_changed.size(), 0);
    QCOMPARE(invalid_snapshot_interaction_changed.size(), 0);
    QCOMPARE(inline_view_model.error(), QStringLiteral("layout.invalid_snapshot"));

    const QVariantList visible_block_rects = inline_view_model.blockHighlightRects();
    const QVariantList visible_inline_rects = inline_view_model.inlineHighlightRects();
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
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
    QCOMPARE(inline_view_model.pageLayouts(), visible_pages);
    QCOMPARE(interaction_changed.size(), 0);

    const QVariantMap previous_input_method_context = inline_view_model.inputMethodContext();
    const QJsonObject invalid_math_block_rect_interaction{
        {QStringLiteral("documentGeneration"), inline_view_model.documentGeneration()},
        {QStringLiteral("fontGeneration"), inline_view_model.fontGeneration()},
        {QStringLiteral("selectionRects"), QJsonArray{}},
        {QStringLiteral("inputMethodContext"),
         QJsonObject{{QStringLiteral("enabled"), true}, {QStringLiteral("preferLatin"), false}}},
        {QStringLiteral("caretGeometry"), QJsonObject{{QStringLiteral("pageIndex"), 0},
                                                      {QStringLiteral("xPt"), 0.0},
                                                      {QStringLiteral("y0Pt"), 0.0},
                                                      {QStringLiteral("y1Pt"), 1.0}}},
        {QStringLiteral("mathBlockHighlightRects"),
         QJsonArray{QJsonObject{{QStringLiteral("pageIndex"), 0},
                                {QStringLiteral("x0Pt"), 1.0},
                                {QStringLiteral("y0Pt"), 1.0},
                                {QStringLiteral("x1Pt"), 700.0},
                                {QStringLiteral("y1Pt"), 2.0}}}}};
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(
        inline_view_model, invalid_math_block_rect_interaction));
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
    QCOMPARE(inline_view_model.inputMethodContext(), previous_input_method_context);
    QCOMPARE(interaction_changed.size(), 0);

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
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
    QCOMPARE(inline_view_model.inputMethodContext(), previous_input_method_context);
    QCOMPARE(interaction_changed.size(), 0);

    QJsonObject invalid_prefer_latin_context{
        {QStringLiteral("surroundingText"), QStringLiteral("")},
        {QStringLiteral("cursorPosition"), 0},
        {QStringLiteral("anchorPosition"), 0},
        {QStringLiteral("enabled"), true},
        {QStringLiteral("preferLatin"), QStringLiteral("true")}};
    QJsonObject invalid_prefer_latin_interaction{
        {QStringLiteral("documentGeneration"), inline_view_model.documentGeneration()},
        {QStringLiteral("fontGeneration"), inline_view_model.fontGeneration()},
        {QStringLiteral("selectionRects"), QJsonArray{}},
        {QStringLiteral("inputMethodContext"), invalid_prefer_latin_context},
        {QStringLiteral("caretGeometry"), QJsonObject{{QStringLiteral("pageIndex"), 0},
                                                      {QStringLiteral("xPt"), 0.0},
                                                      {QStringLiteral("y0Pt"), 0.0},
                                                      {QStringLiteral("y1Pt"), 1.0}}}};
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(inline_view_model,
                                                                 invalid_prefer_latin_interaction));
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
    QCOMPARE(inline_view_model.inputMethodContext(), previous_input_method_context);
    QCOMPARE(interaction_changed.size(), 0);

    QJsonObject missing_input_context_snapshot = root;
    missing_input_context_snapshot.remove(QStringLiteral("inputMethodContext"));
    QVERIFY(!caret::LayoutViewModelTestProbe::installSnapshot(inline_view_model,
                                                              missing_input_context_snapshot));
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
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
    QCOMPARE(inline_view_model.blockHighlightRects(), visible_block_rects);
    QCOMPARE(inline_view_model.inlineHighlightRects(), visible_inline_rects);
    QCOMPARE(interaction_changed.size(), 0);

    root.insert(QStringLiteral("inlineEnvironmentHighlightRects"), QJsonArray{});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    caret::LayoutViewModel block_view_model;
    QCOMPARE(block_view_model.blockHighlightRects().size(), 2);
    QCOMPARE(block_view_model.inlineHighlightRects().size(), 1);

    root.insert(QStringLiteral("blockEnvironmentHighlightRects"), QJsonArray{});
    root.insert(QStringLiteral("mathBlockHighlightRects"), QJsonArray{});
    root.insert(QStringLiteral("mathTextHighlightRects"), QJsonArray{});
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(QJsonDocument(root).toJson(QJsonDocument::Compact))));
    caret::LayoutViewModel empty_view_model;
    QVERIFY(empty_view_model.blockHighlightRects().isEmpty());
    QVERIFY(empty_view_model.inlineHighlightRects().isEmpty());
}

void ShellDocumentInteractionTest::
    commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument() {
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
    QCOMPARE(candidate.value(QStringLiteral("label")).toString(), QStringLiteral("Section"));
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

    QVERIFY(view_model->commandLineCommand(QStringLiteral("begin"))
                .value(QStringLiteral("valid"))
                .toBool());
    QVERIFY(view_model->commandLineCommand(QStringLiteral("type"), QStringLiteral("math"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QVariantMap enter_math = view_model->commandLineCommand(QStringLiteral("accept"));
    QVERIFY2(enter_math.value(QStringLiteral("valid")).toBool(),
             qPrintable(enter_math.value(QStringLiteral("error")).toString()));
    const QVariantMap open_math_query = view_model->insertText(QStringLiteral("\\"));
    QVERIFY2(open_math_query.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_math_query.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model->commandLine().value(QStringLiteral("mode")).toString(),
             QStringLiteral("math"));
    const QVariantMap math_query = view_model->insertText(QStringLiteral("alpha"));
    QVERIFY2(math_query.value(QStringLiteral("valid")).toBool(),
             qPrintable(math_query.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model->commandLine().value(QStringLiteral("cursor")).toInt(), 5);
    const QVariantMap math_navigation = view_model->moveVisualCaret(false, false);
    QVERIFY2(math_navigation.value(QStringLiteral("valid")).toBool(),
             qPrintable(math_navigation.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model->commandLine().value(QStringLiteral("cursor")).toInt(), 4);
    delete probe;
}

void ShellDocumentInteractionTest::inputMethodContextFlowsFromViewModelToCanvas() {
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
                          "import Caret.Canvas\n"
                          "import Caret.ViewModels\n"
                          "Item { DocumentCanvas { objectName: \"canvas\"; "
                          "inputMethodContext: LayoutViewModel.inputMethodContext } }\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* root = component.create();
    QVERIFY(root != nullptr);
    auto* canvas = root->findChild<caret::DocumentCanvas*>(QStringLiteral("canvas"));
    QVERIFY(canvas != nullptr);
    auto* view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(view_model != nullptr);
    QSignalSpy interaction_changed(view_model, &caret::LayoutViewModel::interactionChanged);
    QSignalSpy context_changed(canvas, &caret::DocumentCanvas::inputMethodContextChanged);
    QVERIFY(interaction_changed.isValid());
    QVERIFY(context_changed.isValid());

    const rust::String interaction = caret::ffi::interaction_state_json(*caret::document_runtime());
    QVERIFY(!std::string_view(interaction.data(), interaction.size()).starts_with("error:"));
    QJsonObject root_json =
        QJsonDocument::fromJson(
            QByteArray(interaction.data(), static_cast<qsizetype>(interaction.size())))
            .object();
    QJsonObject input_context = root_json.value(QStringLiteral("inputMethodContext")).toObject();
    QVERIFY(input_context.contains(QStringLiteral("enabled")));
    QVERIFY(input_context.contains(QStringLiteral("preferLatin")));

    const QList<QPair<bool, bool>> contexts{{true, true}, {false, false}};
    for (const auto& [enabled, prefer_latin] : contexts) {
        input_context.insert(QStringLiteral("enabled"), enabled);
        input_context.insert(QStringLiteral("preferLatin"), prefer_latin);
        root_json.insert(QStringLiteral("inputMethodContext"), input_context);
        QVariantMap expected_context = input_context.toVariantMap();
        if (!expected_context.contains(QStringLiteral("surroundingText"))) {
            expected_context.insert(
                QStringLiteral("surroundingText"),
                view_model->inputMethodContext().value(QStringLiteral("surroundingText")));
        }
        const qsizetype context_notifications = context_changed.size();
        const qsizetype interaction_notifications = interaction_changed.size();
        QVERIFY(caret::LayoutViewModelTestProbe::installInteraction(*view_model, root_json));
        QTRY_COMPARE_WITH_TIMEOUT(canvas->inputMethodContext(), expected_context, 3000);
        QCOMPARE(canvas->inputMethodQuery(Qt::ImEnabled).toBool(), enabled);
        QCOMPARE(canvas->inputMethodQuery(Qt::ImHints)
                     .value<Qt::InputMethodHints>()
                     .testFlag(Qt::ImhPreferLatin),
                 prefer_latin);
        QTRY_VERIFY_WITH_TIMEOUT(context_changed.size() > context_notifications, 3000);
        QVERIFY(interaction_changed.size() > interaction_notifications);
    }
    delete root;
}

void ShellDocumentInteractionTest::cancelledInputMethodStateAllowsTheNextCommit() {
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

void ShellDocumentInteractionTest::documentCanvasRoutesCommandKeysAndKeepsNormalEditingBehavior() {
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
        {QStringLiteral("preferLatin"), true},
    };
    canvas->setInputMethodContext(input_method_context);
    QCOMPARE(canvas->inputMethodContext(), input_method_context);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImSurroundingText).toString(), QStringLiteral("caret"));
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorPosition).toInt(), 5);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImAnchorPosition).toInt(), 0);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCurrentSelection).toString(), QStringLiteral("caret"));
    QVERIFY(canvas->inputMethodQuery(Qt::ImHints)
                .value<Qt::InputMethodHints>()
                .testFlag(Qt::ImhPreferLatin));

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
    QTest::keyClick(&window, Qt::Key_Tab, Qt::ShiftModifier);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("shift-tab"));
    QTest::keyClick(&window, Qt::Key_T, Qt::AltModifier);
    QCOMPARE(editor_keys.size(), 1);
    QCOMPARE(editor_keys.takeFirst().at(0).toString(), QStringLiteral("insert-table"));
#ifdef Q_OS_MACOS
    const Qt::KeyboardModifiers insert_modifiers =
        caret::keyboard::physicalControlModifier() | Qt::AltModifier | Qt::ShiftModifier;
    const Qt::KeyboardModifiers delete_row_modifiers = Qt::AltModifier | Qt::ShiftModifier;
    const Qt::KeyboardModifiers delete_column_modifiers = Qt::AltModifier;
#else
    const Qt::KeyboardModifiers insert_modifiers = Qt::AltModifier;
    const Qt::KeyboardModifiers alternative_insert_modifiers =
        caret::keyboard::physicalControlModifier() | Qt::AltModifier | Qt::ShiftModifier;
    const Qt::KeyboardModifiers delete_row_modifiers = Qt::AltModifier | Qt::ShiftModifier;
    const Qt::KeyboardModifiers delete_column_modifiers = Qt::AltModifier;
#endif
    const QList<std::tuple<Qt::Key, Qt::KeyboardModifiers, QString>> table_key_cases = {
        {Qt::Key_Up, insert_modifiers, QStringLiteral("table-insert-row-before")},
        {Qt::Key_Down, insert_modifiers, QStringLiteral("table-insert-row-after")},
        {Qt::Key_Left, insert_modifiers, QStringLiteral("table-insert-col-before")},
        {Qt::Key_Right, insert_modifiers, QStringLiteral("table-insert-col-after")},
        {Qt::Key_Backspace, delete_row_modifiers, QStringLiteral("table-delete-row-backspace")},
        {Qt::Key_Delete, delete_row_modifiers, QStringLiteral("table-delete-row-delete")},
        {Qt::Key_Backspace, delete_column_modifiers,
         QStringLiteral("table-delete-column-backspace")},
        {Qt::Key_Delete, delete_column_modifiers, QStringLiteral("table-delete-column-delete")},
    };
    QShortcut colliding_window_shortcut(QKeySequence(Qt::ALT | Qt::Key_Backspace), &window);
    QSignalSpy colliding_shortcut_activated(&colliding_window_shortcut, &QShortcut::activated);
    QVERIFY(colliding_shortcut_activated.isValid());
    for (const auto& [key, modifiers, expected] : table_key_cases) {
        QKeyEvent shortcut_override(QEvent::ShortcutOverride, key, modifiers);
        shortcut_override.ignore();
        QCoreApplication::sendEvent(canvas, &shortcut_override);
        QVERIFY2(shortcut_override.isAccepted(),
                 "table editing shortcut must override window shortcuts");
        QTest::keyClick(&window, key, modifiers);
        QCOMPARE(editor_keys.size(), 1);
        QCOMPARE(editor_keys.takeFirst().at(0).toString(), expected);
        if (key == Qt::Key_Backspace && modifiers == delete_column_modifiers) {
            QVERIFY2(colliding_shortcut_activated.isEmpty(),
                     "the focused canvas must win against a colliding window shortcut");
        }
    }
#ifndef Q_OS_MACOS
    const QList<QPair<Qt::Key, QString>> insert_cases = {
        {Qt::Key_Up, QStringLiteral("table-insert-row-before")},
        {Qt::Key_Down, QStringLiteral("table-insert-row-after")},
        {Qt::Key_Left, QStringLiteral("table-insert-col-before")},
        {Qt::Key_Right, QStringLiteral("table-insert-col-after")},
    };
    for (const auto& [key, expected] : insert_cases) {
        QKeyEvent shortcut_override(QEvent::ShortcutOverride, key, alternative_insert_modifiers);
        shortcut_override.ignore();
        QCoreApplication::sendEvent(canvas, &shortcut_override);
        QVERIFY2(shortcut_override.isAccepted(),
                 "alternate table insertion shortcut must override window shortcuts");
        QTest::keyClick(&window, key, alternative_insert_modifiers);
        QCOMPARE(editor_keys.size(), 1);
        QCOMPARE(editor_keys.takeFirst().at(0).toString(), expected);
    }
#endif
    auto* non_canvas_focus = new QQuickItem(window.contentItem());
    non_canvas_focus->setFocus(true);
    QTRY_VERIFY_WITH_TIMEOUT(non_canvas_focus->hasActiveFocus(), 3000);
    QTest::keyClick(&window, Qt::Key_Backspace, delete_column_modifiers);
    QCOMPARE(colliding_shortcut_activated.size(), 1);
    canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
#ifdef Q_OS_MACOS
    // macOS 上 Qt 用 ControlModifier 表示 Command；此处只接受物理 Control 映射。
    QTest::keyClick(&window, Qt::Key_Up, Qt::ControlModifier | Qt::AltModifier | Qt::ShiftModifier);
    QVERIFY2(editor_keys.isEmpty(), "Command+Option+Shift+Up is not the table insertion chord");
#endif
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

// 文档交互 slot 共用的 PageSurface 窗口 fixture 与前置步骤 helper。
//
// 每个 slot 独立构建 fixture 并重放到所需文档状态，避免一个函数串起多个互不
// 相关的行为契约；helper 只负责建立前置状态，断言仍留在各 slot 内。
namespace {

class DocumentInteractionFixture {
  public:
    DocumentInteractionFixture() = default;
    ~DocumentInteractionFixture() { delete surface; }
    DocumentInteractionFixture(const DocumentInteractionFixture&) = delete;
    DocumentInteractionFixture& operator=(const DocumentInteractionFixture&) = delete;

    bool initialize(QSize size, const QString& active_theme = QStringLiteral("light"),
                    bool sentinel_theme_colors = false);

    std::unique_ptr<QQmlEngine> engine;
    std::unique_ptr<QQmlComponent> surface_component;
    QQuickWindow window;
    QQuickItem* surface = nullptr;
    caret::DocumentCanvas* canvas = nullptr;
    caret::LayoutViewModel* layout_view_model = nullptr;
    QObject* scroller = nullptr;
    QObject* selection_drag = nullptr;
    QObject* pointer_handler = nullptr;
    QString error;
};

// 页面渲染依赖 Theme 单例；单独运行某个 slot 时先注入断言所需的主题 token。
// sentinel_theme_colors 只给像素验收提供易区分色值，不表示产品主题的配色结论。
void installDocumentInteractionTheme(const QString& active_theme = QStringLiteral("light"),
                                     bool sentinel_theme_colors = false) {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    QVariantMap light =
        caret::shell_test::snapshot_from_theme_facade(theme, QColor(QStringLiteral("#ffffff")), 16);
    QVariantMap dark =
        caret::shell_test::snapshot_from_theme_facade(theme, QColor(QStringLiteral("#111111")), 18);
    if (sentinel_theme_colors) {
        light.insert(QStringLiteral("colorTableGuideline"), QColor(QStringLiteral("#e000c0")));
        light.insert(QStringLiteral("colorBlockEnvironmentHighlight"),
                     QColor(QStringLiteral("#00d060")));
        dark.insert(QStringLiteral("colorTableGuideline"), QColor(QStringLiteral("#20c0ff")));
        dark.insert(QStringLiteral("colorBlockEnvironmentHighlight"),
                    QColor(QStringLiteral("#ffc020")));
    }
    themes.insert(QStringLiteral("light"), std::move(light));
    themes.insert(QStringLiteral("dark"), std::move(dark));
    caret::install_theme_snapshots(std::move(themes), active_theme);
}

bool installBlankInteractionRuntime(QString* error) {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    if (std::string_view(snapshot.data(), snapshot.size()).starts_with("error:")) {
        *error = QStringLiteral("空白文档快照返回错误");
        return false;
    }
    if (!caret::install_layout_snapshot(
            QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size())))) {
        *error = QStringLiteral("空白文档快照无法安装");
        return false;
    }
    if (!caret::install_document_runtime(std::move(runtime))) {
        *error = QStringLiteral("空白文档 runtime 无法安装");
        return false;
    }
    return true;
}

bool DocumentInteractionFixture::initialize(QSize size, const QString& active_theme,
                                            bool sentinel_theme_colors) {
    installDocumentInteractionTheme(active_theme, sentinel_theme_colors);
    engine = std::make_unique<QQmlEngine>();
    engine->addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    surface_component = std::make_unique<QQmlComponent>(engine.get());
    surface_component->setData(QByteArrayLiteral("import QtQuick\n"
                                                 "import Caret.Shell\n"
                                                 "PageSurface {}\n"),
                               QUrl());
    if (!surface_component->isReady()) {
        error = surface_component->errorString();
        return false;
    }
    const QVariantMap dimensions{{QStringLiteral("width"), size.width()},
                                 {QStringLiteral("height"), size.height()}};
    surface = qobject_cast<QQuickItem*>(surface_component->createWithInitialProperties(dimensions));
    if (surface == nullptr) {
        error = QStringLiteral("PageSurface 无法创建");
        return false;
    }
    canvas = surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    scroller = surface->findChild<QObject*>(QStringLiteral("documentScroller"));
    selection_drag = surface->findChild<QObject*>(QStringLiteral("documentSelectionDrag"));
    pointer_handler = surface->findChild<QObject*>(QStringLiteral("documentCanvasPointerHandler"));
    if (canvas == nullptr || scroller == nullptr || selection_drag == nullptr ||
        pointer_handler == nullptr) {
        error = QStringLiteral("PageSurface 缺少文档画布或指针处理子项");
        return false;
    }
    window.resize(size);
    surface->setParentItem(window.contentItem());
    window.show();
    if (!QTest::qWaitForWindowExposed(&window, 3000)) {
        error = QStringLiteral("PageSurface 窗口未在 3000ms 内暴露");
        return false;
    }
    layout_view_model =
        engine->singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    if (layout_view_model == nullptr) {
        error = QStringLiteral("LayoutViewModel 单例不可用");
        return false;
    }
    return true;
}

bool prepareDocumentInteractionFixture(DocumentInteractionFixture& fixture, QSize size = {500, 500},
                                       const QString& active_theme = QStringLiteral("light"),
                                       bool sentinel_theme_colors = false) {
    if (!installBlankInteractionRuntime(&fixture.error)) {
        return false;
    }
    return fixture.initialize(size, active_theme, sentinel_theme_colors);
}

bool waitForDocumentGeneration(DocumentInteractionFixture& fixture, const QString& generation) {
    QElapsedTimer timer;
    timer.start();
    while (fixture.layout_view_model->documentGeneration() != generation) {
        if (timer.elapsed() >= 3000) {
            fixture.error = QStringLiteral("文档代际未在 3000ms 内到达 %1（当前 %2）")
                                .arg(generation, fixture.layout_view_model->documentGeneration());
            return false;
        }
        QTest::qWait(10);
    }
    return true;
}

bool requireValidInteractionResult(const QVariantMap& result, const QString& action,
                                   DocumentInteractionFixture& fixture) {
    if (result.value(QStringLiteral("valid")).toBool()) {
        return true;
    }
    fixture.error =
        QStringLiteral("%1 失败：%2").arg(action, result.value(QStringLiteral("error")).toString());
    return false;
}

bool clickDocumentPageCenter(DocumentInteractionFixture& fixture) {
    const QSizeF page_size = fixture.canvas->pageSizesPt().front().toSizeF();
    const QPointF page_center(std::min(100.0, page_size.width()), 140.0);
    const QPointF content_point = fixture.canvas->documentToContent(0, page_center);
    const QPointF window_point =
        fixture.canvas->mapToItem(fixture.window.contentItem(), content_point);
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QElapsedTimer timer;
    timer.start();
    while (!fixture.canvas->hasActiveFocus()) {
        if (timer.elapsed() >= 3000) {
            fixture.error = QStringLiteral("画布未在 3000ms 内获得焦点");
            return false;
        }
        QTest::qWait(10);
    }
    return true;
}

struct DocumentDragStops {
    QVariantMap start;
    QVariantMap preview_start;
    QVariantMap end;
    bool valid = false;
};

DocumentDragStops findDocumentDragStops(DocumentInteractionFixture& fixture) {
    DocumentDragStops stops;
    const QVariantMap hit_map = fixture.layout_view_model->dragHitMap(0);
    if (!hit_map.value(QStringLiteral("valid")).toBool()) {
        fixture.error = QStringLiteral("拖选命中图失败：%1")
                            .arg(hit_map.value(QStringLiteral("error")).toString());
        return stops;
    }
    for (const QVariant& value : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = value.toMap();
        const QString offset = stop.value(QStringLiteral("position"))
                                   .toMap()
                                   .value(QStringLiteral("offsetUtf8"))
                                   .toString();
        if (offset == QStringLiteral("1") && stops.start.isEmpty()) {
            stops.start = stop;
        }
        if (offset == QStringLiteral("0") && stops.preview_start.isEmpty()) {
            stops.preview_start = stop;
        }
        if (offset == QStringLiteral("4")) {
            stops.end = stop;
        }
    }
    stops.valid = !stops.start.isEmpty() && !stops.preview_start.isEmpty() && !stops.end.isEmpty();
    if (!stops.valid) {
        fixture.error = QStringLiteral("拖选命中图缺少 0/1/4 字节端点");
    }
    return stops;
}

QPointF documentStopPoint(DocumentInteractionFixture& fixture, const QVariantMap& stop) {
    return fixture.canvas->documentToContent(
        stop.value(QStringLiteral("pageIndex")).toInt(),
        QPointF(stop.value(QStringLiteral("xPt")).toReal(),
                stop.value(QStringLiteral("y0Pt")).toReal() + 3.0));
}

QPointF documentStopWindowPoint(DocumentInteractionFixture& fixture, const QVariantMap& stop) {
    return fixture.canvas->mapToItem(fixture.window.contentItem(),
                                     documentStopPoint(fixture, stop));
}

bool renderedGlyphHasInk(DocumentInteractionFixture& fixture, qsizetype placement_index) {
    const QImage frame = fixture.window.grabWindow();
    if (frame.isNull()) {
        return false;
    }
    const QVariantMap current_frame = fixture.canvas->glyphFrames().first().toMap();
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
    const qreal image_scale = fixture.canvas->zoom() * 96.0 / 72.0 /
                              current_frame.value(QStringLiteral("rasterPxPerPt")).toReal();
    const QPointF baseline = fixture.canvas->mapToItem(
        fixture.window.contentItem(),
        fixture.canvas->documentToContent(
            0, QPointF(placement.value(QStringLiteral("xPt")).toReal(),
                       placement.value(QStringLiteral("baselineYPt")).toReal())));
    const QRectF glyph_rect(
        baseline.x() + (image.value(QStringLiteral("left")).toReal() - phase_x) * image_scale,
        baseline.y() + (-image.value(QStringLiteral("top")).toReal() - phase_y) * image_scale,
        image.value(QStringLiteral("width")).toReal() * image_scale,
        image.value(QStringLiteral("height")).toReal() * image_scale);
    const qreal pixels_per_x = static_cast<qreal>(frame.width()) / fixture.window.width();
    const qreal pixels_per_y = static_cast<qreal>(frame.height()) / fixture.window.height();
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
}

QRect windowPixelRectForDocumentRect(DocumentInteractionFixture& fixture, int page_index,
                                     const QRectF& page_rect, const QImage& frame) {
    const QPointF top_left = fixture.canvas->mapToItem(
        fixture.window.contentItem(),
        fixture.canvas->documentToContent(page_index, page_rect.topLeft()));
    const QPointF bottom_right = fixture.canvas->mapToItem(
        fixture.window.contentItem(),
        fixture.canvas->documentToContent(page_index, page_rect.bottomRight()));
    const qreal pixels_per_x = static_cast<qreal>(frame.width()) / fixture.window.width();
    const qreal pixels_per_y = static_cast<qreal>(frame.height()) / fixture.window.height();
    return QRect(qFloor(top_left.x() * pixels_per_x), qFloor(top_left.y() * pixels_per_y),
                 qCeil((bottom_right.x() - top_left.x()) * pixels_per_x),
                 qCeil((bottom_right.y() - top_left.y()) * pixels_per_y))
        .intersected(frame.rect());
}

qsizetype exactColorPixels(const QImage& frame, const QRect& rect, const QColor& color) {
    qsizetype count = 0;
    const QRect bounded = rect.intersected(frame.rect());
    for (int y = bounded.top(); y <= bounded.bottom(); ++y) {
        for (int x = bounded.left(); x <= bounded.right(); ++x) {
            if (frame.pixelColor(x, y) == color) {
                ++count;
            }
        }
    }
    return count;
}

bool dashedThemeColorHasSeparatedRuns(DocumentInteractionFixture& fixture,
                                      const QVariantMap& guideline, const QColor& color,
                                      const QImage& frame, int tolerance = 96) {
    const int page_index = guideline.value(QStringLiteral("pageIndex")).toInt();
    const qreal x0 = guideline.value(QStringLiteral("x0Pt")).toReal();
    const qreal x1 = guideline.value(QStringLiteral("x1Pt")).toReal();
    const qreal y0 = guideline.value(QStringLiteral("y0Pt")).toReal();
    const qreal y1 = guideline.value(QStringLiteral("y1Pt")).toReal();
    const auto matches_token = [&frame, &color, tolerance](int x, int y) {
        const QColor pixel = frame.pixelColor(x, y);
        return pixel.alpha() > 128 && std::abs(pixel.red() - color.red()) <= tolerance &&
               std::abs(pixel.green() - color.green()) <= tolerance &&
               std::abs(pixel.blue() - color.blue()) <= tolerance;
    };
    const auto has_separated_runs = [](int first, int last, const auto& is_colored) {
        bool saw_color = false;
        int gap = 0;
        for (int offset = first; offset <= last; ++offset) {
            if (is_colored(offset)) {
                if (gap >= 2) {
                    return true;
                }
                saw_color = true;
                gap = 0;
            } else if (saw_color) {
                ++gap;
            }
        }
        return false;
    };
    // 沿单边扫描并略过转角，避免把矩形两条实线边之间的空白误认为虚线间隙。
    constexpr qreal kEdgeTolerancePt = 3.0;
    constexpr qreal kCornerInsetPt = 4.0;
    for (const qreal y_pt : {y0, y1}) {
        const QRect pixels = windowPixelRectForDocumentRect(
            fixture, page_index,
            QRectF(QPointF(x0 + kCornerInsetPt, y_pt - kEdgeTolerancePt),
                   QPointF(x1 - kCornerInsetPt, y_pt + kEdgeTolerancePt)),
            frame);
        for (int y = pixels.top(); y <= pixels.bottom(); ++y) {
            if (has_separated_runs(pixels.left(), pixels.right(),
                                   [&](int x) { return matches_token(x, y); })) {
                return true;
            }
        }
    }
    for (const qreal x_pt : {x0, x1}) {
        const QRect pixels = windowPixelRectForDocumentRect(
            fixture, page_index,
            QRectF(QPointF(x_pt - kEdgeTolerancePt, y0 + kCornerInsetPt),
                   QPointF(x_pt + kEdgeTolerancePt, y1 - kCornerInsetPt)),
            frame);
        for (int x = pixels.left(); x <= pixels.right(); ++x) {
            if (has_separated_runs(pixels.top(), pixels.bottom(),
                                   [&](int y) { return matches_token(x, y); })) {
                return true;
            }
        }
    }
    return false;
}

bool waitForGlyphFrame(DocumentInteractionFixture& fixture, const QString& generation,
                       qsizetype minimum_placements = 1) {
    QElapsedTimer timer;
    timer.start();
    while (true) {
        const QVariantList frames = fixture.canvas->glyphFrames();
        if (!frames.isEmpty()) {
            const QVariantMap frame = frames.first().toMap();
            if (frame.value(QStringLiteral("documentGeneration")).toString() == generation &&
                frame.value(QStringLiteral("placements")).toList().size() >= minimum_placements) {
                return true;
            }
        }
        if (timer.elapsed() >= 3000) {
            fixture.error = QStringLiteral("glyph 帧未在 3000ms 内到达代际 %1").arg(generation);
            return false;
        }
        QTest::qWait(10);
    }
}

// 通过画布键盘/公开 ViewModel 命令把空白文档重放到目标文档代际，供各行为契约
// slot 建立独立前置状态；重放步骤本身不重复断言，由拥有该行为的 slot 断言。
bool clickDocumentForTyping(DocumentInteractionFixture& fixture) {
    if (fixture.canvas->hasActiveFocus()) {
        return true;
    }
    return clickDocumentPageCenter(fixture);
}

bool replayDocumentToGeneration(DocumentInteractionFixture& fixture, int generation) {
    if (generation <= 0) {
        return true;
    }
    if (!clickDocumentForTyping(fixture)) {
        return false;
    }
    QTest::keyClick(&fixture.window, Qt::Key_X);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("1"))) {
        return false;
    }
    if (generation == 1) {
        return true;
    }
    if (!requireValidInteractionResult(fixture.layout_view_model->insertText(QStringLiteral("！")),
                                       QStringLiteral("插入全角叹号"), fixture)) {
        return false;
    }
    if (!waitForDocumentGeneration(fixture, QStringLiteral("2"))) {
        return false;
    }
    if (generation == 2) {
        return true;
    }
    QTest::keyClick(&fixture.window, Qt::Key_Return);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("3"))) {
        return false;
    }
    if (generation == 3) {
        return true;
    }
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("4"))) {
        return false;
    }
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("5"))) {
        return false;
    }
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("6"))) {
        return false;
    }
    if (generation == 6) {
        return true;
    }
    QTest::keyClick(&fixture.window, Qt::Key_Return, Qt::ShiftModifier);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("7"))) {
        return false;
    }
    if (generation == 7) {
        return true;
    }
    if (!requireValidInteractionResult(fixture.layout_view_model->insertText(QStringLiteral("abc")),
                                       QStringLiteral("插入 abc"), fixture)) {
        return false;
    }
    if (!waitForDocumentGeneration(fixture, QStringLiteral("8"))) {
        return false;
    }
    if (generation == 8) {
        return true;
    }
    DocumentDragStops stops = findDocumentDragStops(fixture);
    if (!stops.valid) {
        return false;
    }
    if (!requireValidInteractionResult(
            fixture.layout_view_model->setSelection(stops.start, stops.end),
            QStringLiteral("设置 1..4 字节选区"), fixture)) {
        return false;
    }
    QGuiApplication::clipboard()->setText(QStringLiteral("abc"));
    QTest::keyClick(&fixture.window, Qt::Key_V, Qt::ControlModifier);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("9"))) {
        return false;
    }
    if (generation == 9) {
        return true;
    }
    stops = findDocumentDragStops(fixture);
    if (!stops.valid) {
        return false;
    }
    if (!requireValidInteractionResult(
            fixture.layout_view_model->setSelection(stops.start, stops.end),
            QStringLiteral("重新设置 1..4 字节选区"), fixture)) {
        return false;
    }
    QGuiApplication::clipboard()->setText(QStringLiteral("left\nright"));
    QTest::keyClick(&fixture.window, Qt::Key_V, Qt::ControlModifier);
    if (!waitForDocumentGeneration(fixture, QStringLiteral("10"))) {
        return false;
    }
    return true;
}

bool prepareDocumentInteractionAt(DocumentInteractionFixture& fixture, int generation) {
    if (!prepareDocumentInteractionFixture(fixture)) {
        return false;
    }
    return replayDocumentToGeneration(fixture, generation);
}

// 富剪贴板 slot 的固定文档：正文文本 + InlineMath、Equation、Align 与尾段落。
constexpr auto kRichClipboardDocument = R"json({
    "format":"caret","version":1,
    "requiredFeatures":["caret.math.align","caret.math.content-flow.v2","caret.math.equation",
        "caret.math.inline","caret.math.syntax.v1","caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings","caret.text.default-font-size"],
    "media":[],"document":{
        "globalSettings":{},"body":[
            {"type":"paragraph","id":"paragraph-one","inlines":[
                {"type":"text","runs":[{"text":"before "}]},
                {"type":"inlineMath","id":"inline-x","mathTree":{
                    "schemaVersion":3,"root":{"id":"inline-root","content":[
                        {"type":"math","id":"inline-x-symbol","kind":"symbol","text":"x","attributes":{"atomClass":"ord"}}
                    ]}
                }},
                {"type":"text","runs":[{"text":" after"}]}
            ]},
            {"type":"equation","id":"equation-y","mathTree":{
                "schemaVersion":3,"root":{"id":"equation-root","content":[
                    {"type":"math","id":"equation-y-symbol","kind":"symbol","text":"y","attributes":{"atomClass":"ord"}}
                ]}
            }},
            {"type":"align","id":"align-one","rows":[
                {"id":"align-row","cells":[{"schemaVersion":3,
                    "root":{"id":"align-root","content":[
                        {"type":"math","id":"align-a-symbol","kind":"symbol","text":"a","attributes":{"atomClass":"ord"}}
                    ]}}]}
            ]},
            {"type":"paragraph","id":"paragraph-two","inlines":[
                {"type":"text","runs":[{"text":"tail"}]}
            ]}
        ],
        "pageSections":[{"id":"page-section","props":{}}]
    }
})json";

constexpr auto kClipboardFragmentMime = "application/vnd.caret.fragment+json";

constexpr auto kTableWithDisplayMathDocument = R"json({
    "format":"caret","version":1,"media":[],
    "requiredFeatures":["caret.math.content-flow.v2","caret.math.equation","caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1","caret.paragraph.layout-settings","caret.table.v1",
        "caret.text.default-font-size"],
    "document":{"globalSettings":{},"body":[
        {"type":"table","id":"table","rows":[{"id":"row","cells":[
            {"id":"left","blocks":[{"type":"paragraph","id":"text","inlines":[
                {"type":"text","runs":[{"text":"body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text body text"}]}]}]},
            {"id":"right","blocks":[{"type":"equation","id":"formula","mathTree":{
                "schemaVersion":3,"root":{"id":"formula-root","content":[
                    {"type":"math","id":"formula-x","kind":"symbol","text":"x",
                     "attributes":{"atomClass":"ord"}}]}}}]}
        ]}]}
    ], "pageSections":[{"id":"page-section","props":{}}]}
})json";

QString clipboard_fragment_mime_name() { return QString::fromLatin1(kClipboardFragmentMime); }

rust::String rust_string_from_bytes(const QByteArray& bytes) {
    return rust::String(bytes.constData(), static_cast<std::size_t>(bytes.size()));
}

// 用临时 .cdoc 准备真实 Rust 会话并安装布局快照与文档 runtime。
bool installClipboardDocumentRuntime(const QByteArray& document_json, QString* error) {
    QTemporaryDir directory;
    if (!directory.isValid()) {
        *error = QStringLiteral("剪贴板测试临时目录不可用");
        return false;
    }
    const QString path = directory.filePath(QStringLiteral("rich-clipboard.cdoc"));
    QFile document_file(path);
    if (!document_file.open(QIODevice::WriteOnly)) {
        *error = QStringLiteral("剪贴板测试文档无法写入");
        return false;
    }
    if (document_file.write(document_json) != document_json.size()) {
        *error = QStringLiteral("剪贴板测试文档写入不完整");
        return false;
    }
    document_file.close();

    auto runtime = caret::ffi::new_blank_document_runtime();
    const auto prepared =
        caret::ffi::prepare_document_file(rust_string_from_bytes(path.toUtf8()), 1, 0);
    if (!prepared.error.empty()) {
        *error =
            QString::fromUtf8(prepared.error.data(), static_cast<qsizetype>(prepared.error.size()));
        return false;
    }
    const rust::String installed = caret::ffi::install_prepared_document(*runtime, prepared.ticket);
    if (!installed.empty()) {
        *error = QString::fromUtf8(installed.data(), static_cast<qsizetype>(installed.size()));
        return false;
    }
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    const std::string_view snapshot_view(snapshot.data(), snapshot.size());
    if (snapshot_view.starts_with("error:")) {
        *error = QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()));
        return false;
    }
    if (!caret::install_layout_snapshot(
            QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size())))) {
        *error = QStringLiteral("剪贴板测试布局快照无法安装");
        return false;
    }
    if (!caret::install_document_runtime(std::move(runtime))) {
        *error = QStringLiteral("剪贴板测试文档 runtime 无法安装");
        return false;
    }
    return true;
}

QVariantMap findBodyDragStop(const QVariantMap& hit_map, const QString& paragraph_id, int offset) {
    for (const QVariant& value : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("kind")).toString() != QStringLiteral("body") ||
            position.value(QStringLiteral("paragraphId")).toString() != paragraph_id ||
            position.value(QStringLiteral("offsetUtf8")).toString() != QString::number(offset) ||
            stop.value(QStringLiteral("visualAffinity")).toString() !=
                QStringLiteral("downstream")) {
            continue;
        }
        return stop;
    }
    return {};
}

QString paragraph_text_by_id(const caret::LayoutViewModel& view_model,
                             const QString& paragraph_id) {
    for (const QVariant& value : view_model.paragraphLayouts()) {
        const QVariantMap paragraph = value.toMap();
        if (paragraph.value(QStringLiteral("paragraphId")).toString() == paragraph_id)
            return paragraph.value(QStringLiteral("text")).toString();
    }
    return {};
}

qsizetype replacement_char_count(const QString& text) { return text.count(QChar(0xFFFC)); }

qsizetype math_fragment_count(const caret::LayoutViewModel& view_model) {
    qsizetype total = 0;
    for (const QVariant& page : view_model.pageLayouts())
        total += page.toMap().value(QStringLiteral("mathFragments")).toList().size();
    return total;
}

QStringList math_fragment_ids(const caret::LayoutViewModel& view_model) {
    QStringList ids;
    for (const QVariant& page : view_model.pageLayouts()) {
        for (const QVariant& fragment :
             page.toMap().value(QStringLiteral("mathFragments")).toList()) {
            ids.push_back(fragment.toMap().value(QStringLiteral("mathId")).toString());
        }
    }
    return ids;
}

qsizetype inline_math_placeholder_count(const caret::LayoutViewModel& view_model) {
    qsizetype total = 0;
    for (const QVariant& value : view_model.paragraphLayouts())
        total += value.toMap().value(QStringLiteral("text")).toString().count(QChar(0xFFFC));
    return total;
}

bool select_document_paragraph_range(caret::LayoutViewModel& view_model, const QString& anchor_id,
                                     int anchor_offset, const QString& focus_id, int focus_offset) {
    const QVariantMap hit_map = view_model.dragHitMap(0);
    if (!hit_map.value(QStringLiteral("valid")).toBool())
        return false;
    const QVariantMap anchor = findBodyDragStop(hit_map, anchor_id, anchor_offset);
    const QVariantMap focus = findBodyDragStop(hit_map, focus_id, focus_offset);
    if (anchor.isEmpty() || focus.isEmpty())
        return false;
    return view_model.setSelection(anchor, focus).value(QStringLiteral("valid")).toBool();
}

void set_clipboard_fragment_payload(const QByteArray& fragment_utf8, const QString& text) {
    auto* mime = new QMimeData;
    mime->setData(clipboard_fragment_mime_name(), fragment_utf8);
    mime->setText(text);
    QGuiApplication::clipboard()->setMimeData(mime);
}

} // namespace

void ShellDocumentInteractionTest::documentHitTestReturnsRustPosition() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    QVERIFY2(fixture.surface_component->isReady(),
             qPrintable(fixture.surface_component->errorString()));
    QVERIFY(fixture.surface != nullptr);
    QVERIFY(fixture.canvas != nullptr);

    QQmlComponent probe_component(fixture.engine.get());
    probe_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item {\n"
                          "    property var hit: LayoutViewModel.hitTestDocument(0, 73, 73)\n"
                          "    property var miss: LayoutViewModel.hitTestDocument(-1, 73, 73)\n"
                          "}\n"),
        QUrl());
    QVERIFY2(probe_component.isReady(), qPrintable(probe_component.errorString()));
    QObject* probe = probe_component.create();
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

    QTRY_VERIFY_WITH_TIMEOUT(fixture.window.isExposed(), 3000);
    const QSizeF page_size = fixture.canvas->pageSizesPt().front().toSizeF();
    const QPointF page_center(std::min(100.0, page_size.width()), 140.0);
    const QPointF content_point = fixture.canvas->documentToContent(0, page_center);
    const QVariantMap page_hit = fixture.canvas->hitTestPage(content_point);
    QCOMPARE(page_hit.value(QStringLiteral("pageIndex")).toInt(), 0);
    QVERIFY(fixture.layout_view_model != nullptr);
    const QVariantMap invalid_glyph_request =
        fixture.layout_view_model->rasterizePageGlyphs(-1, 1.0, 0.0, 0.0, 0, 0, 0, 255);
    QVERIFY(!invalid_glyph_request.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid_glyph_request.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_request"));
    const QVariantMap rust_hit = fixture.layout_view_model->hitTestDocument(
        page_hit.value(QStringLiteral("pageIndex")).toInt(),
        page_hit.value(QStringLiteral("xPt")).toReal(),
        page_hit.value(QStringLiteral("yPt")).toReal());
    QVERIFY2(rust_hit.value(QStringLiteral("valid")).toBool(),
             qPrintable(rust_hit.value(QStringLiteral("error")).toString()));
}

void ShellDocumentInteractionTest::documentCanvasClickAndTypingPublishRustSnapshot() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 0), qPrintable(fixture.error));
    const QSizeF page_size = fixture.canvas->pageSizesPt().front().toSizeF();
    const QPointF page_center(std::min(100.0, page_size.width()), 140.0);
    const QPointF content_point = fixture.canvas->documentToContent(0, page_center);
    QSignalSpy position_spy(fixture.surface, SIGNAL(documentPositionHit(QVariant)));
    QVERIFY(position_spy.isValid());
    const QPointF window_point =
        fixture.canvas->mapToItem(fixture.window.contentItem(), content_point);
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(position_spy.size(), 1, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);
    const QVariantMap clicked_hit = position_spy.takeFirst().at(0).toMap();
    QVERIFY(clicked_hit.value(QStringLiteral("valid")).toBool());
    QCOMPARE(clicked_hit.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("0"));
    const qreal initial_caret_x =
        fixture.canvas->caretGeometry().value(QStringLiteral("xPt")).toReal();
    QSignalSpy text_input_spy(fixture.canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_X);
    QTRY_COMPARE_WITH_TIMEOUT(text_input_spy.size(), 1, 3000);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("x"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("1"),
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->caretGeometry().value(QStringLiteral("xPt")).toReal() >
                                 initial_caret_x,
                             3000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->glyphFrames().isEmpty() &&
                                 fixture.canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("1") &&
                                 !fixture.canvas->glyphFrames()
                                      .first()
                                      .toMap()
                                      .value(QStringLiteral("placements"))
                                      .toList()
                                      .isEmpty(),
                             3000);
    const QVariantMap updated_glyph_frame = fixture.canvas->glyphFrames().first().toMap();
    QCOMPARE(updated_glyph_frame.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("1"));
    QVERIFY(!updated_glyph_frame.value(QStringLiteral("placements")).toList().isEmpty());
}

void ShellDocumentInteractionTest::tableGuidelinesRefreshForArrowAndCellClick() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    const QVariantMap inserted =
        fixture.layout_view_model->handleEditorKey(QStringLiteral("insert-table"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().size(), 1, 3000);
    const QVariantMap first_cell = fixture.layout_view_model->tableGuidelines().front().toMap();
    const QVariantMap first_caret = fixture.canvas->caretGeometry();

    fixture.canvas->forceActiveFocus();
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);
    QSignalSpy navigation_spy(fixture.canvas, &caret::DocumentCanvas::visualNavigationRequested);
    QVERIFY(navigation_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_Right);
    QTRY_COMPARE_WITH_TIMEOUT(navigation_spy.size(), 1, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->caretGeometry() != first_caret, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().size() == 1 &&
                                 fixture.layout_view_model->tableGuidelines().front().toMap() !=
                                     first_cell,
                             3000);
    const QVariantMap second_cell = fixture.layout_view_model->tableGuidelines().front().toMap();
    QCOMPARE(fixture.canvas->tableGuidelines().front().toMap(), second_cell);

    const QPointF page_point(first_cell.value(QStringLiteral("x0Pt")).toReal() +
                                 (first_cell.value(QStringLiteral("x1Pt")).toReal() -
                                  first_cell.value(QStringLiteral("x0Pt")).toReal()) /
                                     2.0,
                             first_cell.value(QStringLiteral("y0Pt")).toReal() +
                                 (first_cell.value(QStringLiteral("y1Pt")).toReal() -
                                  first_cell.value(QStringLiteral("y0Pt")).toReal()) /
                                     2.0);
    const QPointF content_point = fixture.canvas->documentToContent(0, page_point);
    const QPointF window_point =
        fixture.canvas->mapToItem(fixture.window.contentItem(), content_point);
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().size() == 1 &&
                                 fixture.layout_view_model->tableGuidelines().front().toMap() !=
                                     second_cell,
                             3000);
    QCOMPARE(fixture.canvas->tableGuidelines().front().toMap(),
             fixture.layout_view_model->tableGuidelines().front().toMap());
    const QVariantMap focused_caret = fixture.layout_view_model->caretGeometry();
    const QVariantList focused_guidelines = fixture.layout_view_model->tableGuidelines();
    QSignalSpy stale_interaction_changed(fixture.layout_view_model,
                                         &caret::LayoutViewModel::interactionChanged);
    QVERIFY(stale_interaction_changed.isValid());
    const rust::String current_interaction =
        caret::ffi::interaction_state_json(*caret::document_runtime());
    QVERIFY(!std::string_view(current_interaction.data(), current_interaction.size())
                 .starts_with("error:"));
    QJsonObject stale_interaction =
        QJsonDocument::fromJson(QByteArray(current_interaction.data(),
                                           static_cast<qsizetype>(current_interaction.size())))
            .object();
    stale_interaction.insert(QStringLiteral("documentGeneration"), QStringLiteral("0"));
    QVERIFY(!caret::LayoutViewModelTestProbe::installInteraction(*fixture.layout_view_model,
                                                                 stale_interaction));
    QCOMPARE(fixture.layout_view_model->tableGuidelines(), focused_guidelines);
    QCOMPARE(fixture.layout_view_model->caretGeometry(), focused_caret);
    QCOMPARE(fixture.canvas->tableGuidelines(), focused_guidelines);
    QCOMPARE(stale_interaction_changed.size(), 0);

    const QVariantMap focused_cell = fixture.layout_view_model->tableGuidelines().front().toMap();
    const QPointF outside_point(focused_cell.value(QStringLiteral("x0Pt")).toReal() +
                                    (focused_cell.value(QStringLiteral("x1Pt")).toReal() -
                                     focused_cell.value(QStringLiteral("x0Pt")).toReal()) /
                                        2.0,
                                focused_cell.value(QStringLiteral("y1Pt")).toReal() + 80.0);
    const QPointF outside_content = fixture.canvas->documentToContent(0, outside_point);
    const QPointF outside_window =
        fixture.canvas->mapToItem(fixture.window.contentItem(), outside_content);
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, outside_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().isEmpty(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->tableGuidelines().isEmpty(), 3000);
}

void ShellDocumentInteractionTest::tableThemeTokensRenderAcrossZoomAndWindowFocus() {
    for (const QString& theme : {QStringLiteral("light"), QStringLiteral("dark")}) {
        DocumentInteractionFixture fixture;
        QVERIFY2(prepareDocumentInteractionFixture(fixture, QSize(600, 500), theme, true),
                 qPrintable(fixture.error));
        fixture.window.requestActivate();
        QTRY_VERIFY_WITH_TIMEOUT(fixture.window.isActive(), 3000);
        fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
        QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);
        const QColor guideline_color = theme == QStringLiteral("light")
                                           ? QColor(QStringLiteral("#e000c0"))
                                           : QColor(QStringLiteral("#20c0ff"));
        const QColor highlight_color = theme == QStringLiteral("light")
                                           ? QColor(QStringLiteral("#00d060"))
                                           : QColor(QStringLiteral("#ffc020"));
        const QColor page_color = theme == QStringLiteral("light")
                                      ? QColor(QStringLiteral("#ffffff"))
                                      : QColor(QStringLiteral("#111111"));
        auto* theme_model = fixture.engine->singletonInstance<caret::ThemeViewModel*>(
            "Caret.ViewModels", "ThemeViewModel");
        if (theme_model == nullptr)
            QFAIL("the page surface requires a theme view model");
        QCOMPARE(theme_model->activeTheme(), theme);

        const QVariantMap inserted =
            fixture.layout_view_model->handleEditorKey(QStringLiteral("insert-table"));
        QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
                 qPrintable(inserted.value(QStringLiteral("error")).toString()));
        QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->tableGuidelines().size(), 1, 3000);
        QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->blockHighlightRects().size(), 1, 3000);
        QCOMPARE(fixture.canvas->tableGuidelineColor(), guideline_color);
        QCOMPARE(fixture.canvas->blockHighlightColor(), highlight_color);
        QCOMPARE(fixture.canvas->pageColor(), page_color);

        const QVariantMap guideline = fixture.canvas->tableGuidelines().front().toMap();
        const QVariantMap highlight = fixture.canvas->blockHighlightRects().front().toMap();
        QCOMPARE(guideline.value(QStringLiteral("pageIndex")).toInt(), 0);
        QCOMPARE(highlight.value(QStringLiteral("pageIndex")).toInt(), 0);
        for (const qreal zoom : {1.0, 2.0}) {
            fixture.canvas->setZoom(zoom);
            QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->zoom(), zoom, 3000);
            QTest::qWait(50);
            const QImage frame = fixture.window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            QVERIFY(!frame.isNull());
            const QRectF highlight_rect(QPointF(highlight.value(QStringLiteral("x0Pt")).toReal(),
                                                highlight.value(QStringLiteral("y0Pt")).toReal()),
                                        QPointF(highlight.value(QStringLiteral("x1Pt")).toReal(),
                                                highlight.value(QStringLiteral("y1Pt")).toReal()));
            const QRect highlight_pixels =
                windowPixelRectForDocumentRect(fixture, 0, highlight_rect, frame);
            QVERIFY2(exactColorPixels(frame, highlight_pixels, highlight_color) > 0,
                     "the active theme block highlight token must appear in the captured window");
            QVERIFY2(dashedThemeColorHasSeparatedRuns(fixture, guideline, guideline_color, frame),
                     "the active theme table token must render separated dashed segments");
        }

        const QVariantList active_cell = fixture.layout_view_model->tableGuidelines();
        const QVariantMap active_caret = fixture.layout_view_model->caretGeometry();
        const QVariantList active_highlights = fixture.canvas->blockHighlightRects();
        auto* other_focus = new QQuickItem(fixture.window.contentItem());
        other_focus->forceActiveFocus(Qt::OtherFocusReason);
        QTRY_VERIFY_WITH_TIMEOUT(other_focus->hasActiveFocus(), 3000);
        QVERIFY(!fixture.canvas->hasActiveFocus());
        fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
        QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);
        QCOMPARE(fixture.layout_view_model->tableGuidelines(), active_cell);
        QCOMPARE(fixture.canvas->tableGuidelines(), active_cell);
        QCOMPARE(fixture.layout_view_model->caretGeometry(), active_caret);
        delete other_focus;

        QQuickWindow other_window;
        other_window.resize(200, 200);
        other_window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&other_window, 3000));
        other_window.requestActivate();
        QTRY_VERIFY_WITH_TIMEOUT(other_window.isActive(), 3000);
        QTRY_VERIFY_WITH_TIMEOUT(!fixture.window.isActive(), 3000);
        QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->hasActiveFocus(), 3000);
        QCOMPARE(fixture.canvas->tableGuidelines(), active_cell);
        QCOMPARE(fixture.canvas->blockHighlightRects(), active_highlights);
        fixture.window.requestActivate();
        QTRY_VERIFY_WITH_TIMEOUT(fixture.window.isActive(), 3000);
        fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
        QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);
        QCOMPARE(fixture.layout_view_model->tableGuidelines(), active_cell);
        QCOMPARE(fixture.canvas->tableGuidelines(), active_cell);
        QCOMPARE(fixture.layout_view_model->caretGeometry(), active_caret);
        QTRY_VERIFY_WITH_TIMEOUT(
            dashedThemeColorHasSeparatedRuns(
                fixture, guideline, guideline_color,
                fixture.window.grabWindow().convertToFormat(QImage::Format_ARGB32)),
            3000);
    }
}

void ShellDocumentInteractionTest::tablePaintUsesBuiltinThemeColors() {
    const rust::String catalog = caret::ffi::builtin_theme_catalog();
    const QByteArray catalog_bytes(catalog.data(), static_cast<qsizetype>(catalog.size()));
    const QJsonDocument builtin_catalog = QJsonDocument::fromJson(catalog_bytes);
    QVERIFY(builtin_catalog.isObject());
    const QScopeGuard restore_theme(
        [&catalog_bytes] { (void)caret::install_theme_catalog(catalog_bytes); });
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture, QSize(920, 500)),
             qPrintable(fixture.error));
    const QVariantMap inserted =
        fixture.layout_view_model->handleEditorKey(QStringLiteral("insert-table"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->tableGuidelines().size(), 1, 3000);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->blockHighlightRects().size(), 1, 3000);
    const QVariantMap guideline = fixture.canvas->tableGuidelines().front().toMap();
    const QVariantMap highlight = fixture.canvas->blockHighlightRects().front().toMap();
    const QRectF highlight_rect(QPointF(highlight.value(QStringLiteral("x0Pt")).toReal(),
                                        highlight.value(QStringLiteral("y0Pt")).toReal()),
                                QPointF(highlight.value(QStringLiteral("x1Pt")).toReal(),
                                        highlight.value(QStringLiteral("y1Pt")).toReal()));
    const int page_index = highlight.value(QStringLiteral("pageIndex")).toInt();
    const auto grid_lines = [&fixture] {
        return fixture.canvas->glyphFrames()
            .value(0)
            .toMap()
            .value(QStringLiteral("shapes"))
            .toList();
    };
    QTRY_VERIFY_WITH_TIMEOUT(!grid_lines().isEmpty(), 3000);
    const QVariantMap grid_line = grid_lines().front().toMap();
    QVERIFY(grid_line.value(QStringLiteral("strokeEnabled")).toBool());
    const QColor grid_color(grid_line.value(QStringLiteral("strokeRed")).toInt(),
                            grid_line.value(QStringLiteral("strokeGreen")).toInt(),
                            grid_line.value(QStringLiteral("strokeBlue")).toInt());
    for (const QString& theme : {QStringLiteral("light"), QStringLiteral("dark")}) {
        QJsonObject active_catalog = builtin_catalog.object();
        active_catalog.insert(QStringLiteral("active"), theme);
        QVERIFY(caret::install_theme_catalog(QJsonDocument(active_catalog).toJson()));
        const QJsonObject tokens =
            active_catalog.value(QStringLiteral("themes")).toObject().value(theme).toObject();
        const auto token_color = [&tokens](const QString& name) {
            return QColor::fromString(
                tokens.value(name).toObject().value(QStringLiteral("value")).toString());
        };
        const QColor guideline_color = token_color(QStringLiteral("colorTableGuideline"));
        const QColor highlight_color =
            token_color(QStringLiteral("colorBlockEnvironmentHighlight"));
        QVERIFY(guideline_color.isValid());
        QVERIFY(highlight_color.isValid());
        QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->tableGuidelineColor(), guideline_color, 3000);
        QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->blockHighlightColor(), highlight_color, 3000);
        QVERIFY(highlight_color != guideline_color);
        QVERIFY(highlight_color != fixture.canvas->pageColor());
        // 生产主题的边缘像素可能混色，给虚线与实线各保留互不重叠的采样范围。
        constexpr int kEdgeTolerance = 12;
        QVERIFY2(std::abs(guideline_color.lightness() - grid_color.lightness()) >
                     2 * kEdgeTolerance,
                 "the active cell dashes must be distinguishable from the solid grid");
        const auto capture = fixture.surface->grabToImage();
        if (capture.isNull())
            QFAIL("the page surface must support frame capture");
        QTRY_VERIFY_WITH_TIMEOUT(!capture->image().isNull(), 3000);
        const QImage frame = capture->image().convertToFormat(QImage::Format_ARGB32);
        const QRect pixels =
            windowPixelRectForDocumentRect(fixture, page_index, highlight_rect, frame);
        QVERIFY(exactColorPixels(frame, pixels, highlight_color) > 0);
        QVERIFY(dashedThemeColorHasSeparatedRuns(fixture, guideline, guideline_color, frame,
                                                 kEdgeTolerance));
    }
}

void ShellDocumentInteractionTest::tableBoundaryCaretAndShiftArrowsUseWholeGrid() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture, QSize(920, 500)),
             qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    QVERIFY(model->insertText(QStringLiteral("before")).value(QStringLiteral("valid")).toBool());
    QVERIFY(model->handleEditorKey(QStringLiteral("insert-table"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QVariantList table_rects = model->blockHighlightRects();
    QCOMPARE(table_rects.size(), 1);
    QVERIFY(
        model->handleEditorKey(QStringLiteral("escape")).value(QStringLiteral("valid")).toBool());
    QVERIFY(model->insertText(QStringLiteral("after")).value(QStringLiteral("valid")).toBool());
    const QString generation = model->documentGeneration();
    const QVariantMap hit_map = model->dragHitMap(0);
    QVERIFY(hit_map.value(QStringLiteral("valid")).toBool());
    const QVariantList cells = hit_map.value(QStringLiteral("cellHitRegions")).toList();
    QCOMPARE(cells.size(), 4);
    const QVariantMap table = table_rects.front().toMap();
    const qreal table_top = table.value(QStringLiteral("y0Pt")).toReal();
    const qreal table_bottom = table.value(QStringLiteral("y1Pt")).toReal();
    QVariantMap before;
    QVariantMap after;
    for (const QVariant& entry : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = entry.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("offsetUtf8")).toString() != QStringLiteral("2") ||
            !stop.value(QStringLiteral("cellId")).toString().isEmpty()) {
            continue;
        }
        if (stop.value(QStringLiteral("y1Pt")).toReal() <= table_top) {
            before = stop;
        } else if (stop.value(QStringLiteral("y0Pt")).toReal() >= table_bottom) {
            after = stop;
        }
    }
    QVERIFY(!before.isEmpty());
    QVERIFY(!after.isEmpty());
    fixture.window.requestActivate();
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);

    for (const bool forward : {true, false}) {
        const QVariantMap& stop = forward ? before : after;
        const Qt::Key key = forward ? Qt::Key_Down : Qt::Key_Up;
        const QVariantMap cell = (forward ? cells.front() : cells.back()).toMap();
        QVERIFY(model->setSelection(stop, stop).value(QStringLiteral("valid")).toBool());
        QTest::keyClick(&fixture.window, key);
        const QVariantMap caret = fixture.canvas->caretGeometry();
        QCOMPARE(caret.value(QStringLiteral("xPt")).toReal(),
                 cell.value(forward ? QStringLiteral("x0Pt") : QStringLiteral("x1Pt")).toReal());
        const qreal caret_y_sum = caret.value(QStringLiteral("y0Pt")).toReal() +
                                  caret.value(QStringLiteral("y1Pt")).toReal();
        const qreal row_y_sum = cell.value(QStringLiteral("y0Pt")).toReal() +
                                cell.value(QStringLiteral("y1Pt")).toReal();
        QVERIFY(std::abs(caret_y_sum - row_y_sum) < 0.01);

        QVERIFY(model->setSelection(stop, stop).value(QStringLiteral("valid")).toBool());
        QTest::keyClick(&fixture.window, key, Qt::ShiftModifier);
        QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->selectionRects().contains(table_rects.front()),
                                 3000);
        QCOMPARE(model->documentGeneration(), generation);
    }
}

void ShellDocumentInteractionTest::tableGuidelineUsesTheVisiblePageFragment() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString path = directory.filePath(QStringLiteral("many-row-table.cdoc"));
    QJsonArray rows;
    for (int row_index = 0; row_index < 70; ++row_index) {
        const QString row_id = QStringLiteral("row-%1").arg(row_index);
        const QString cell_id = QStringLiteral("cell-%1").arg(row_index);
        const QString paragraph_id = QStringLiteral("paragraph-%1").arg(row_index);
        const QJsonObject paragraph{{QStringLiteral("type"), QStringLiteral("paragraph")},
                                    {QStringLiteral("id"), paragraph_id},
                                    {QStringLiteral("inlines"), QJsonArray{}}};
        const QJsonObject cell{{QStringLiteral("id"), cell_id},
                               {QStringLiteral("blocks"), QJsonArray{paragraph}}};
        const QJsonObject row{{QStringLiteral("id"), row_id},
                              {QStringLiteral("cells"), QJsonArray{cell}}};
        rows.append(row);
    }
    const QJsonObject table{{QStringLiteral("type"), QStringLiteral("table")},
                            {QStringLiteral("id"), QStringLiteral("long-table")},
                            {QStringLiteral("rows"), rows}};
    const QJsonObject page_section{{QStringLiteral("id"), QStringLiteral("section")},
                                   {QStringLiteral("props"), QJsonObject{}}};
    const QJsonObject document{
        {QStringLiteral("format"), QStringLiteral("caret")},
        {QStringLiteral("version"), 1},
        {QStringLiteral("requiredFeatures"),
         QJsonArray{QStringLiteral("caret.paragraph.content-flow.v1"),
                    QStringLiteral("caret.paragraph.layout-settings"),
                    QStringLiteral("caret.table.v1"),
                    QStringLiteral("caret.text.default-font-size")}},
        {QStringLiteral("media"), QJsonArray{}},
        {QStringLiteral("document"),
         QJsonObject{{QStringLiteral("globalSettings"), QJsonObject{}},
                     {QStringLiteral("body"), QJsonArray{table}},
                     {QStringLiteral("pageSections"), QJsonArray{page_section}}}}};
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly | QIODevice::Text));
    const QByteArray bytes = QJsonDocument(document).toJson(QJsonDocument::Compact);
    QCOMPARE(file.write(bytes), bytes.size());
    file.close();

    DocumentInteractionFixture fixture;
    QVERIFY2(
        prepareDocumentInteractionFixture(fixture, QSize(600, 600), QStringLiteral("light"), true),
        qPrintable(fixture.error));
    QSignalSpy open_finished(fixture.layout_view_model,
                             &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(open_finished.isValid());
    const QVariantMap open_result = fixture.layout_view_model->openDocument(path);
    QVERIFY2(open_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_result.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(open_finished.size(), 1, 5000);
    QVERIFY2(open_finished.first().at(0).toBool(),
             qPrintable(open_finished.first().at(1).toString()));
    QVERIFY2(fixture.layout_view_model->pageLayouts().size() > 1,
             "the real table layout must produce multiple pages");

    fixture.window.requestActivate();
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);

    constexpr int visible_page = 1;
    const QPointF page_origin = fixture.canvas->pageOrigin(visible_page);
    QVERIFY(fixture.scroller->setProperty("contentY", page_origin.y()));
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->visiblePageRange().size() == 2 &&
                                 fixture.canvas->visiblePageRange().at(0).toInt() <= visible_page &&
                                 fixture.canvas->visiblePageRange().at(1).toInt() > visible_page,
                             3000);

    const QVariantMap hit_map = fixture.layout_view_model->dragHitMap(visible_page);
    QVERIFY2(hit_map.value(QStringLiteral("valid")).toBool(),
             qPrintable(hit_map.value(QStringLiteral("error")).toString()));
    const QVariantList cells = hit_map.value(QStringLiteral("cellHitRegions")).toList();
    QVERIFY(!cells.isEmpty());
    const QVariantMap cell = cells.front().toMap();
    const QPointF center((cell.value(QStringLiteral("x0Pt")).toReal() +
                          cell.value(QStringLiteral("x1Pt")).toReal()) /
                             2.0,
                         (cell.value(QStringLiteral("y0Pt")).toReal() +
                          cell.value(QStringLiteral("y1Pt")).toReal()) /
                             2.0);
    const QPointF window_point = fixture.canvas->mapToItem(
        fixture.window.contentItem(), fixture.canvas->documentToContent(visible_page, center));
    QVERIFY(QRectF(QPointF(0.0, 0.0), QSizeF(fixture.window.size())).contains(window_point));
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().size() == 1 &&
                                 fixture.layout_view_model->tableGuidelines()
                                         .front()
                                         .toMap()
                                         .value(QStringLiteral("pageIndex"))
                                         .toInt() == visible_page,
                             3000);
    const QVariantMap guideline = fixture.layout_view_model->tableGuidelines().front().toMap();
    QCOMPARE(fixture.canvas->tableGuidelines(), fixture.layout_view_model->tableGuidelines());
    const QColor guideline_color(QStringLiteral("#e000c0"));
    QCOMPARE(fixture.canvas->tableGuidelineColor(), guideline_color);

    // 请求 PageSurface 重新渲染完整视口，避免窗口截图读到软件后端的部分重绘缓冲。
    const auto capture = fixture.surface->grabToImage();
    if (capture.isNull())
        QFAIL("the visible page surface must support frame capture");
    QTRY_VERIFY_WITH_TIMEOUT(!capture->image().isNull(), 3000);
    const QImage frame = capture->image().convertToFormat(QImage::Format_ARGB32);
    QVERIFY(dashedThemeColorHasSeparatedRuns(fixture, guideline, guideline_color, frame));
}

void ShellDocumentInteractionTest::tableDragSelectsAcrossUnevenCells() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    QVERIFY(model->handleEditorKey(QStringLiteral("insert-table"))
                .value(QStringLiteral("valid"))
                .toBool());
    const auto caret_midpoint = [](const QVariantMap& caret) {
        return QPointF(caret.value(QStringLiteral("xPt")).toReal(),
                       (caret.value(QStringLiteral("y0Pt")).toReal() +
                        caret.value(QStringLiteral("y1Pt")).toReal()) /
                           2.0);
    };
    QVERIFY(model->insertText(QStringLiteral("first")).value(QStringLiteral("valid")).toBool());
    const QPointF left_start = caret_midpoint(model->caretGeometry());
    for (int line = 0; line < 3; ++line) {
        QVERIFY(model->handleEditorKey(QStringLiteral("enter"))
                    .value(QStringLiteral("valid"))
                    .toBool());
        QVERIFY(
            model->insertText(QStringLiteral("long cell")).value(QStringLiteral("valid")).toBool());
    }
    const QPointF left_end = caret_midpoint(model->caretGeometry());
    QVERIFY(model->handleEditorKey(QStringLiteral("tab")).value(QStringLiteral("valid")).toBool());
    QVERIFY(model->insertText(QStringLiteral("short")).value(QStringLiteral("valid")).toBool());
    const QVariantMap right_caret = model->caretGeometry();
    QCOMPARE(model->tableGuidelines().size(), 1);
    const QVariantMap right_cell = model->tableGuidelines().front().toMap();
    const QPointF right_blank(caret_midpoint(right_caret).x(), left_end.y());
    QVERIFY(right_blank.y() > right_caret.value(QStringLiteral("y1Pt")).toReal());
    QVERIFY(right_blank.y() < right_cell.value(QStringLiteral("y1Pt")).toReal());

    const QVariantList table_rect = model->blockHighlightRects();
    QCOMPARE(table_rect.size(), 1);
    const QString generation = model->documentGeneration();
    const auto window_point = [&fixture](QPointF point) {
        return fixture.canvas->mapToItem(fixture.window.contentItem(),
                                         fixture.canvas->documentToContent(0, point));
    };
    const QPointF start = window_point(left_start);
    const QPointF end = window_point(right_blank);
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, start.toPoint());
    QMouseEvent move(QEvent::MouseMove, end, end, fixture.window.mapToGlobal(end.toPoint()),
                     Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &move);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.pointer_handler->property("selectionDragActive").toBool(),
                             3000);
    QTRY_COMPARE_WITH_TIMEOUT(model->selectionRects(), table_rect, 3000);
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, end.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->selectionRects(), table_rect, 3000);
    QCOMPARE(model->documentGeneration(), generation);
}

void ShellDocumentInteractionTest::tableDragFromPreeditSelectsAcrossCells() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    QVERIFY(model->handleEditorKey(QStringLiteral("insert-table"))
                .value(QStringLiteral("valid"))
                .toBool());
    const QVariantList table_rect = model->blockHighlightRects();
    QCOMPARE(table_rect.size(), 1);
    const QString generation = model->documentGeneration();
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    const QString preedit_text = QStringLiteral("nihong");
    QInputMethodEvent preedit(preedit_text, {});
    QCoreApplication::sendEvent(fixture.canvas, &preedit);
    QTRY_VERIFY_WITH_TIMEOUT(!model->compositionRects().isEmpty(), 3000);

    const auto stops = model->dragHitMap(0).value(QStringLiteral("stops")).toList();
    const QString preedit_end = QString::number(preedit_text.toUtf8().size());
    QVariantMap preedit_stop;
    for (const QVariant& stop_value : stops) {
        const auto stop = stop_value.toMap();
        const auto position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("offsetUtf8")) == preedit_end) {
            preedit_stop = stop;
            break;
        }
    }
    QVERIFY(!preedit_stop.isEmpty());
    const QString preedit_cell = preedit_stop.value(QStringLiteral("cellId")).toString();
    QVERIFY(!preedit_cell.isEmpty());
    QVariantMap other_cell_stop;
    for (const QVariant& stop_value : stops) {
        const auto stop = stop_value.toMap();
        const QString cell = stop.value(QStringLiteral("cellId")).toString();
        if (!cell.isEmpty() && cell != preedit_cell) {
            other_cell_stop = stop;
            break;
        }
    }
    QVERIFY(!other_cell_stop.isEmpty());

    const QPointF start = documentStopWindowPoint(fixture, preedit_stop);
    const QPointF end = documentStopWindowPoint(fixture, other_cell_stop);
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, start.toPoint());
    QMouseEvent move(QEvent::MouseMove, end, end, fixture.window.mapToGlobal(end.toPoint()),
                     Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &move);
    QTRY_COMPARE_WITH_TIMEOUT(model->selectionRects(), table_rect, 3000);
    QVERIFY(model->compositionRects().isEmpty());
    QVERIFY(fixture.canvas->preeditText().isEmpty());
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, end.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->selectionRects(), table_rect, 3000);
    QCOMPARE(model->documentGeneration(), generation);
}

void ShellDocumentInteractionTest::textDragFromPreeditKeepsTheMappedAnchor() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    const QString source = QStringLiteral("abcdefghijklmnopqrstuvwxyz");
    QVERIFY(model->insertText(source).value(QStringLiteral("valid")).toBool());
    const QString paragraph_id =
        model->paragraphLayouts().first().toMap().value(QStringLiteral("paragraphId")).toString();
    const int anchor_offset = 3;
    const QVariantMap original_anchor =
        findBodyDragStop(model->dragHitMap(0), paragraph_id, anchor_offset);
    QVERIFY(!original_anchor.isEmpty());
    QVERIFY(model->setSelection(original_anchor, original_anchor)
                .value(QStringLiteral("valid"))
                .toBool());
    const QString generation = model->documentGeneration();
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    const QString preedit_text = QStringLiteral("12345");
    QInputMethodEvent preedit(preedit_text, {});
    QCoreApplication::sendEvent(fixture.canvas, &preedit);
    QTRY_VERIFY_WITH_TIMEOUT(!model->compositionRects().isEmpty(), 3000);

    const QVariantMap hit_map = model->dragHitMap(0);
    const int preedit_length = static_cast<int>(preedit_text.toUtf8().size());
    const int source_length = static_cast<int>(source.toUtf8().size());
    const QVariantMap preview_anchor =
        findBodyDragStop(hit_map, paragraph_id, anchor_offset + preedit_length);
    const QVariantMap preview_focus =
        findBodyDragStop(hit_map, paragraph_id, source_length + preedit_length);
    QVERIFY(!preview_anchor.isEmpty());
    QVERIFY(!preview_focus.isEmpty());
    const QPointF start = documentStopWindowPoint(fixture, preview_anchor);
    const QPointF end = documentStopWindowPoint(fixture, preview_focus);
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, start.toPoint());
    QMouseEvent move(QEvent::MouseMove, end, end, fixture.window.mapToGlobal(end.toPoint()),
                     Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &move);
    QTRY_VERIFY_WITH_TIMEOUT(!model->selectionRects().isEmpty(), 3000);
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, end.toPoint());
    const QVariantMap input_context = model->inputMethodContext();
    QCOMPARE(input_context.value(QStringLiteral("anchorPosition")).toInt(), anchor_offset);
    QCOMPARE(input_context.value(QStringLiteral("cursorPosition")).toInt(), source_length);
    QCOMPARE(input_context.value(QStringLiteral("surroundingText")).toString(), source);
    QCOMPARE(model->documentGeneration(), generation);
}

void ShellDocumentInteractionTest::tableDragSelectsACellContainingOnlyDisplayMath() {
    DocumentInteractionFixture fixture;
    QVERIFY2(
        installClipboardDocumentRuntime(QByteArray(kTableWithDisplayMathDocument), &fixture.error),
        qPrintable(fixture.error));
    QVERIFY2(fixture.initialize({500, 500}), qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    const QVariantMap hit_map = model->dragHitMap(0);
    const QVariantMap body_stop = findBodyDragStop(hit_map, QStringLiteral("text"), 0);
    QVERIFY(!body_stop.isEmpty());
    QVERIFY(model->setSelection(body_stop, body_stop).value(QStringLiteral("valid")).toBool());
    const QVariantList table_rect = model->blockHighlightRects();
    QCOMPARE(table_rect.size(), 1);
    QVariantMap formula_cell;
    for (const QVariant& entry : hit_map.value(QStringLiteral("cellHitRegions")).toList()) {
        const auto region = entry.toMap();
        if (region.value(QStringLiteral("cellId")) == QStringLiteral("right")) {
            formula_cell = region;
            break;
        }
    }
    QVERIFY(!formula_cell.isEmpty());
    const QPointF cell_point(formula_cell.value(QStringLiteral("x0Pt")).toReal() + 6.0,
                             formula_cell.value(QStringLiteral("y1Pt")).toReal() - 3.0);
    bool has_formula_boundary = false;
    for (const QVariant& entry : hit_map.value(QStringLiteral("stops")).toList()) {
        const auto stop = entry.toMap();
        if (stop.value(QStringLiteral("cellId")) == QStringLiteral("right") &&
            stop.value(QStringLiteral("domainId")) == QStringLiteral("body")) {
            has_formula_boundary = true;
            QVERIFY(stop.value(QStringLiteral("y1Pt")).toReal() < cell_point.y());
        }
    }
    QVERIFY(has_formula_boundary);
    const QPointF start = documentStopWindowPoint(fixture, body_stop);
    const QPointF end = fixture.canvas->mapToItem(fixture.window.contentItem(),
                                                  fixture.canvas->documentToContent(0, cell_point));
    const QString generation = model->documentGeneration();
    for (const bool forward : {true, false}) {
        QVERIFY(model->clearSelection().value(QStringLiteral("valid")).toBool());
        const QPointF anchor = forward ? start : end;
        const QPointF focus = forward ? end : start;
        QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, anchor.toPoint());
        QMouseEvent move(QEvent::MouseMove, focus, focus,
                         fixture.window.mapToGlobal(focus.toPoint()), Qt::NoButton, Qt::LeftButton,
                         Qt::NoModifier);
        QCoreApplication::sendEvent(&fixture.window, &move);
        QTRY_COMPARE_WITH_TIMEOUT(model->selectionRects(), table_rect, 3000);
        QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, focus.toPoint());
        QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->selectionRects(), table_rect, 3000);
    }
    QCOMPARE(model->documentGeneration(), generation);
}

void ShellDocumentInteractionTest::tableShapeShortcutsMutateThroughTheFocusedViewModel() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    const QVariantMap inserted =
        fixture.layout_view_model->handleEditorKey(QStringLiteral("insert-table"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->tableGuidelines().size(), 1, 3000);
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);

    // QKeyEvent 经焦点画布、QML 信号处理器和 LayoutViewModel 进入 Rust；每次表格结构编辑
    // 安装后都会推进文档代际。
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    const auto send_and_wait_for_edit =
        [&fixture, &editor_key_spy](Qt::Key key, Qt::KeyboardModifiers modifiers,
                                    const QString& expected_intent) {
            const QString previous_generation = fixture.layout_view_model->documentGeneration();
            QTest::keyClick(&fixture.window, key, modifiers);
            QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
            QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), expected_intent);
            QTRY_VERIFY_WITH_TIMEOUT(
                fixture.layout_view_model->documentGeneration() != previous_generation, 3000);
        };
#ifdef Q_OS_MACOS
    const Qt::KeyboardModifiers insert_modifiers =
        caret::keyboard::physicalControlModifier() | Qt::AltModifier | Qt::ShiftModifier;
    send_and_wait_for_edit(Qt::Key_Down, insert_modifiers,
                           QStringLiteral("table-insert-row-after"));
    send_and_wait_for_edit(Qt::Key_Right, insert_modifiers,
                           QStringLiteral("table-insert-col-after"));
#else
    send_and_wait_for_edit(Qt::Key_Down, Qt::AltModifier, QStringLiteral("table-insert-row-after"));
    const Qt::KeyboardModifiers alternate_modifiers =
        caret::keyboard::physicalControlModifier() | Qt::AltModifier | Qt::ShiftModifier;
    send_and_wait_for_edit(Qt::Key_Down, alternate_modifiers,
                           QStringLiteral("table-insert-row-after"));
    send_and_wait_for_edit(Qt::Key_Right, Qt::AltModifier,
                           QStringLiteral("table-insert-col-after"));
    send_and_wait_for_edit(Qt::Key_Right, alternate_modifiers,
                           QStringLiteral("table-insert-col-after"));
#endif
    const Qt::KeyboardModifiers delete_row_modifiers = Qt::AltModifier | Qt::ShiftModifier;
    const Qt::KeyboardModifiers delete_column_modifiers = Qt::AltModifier;
    send_and_wait_for_edit(Qt::Key_Backspace, delete_row_modifiers,
                           QStringLiteral("table-delete-row-backspace"));
    send_and_wait_for_edit(Qt::Key_Delete, delete_row_modifiers,
                           QStringLiteral("table-delete-row-delete"));
    send_and_wait_for_edit(Qt::Key_Backspace, delete_column_modifiers,
                           QStringLiteral("table-delete-column-backspace"));
    send_and_wait_for_edit(Qt::Key_Delete, delete_column_modifiers,
                           QStringLiteral("table-delete-column-delete"));
}

void ShellDocumentInteractionTest::documentCanvasRasterizesTypedGlyphWithInk() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 1), qPrintable(fixture.error));
    QVERIFY2(waitForGlyphFrame(fixture, QStringLiteral("1")), qPrintable(fixture.error));
    QTRY_VERIFY_WITH_TIMEOUT(renderedGlyphHasInk(fixture, 0), 3000);
}

void ShellDocumentInteractionTest::documentFontFallbackPreservesLatinBaseline() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 1), qPrintable(fixture.error));
    QVERIFY2(waitForGlyphFrame(fixture, QStringLiteral("1")), qPrintable(fixture.error));
    const QVariantMap latin_glyph_frame = fixture.canvas->glyphFrames().first().toMap();
    const qreal latin_baseline_before_fallback =
        latin_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    const QVariantMap fullwidth_insert =
        fixture.layout_view_model->insertText(QStringLiteral("！"));
    QVERIFY2(fullwidth_insert.value(QStringLiteral("valid")).toBool(),
             qPrintable(fullwidth_insert.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("2"),
                              3000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->glyphFrames().isEmpty() &&
                                 fixture.canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("2") &&
                                 fixture.canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("placements"))
                                         .toList()
                                         .size() >= 2,
                             3000);
    const QVariantMap mixed_glyph_frame = fixture.canvas->glyphFrames().first().toMap();
    const qreal latin_baseline_after_fallback =
        mixed_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    QCOMPARE(latin_baseline_after_fallback, latin_baseline_before_fallback);
    QTRY_VERIFY_WITH_TIMEOUT(renderedGlyphHasInk(fixture, 1), 3000);
}

void ShellDocumentInteractionTest::returnSplitsParagraphAndForwardsEditorKey() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 2), qPrintable(fixture.error));
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_Return);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("3"),
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts().size(), 2);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(0)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());
}

void ShellDocumentInteractionTest::inputMethodCompositionUnderlineTracksPageTransforms() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    const qulonglong composition_revision = fixture.layout_view_model->layoutRevision();
    QList<QInputMethodEvent::Attribute> composition_attributes;
    composition_attributes.push_back(
        QInputMethodEvent::Attribute(QInputMethodEvent::Cursor, 2, 0, {}));
    QInputMethodEvent composition_event(QStringLiteral("ni hao"), composition_attributes);
    QCoreApplication::sendEvent(fixture.canvas, &composition_event);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->layoutRevision(), composition_revision + 1,
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("ni hao"));
    QVERIFY(!fixture.layout_view_model->compositionRects().isEmpty());
    QQuickItem* underline = nullptr;
    for (QQuickItem* item : fixture.canvas->parentItem()->childItems()) {
        if (item->objectName() == QStringLiteral("compositionUnderline")) {
            underline = item;
            break;
        }
    }
    QVERIFY(underline != nullptr);
    const QVariantMap composition_rect =
        fixture.layout_view_model->compositionRects().first().toMap();
    const auto expected_underline_start = [&]() {
        return fixture.canvas->documentToContent(
            composition_rect.value(QStringLiteral("pageIndex")).toInt(),
            QPointF(composition_rect.value(QStringLiteral("x0Pt")).toReal(),
                    composition_rect.value(QStringLiteral("y0Pt")).toReal()));
    };
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(underline->y(), expected_underline_start().y(), 3000);
    fixture.surface->setProperty("pageScale", 1.5);
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(underline->y(), expected_underline_start().y(), 3000);
    fixture.window.resize(fixture.window.width() + 100, fixture.window.height());
    QTRY_COMPARE_WITH_TIMEOUT(underline->x(), expected_underline_start().x(), 3000);
    fixture.surface->setProperty("pageScale", 1.0);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->glyphFrames().isEmpty() &&
                                 !fixture.canvas->glyphFrames()
                                      .first()
                                      .toMap()
                                      .value(QStringLiteral("placements"))
                                      .toList()
                                      .isEmpty(),
                             3000);
}

void ShellDocumentInteractionTest::mixedCompositionPublishesOneUnderlinePerRect() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    const qulonglong composition_revision = fixture.layout_view_model->layoutRevision();
    QInputMethodEvent first_composition(QStringLiteral("ni hao"), {});
    QCoreApplication::sendEvent(fixture.canvas, &first_composition);
    QInputMethodEvent mixed_composition(QStringLiteral("hello中文hello"), {});
    QCoreApplication::sendEvent(fixture.canvas, &mixed_composition);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->layoutRevision(), composition_revision + 2,
                              3000);
    const QVariantList mixed_rects = fixture.layout_view_model->compositionRects();
    QVERIFY(!mixed_rects.isEmpty());
    QList<QQuickItem*> underlines;
    for (QQuickItem* item : fixture.canvas->parentItem()->childItems()) {
        if (item->objectName() == QStringLiteral("compositionUnderline")) {
            underlines.push_back(item);
        }
    }
    QCOMPARE(underlines.size(), mixed_rects.size());
    for (qsizetype index = 0; index < mixed_rects.size(); ++index) {
        const QVariantMap rect = mixed_rects.at(index).toMap();
        const QPointF start =
            fixture.canvas->documentToContent(rect.value(QStringLiteral("pageIndex")).toInt(),
                                              QPointF(rect.value(QStringLiteral("x0Pt")).toReal(),
                                                      rect.value(QStringLiteral("y0Pt")).toReal()));
        QCOMPARE(underlines.at(index)->x(), start.x());
        QCOMPARE(underlines.at(index)->y(), start.y());
    }
}

void ShellDocumentInteractionTest::cancelledCompositionClearsPreeditAndCompositionRects() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    const qulonglong composition_revision = fixture.layout_view_model->layoutRevision();
    QInputMethodEvent composition_event(QStringLiteral("ni hao"), {});
    QCoreApplication::sendEvent(fixture.canvas, &composition_event);
    QInputMethodEvent mixed_composition(QStringLiteral("hello中文hello"), {});
    QCoreApplication::sendEvent(fixture.canvas, &mixed_composition);
    QTest::keyClick(&fixture.window, Qt::Key_Escape);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->preeditText().isEmpty(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->compositionRects().isEmpty(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("escape"));
    QVERIFY(fixture.layout_view_model->layoutRevision() >= composition_revision + 3);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());
    QVERIFY(fixture.layout_view_model->compositionRects().isEmpty());
    const QVariantMap idle_ime_response =
        fixture.layout_view_model->inputMethodEvent(QString(), QString(), 0, 0, 0);
    QVERIFY2(idle_ime_response.value(QStringLiteral("valid")).toBool(),
             qPrintable(idle_ime_response.value(QStringLiteral("error")).toString()));
    QCOMPARE(fixture.layout_view_model->documentGeneration(), QStringLiteral("3"));
}

void ShellDocumentInteractionTest::escapeClosesCommandLineWithoutEditingTheDocument() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QKeyEvent command_start(QEvent::KeyPress, Qt::Key_Backslash, Qt::NoModifier,
                            QStringLiteral("\\"));
    QCoreApplication::sendEvent(fixture.canvas, &command_start);
    QTRY_VERIFY_WITH_TIMEOUT(
        fixture.layout_view_model->commandLine().value(QStringLiteral("active")).toBool(), 3000);
    QTest::keyClick(&fixture.window, Qt::Key_Escape);
    QTRY_VERIFY_WITH_TIMEOUT(
        !fixture.layout_view_model->commandLine().value(QStringLiteral("active")).toBool(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("escape"));
    QCOMPARE(fixture.layout_view_model->documentGeneration(), QStringLiteral("3"));
}

void ShellDocumentInteractionTest::inputMethodContextTracksCaretNavigation() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    QTest::keyClick(&fixture.window, Qt::Key_Left);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("surroundingText"))
                                  .toString(),
                              QStringLiteral("x！"), 3000);
    QCOMPARE(fixture.layout_view_model->inputMethodContext()
                 .value(QStringLiteral("cursorPosition"))
                 .toInt(),
             2);
    QTest::keyClick(&fixture.window, Qt::Key_Right);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("surroundingText"))
                                  .toString(),
                              QString(), 3000);
}

void ShellDocumentInteractionTest::backspaceMergesParagraphsAndDeletesText() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 3), qPrintable(fixture.error));
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("4"),
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts().size(), 1);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("5"),
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));
    QTest::keyClick(&fixture.window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("backspace"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("6"),
                              3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());
}

void ShellDocumentInteractionTest::shiftEnterInsertsSoftBreakWithoutSplittingParagraph() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 6), qPrintable(fixture.error));
    QSignalSpy editor_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_Return, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(editor_key_spy.size(), 1, 3000);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("shift-enter"));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("7"),
                              3000);
}

void ShellDocumentInteractionTest::shiftNavigationProjectsSelectionRects() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 7), qPrintable(fixture.error));
    const QVariantMap drag_text = fixture.layout_view_model->insertText(QStringLiteral("abc"));
    QVERIFY2(drag_text.value(QStringLiteral("valid")).toBool(),
             qPrintable(drag_text.value(QStringLiteral("error")).toString()));
    QCOMPARE(fixture.layout_view_model->documentGeneration(), QStringLiteral("8"));
    QTest::keyClick(&fixture.window, Qt::Key_Left, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("anchorPosition"))
                                  .toInt(),
                              4, 3000);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("cursorPosition"))
                                  .toInt(),
                              3, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.layout_view_model->selectionRects().isEmpty(), 3000);
    QTest::keyClick(&fixture.window, Qt::Key_Right, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("cursorPosition"))
                                  .toInt(),
                              4, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->selectionRects().isEmpty(), 3000);
}

void ShellDocumentInteractionTest::dragSelectionCommitsSameRustSelectionProjection() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 8), qPrintable(fixture.error));
    const QVariantMap hit_map = fixture.layout_view_model->dragHitMap(0);
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
    QVERIFY(fixture.scroller != nullptr);
    QVERIFY(fixture.selection_drag != nullptr);
    QVERIFY(fixture.pointer_handler != nullptr);
    const qreal drag_scroll_y = fixture.scroller->property("contentY").toReal();
    const QPointF drag_preview_start_window = documentStopWindowPoint(fixture, drag_preview_start);
    const QPointF drag_end_window = documentStopWindowPoint(fixture, drag_end);
    const QRectF ime_caret_before_drag =
        fixture.canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier,
                      drag_preview_start_window.toPoint());
    const QPoint drag_end_point = drag_end_window.toPoint();
    QMouseEvent drag_move(QEvent::MouseMove, drag_end_window, drag_end_window,
                          fixture.window.mapToGlobal(drag_end_point), Qt::NoButton, Qt::LeftButton,
                          Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.selection_drag->property("active").toBool(), 3000);
    const QVariantMap preview_anchor = fixture.pointer_handler->property("dragAnchor").toMap();
    const QVariantMap preview_focus = fixture.pointer_handler->property("dragFocus").toMap();
    QVERIFY(!preview_anchor.isEmpty());
    QVERIFY(!preview_focus.isEmpty());
    QVERIFY(preview_anchor.value(QStringLiteral("position"))
                .toMap()
                .value(QStringLiteral("offsetUtf8")) !=
            preview_focus.value(QStringLiteral("position"))
                .toMap()
                .value(QStringLiteral("offsetUtf8")));
    QTRY_COMPARE_WITH_TIMEOUT(
        fixture.canvas->caretPreviewGeometry().value(QStringLiteral("xPt")).toReal(),
        preview_focus.value(QStringLiteral("xPt")).toReal(), 3000);
    QCOMPARE(fixture.canvas->caretPreviewGeometry().value(QStringLiteral("pageIndex")).toInt(),
             preview_focus.value(QStringLiteral("pageIndex")).toInt());
    QCOMPARE(fixture.canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(),
             ime_caret_before_drag);
    // 正文拖选在拖动中直接提交同一 Rust 选区投影；预览矩形列表保持为空，画布消费
    // selectionRects，因此跨公式块/块环境也能在拖动中显示与松手后一致的高亮。
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->selectionPreviewRects().isEmpty(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.layout_view_model->selectionRects().isEmpty(), 3000);
    const QVariantList preview_rectangles = fixture.layout_view_model->selectionRects();
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.layout_view_model->selectionRects().isEmpty(), 3000);
    const QVariantList committed_rectangles = fixture.layout_view_model->selectionRects();
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
    QVERIFY(fixture.canvas->selectionPreviewRects().isEmpty());
    QVERIFY(fixture.canvas->caretPreviewGeometry().isEmpty());
    QCOMPARE(fixture.scroller->property("contentY").toReal(), drag_scroll_y);
    QCOMPARE(fixture.layout_view_model->documentGeneration(), QStringLiteral("8"));
    QCOMPARE(fixture.layout_view_model->inputMethodContext()
                 .value(QStringLiteral("anchorPosition"))
                 .toInt(),
             0);
    QCOMPARE(fixture.layout_view_model->inputMethodContext()
                 .value(QStringLiteral("cursorPosition"))
                 .toInt(),
             4);
}

void ShellDocumentInteractionTest::reverseDragMovesInputMethodCaretToFocus() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 8), qPrintable(fixture.error));
    const DocumentDragStops stops = findDocumentDragStops(fixture);
    QVERIFY2(stops.valid, qPrintable(fixture.error));
    const QPointF drag_preview_start_window = documentStopWindowPoint(fixture, stops.preview_start);
    const QPointF drag_end_window = documentStopWindowPoint(fixture, stops.end);
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QMouseEvent reverse_drag_move(QEvent::MouseMove, drag_preview_start_window,
                                  drag_preview_start_window,
                                  fixture.window.mapToGlobal(drag_preview_start_window.toPoint()),
                                  Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &reverse_drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.selection_drag->property("active").toBool(), 3000);
    const QVariantMap reverse_focus = fixture.pointer_handler->property("dragFocus").toMap();
    QCOMPARE(reverse_focus.value(QStringLiteral("position"))
                 .toMap()
                 .value(QStringLiteral("offsetUtf8"))
                 .toString(),
             QStringLiteral("0"));
    QTRY_COMPARE_WITH_TIMEOUT(
        fixture.canvas->caretPreviewGeometry().value(QStringLiteral("xPt")).toReal(),
        reverse_focus.value(QStringLiteral("xPt")).toReal(), 3000);
    // 正文拖选在拖动中提交选区，输入法光标随之移动到反向拖动焦点，而不是冻结在原位。
    const QPointF reverse_focus_content = documentStopPoint(fixture, reverse_focus);
    QTRY_COMPARE_WITH_TIMEOUT(
        qRound(fixture.canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF().x()),
        qRound(reverse_focus_content.x()), 3000);
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier,
                        drag_preview_start_window.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("anchorPosition"))
                                  .toInt(),
                              4, 3000);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inputMethodContext()
                                  .value(QStringLiteral("cursorPosition"))
                                  .toInt(),
                              0, 3000);
    QVERIFY(fixture.canvas->caretPreviewGeometry().isEmpty());
    QCOMPARE(fixture.canvas->caretGeometry().value(QStringLiteral("xPt")).toReal(),
             reverse_focus.value(QStringLiteral("xPt")).toReal());
}

void ShellDocumentInteractionTest::clipboardCopyPasteUsesRustSelectionProjection() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 8), qPrintable(fixture.error));
    const DocumentDragStops stops = findDocumentDragStops(fixture);
    QVERIFY2(stops.valid, qPrintable(fixture.error));
    QVERIFY(fixture.layout_view_model->setSelection(stops.preview_start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    QSignalSpy copy_key_spy(fixture.canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(copy_key_spy.isValid());
    QTest::keyClick(&fixture.window, Qt::Key_C, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(copy_key_spy.size(), 1, 3000);
    QCOMPARE(copy_key_spy.takeFirst().at(0).toString(), QStringLiteral("mod-c"));
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("\nabc"));
    QVERIFY(fixture.layout_view_model->setSelection(stops.start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::keyClick(&fixture.window, Qt::Key_C, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(copy_key_spy.size(), 1, 3000);
    QCOMPARE(copy_key_spy.takeFirst().at(0).toString(), QStringLiteral("mod-c"));
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("abc"));
    QTest::keyClick(&fixture.window, Qt::Key_V, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("9"),
                              3000);
}

void ShellDocumentInteractionTest::selectionDragFromExistingSelectionPublishesCanvasRects() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 8), qPrintable(fixture.error));
    const DocumentDragStops stops = findDocumentDragStops(fixture);
    QVERIFY2(stops.valid, qPrintable(fixture.error));
    const QPointF drag_start_window = documentStopWindowPoint(fixture, stops.start);
    const QPointF drag_end_window = documentStopWindowPoint(fixture, stops.end);
    QVERIFY(fixture.layout_view_model->setSelection(stops.start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mousePress(&fixture.window, Qt::LeftButton, Qt::NoModifier, drag_start_window.toPoint());
    QMouseEvent drag_move(QEvent::MouseMove, drag_end_window, drag_end_window,
                          fixture.window.mapToGlobal(drag_end_window.toPoint()), Qt::NoButton,
                          Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&fixture.window, &drag_move);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.pointer_handler->property("selectionDragActive").toBool(),
                             3000);
    QVERIFY(!fixture.layout_view_model->selectionRects().isEmpty());
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->selectionRects().isEmpty(), 3000);
    QTest::mouseRelease(&fixture.window, Qt::LeftButton, Qt::NoModifier, drag_end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.pointer_handler->property("selectionDragActive").toBool(),
                             3000);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->selectionRects().isEmpty(), 3000);
}

void ShellDocumentInteractionTest::mouseClickClearsSelectionInsideAndOutsideText() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 8), qPrintable(fixture.error));
    const DocumentDragStops stops = findDocumentDragStops(fixture);
    QVERIFY2(stops.valid, qPrintable(fixture.error));
    const QPointF drag_start_window = documentStopWindowPoint(fixture, stops.start);
    QVERIFY(fixture.layout_view_model->setSelection(stops.start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, drag_start_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->selectionRects().isEmpty(), 3000);
    QVERIFY(fixture.layout_view_model->setSelection(stops.start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::mouseClick(&fixture.window, Qt::LeftButton, Qt::NoModifier, QPoint(1, 1));
    QTRY_VERIFY_WITH_TIMEOUT(fixture.layout_view_model->selectionRects().isEmpty(), 3000);
}

void ShellDocumentInteractionTest::multilinePasteSplitsParagraphs() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 9), qPrintable(fixture.error));
    const DocumentDragStops stops = findDocumentDragStops(fixture);
    QVERIFY2(stops.valid, qPrintable(fixture.error));
    QVERIFY(fixture.layout_view_model->setSelection(stops.start, stops.end)
                .value(QStringLiteral("valid"))
                .toBool());
    const QString text_before_multiline_paste = fixture.layout_view_model->paragraphLayouts()
                                                    .at(0)
                                                    .toMap()
                                                    .value(QStringLiteral("text"))
                                                    .toString();
    const auto drag_start_offset = stops.start.value(QStringLiteral("position"))
                                       .toMap()
                                       .value(QStringLiteral("offsetUtf8"))
                                       .toString()
                                       .toUInt();
    const QString expected_first_paragraph =
        QString::fromUtf8(text_before_multiline_paste.toUtf8().first(drag_start_offset)) +
        QStringLiteral("left");
    QGuiApplication::clipboard()->setText(QStringLiteral("left\nright"));
    QTest::keyClick(&fixture.window, Qt::Key_V, Qt::ControlModifier);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->documentGeneration(), QStringLiteral("10"),
                              3000);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->paragraphLayouts().size(), 2, 3000);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(0)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             expected_first_paragraph);
    QCOMPARE(fixture.layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("right"));
}

void ShellDocumentInteractionTest::insertReplacementClearsSelectionAndPublishesInteractionChange() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    auto* model = fixture.layout_view_model;
    const QString source = QStringLiteral("original text");
    const QString neighbor = QStringLiteral("next paragraph");
    QVERIFY(model->insertText(source).value(QStringLiteral("valid")).toBool());
    QVERIFY(
        model->handleEditorKey(QStringLiteral("enter")).value(QStringLiteral("valid")).toBool());
    QVERIFY(model->insertText(neighbor).value(QStringLiteral("valid")).toBool());
    const QVariantList paragraphs = model->paragraphLayouts();
    QCOMPARE(paragraphs.size(), 2);
    const QString paragraph_key = QStringLiteral("paragraphId");
    const QString source_id = paragraphs[0].toMap().value(paragraph_key).toString();
    const QString neighbor_id = paragraphs[1].toMap().value(paragraph_key).toString();
    const QVariantMap hit_map = model->dragHitMap(0);
    const QVariantMap start = findBodyDragStop(hit_map, source_id, 0);
    const QVariantMap end =
        findBodyDragStop(hit_map, source_id, static_cast<int>(source.toUtf8().size()));
    QVERIFY(!start.isEmpty());
    QVERIFY(!end.isEmpty());
    QVERIFY(model->setSelection(start, end).value(QStringLiteral("valid")).toBool());
    QVERIFY(!model->selectionRects().isEmpty());
    QSignalSpy interaction_changed_spy(model, &caret::LayoutViewModel::interactionChanged);
    QVERIFY(interaction_changed_spy.isValid());
    const QString replacement = QStringLiteral("replacement");
    QVERIFY(model->insertText(replacement).value(QStringLiteral("valid")).toBool());
    QVERIFY(interaction_changed_spy.size() > 0);
    QVERIFY(model->selectionRects().isEmpty());
    QCOMPARE(model->inputMethodContext().value(QStringLiteral("surroundingText")).toString(),
             replacement);
    QCOMPARE(paragraph_text_by_id(*model, neighbor_id), neighbor);
}

void ShellDocumentInteractionTest::crossPageDragPreviewIncludesIntermediatePages() {
    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionAt(fixture, 0), qPrintable(fixture.error));
    QVERIFY(fixture.layout_view_model->insertText(QStringLiteral("word ").repeated(4000))
                .value(QStringLiteral("valid"))
                .toBool());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->pageSizesPt().size() >= 3, 3000);
    const QVariantList first_stops =
        fixture.layout_view_model->dragHitMap(0).value(QStringLiteral("stops")).toList();
    const QVariantList third_stops =
        fixture.layout_view_model->dragHitMap(2).value(QStringLiteral("stops")).toList();
    QVERIFY(!first_stops.isEmpty());
    QVERIFY(!third_stops.isEmpty());
    fixture.pointer_handler->setProperty(
        "dragSnapshot",
        QVariantMap{
            {QStringLiteral("documentGeneration"), fixture.layout_view_model->documentGeneration()},
            {QStringLiteral("fontGeneration"), fixture.layout_view_model->fontGeneration()},
            {QStringLiteral("sessionId"), fixture.layout_view_model->sessionId()},
            {QStringLiteral("layoutRevision"),
             QString::number(fixture.layout_view_model->layoutRevision())}});
    const QVariant first_point =
        QVariant::fromValue(documentStopPoint(fixture, first_stops.first().toMap()));
    const QVariantMap third_stop = third_stops.first().toMap();
    const QVariant third_point = QVariant::fromValue(fixture.canvas->documentToContent(
        2, QPointF(third_stop.value(QStringLiteral("xPt")).toReal(),
                   third_stop.value(QStringLiteral("y0Pt")).toReal() + 3.0)));
    QCOMPARE(fixture.canvas->hitTestPage(third_point.toPointF())
                 .value(QStringLiteral("pageIndex"))
                 .toInt(),
             2);
    QVERIFY(QMetaObject::invokeMethod(fixture.pointer_handler, "updateDragPreview",
                                      Q_ARG(QVariant, first_point),
                                      Q_ARG(QVariant, QVariant(true))));
    QVERIFY(QMetaObject::invokeMethod(fixture.pointer_handler, "updateDragPreview",
                                      Q_ARG(QVariant, third_point),
                                      Q_ARG(QVariant, QVariant(false))));
    QVERIFY(!fixture.pointer_handler->property("dragAnchor").toMap().isEmpty());
    // 正文拖选在拖动中提交同一选区投影；跨页范围直接来自 LayoutViewModel.selectionRects。
    const QVariantList cross_page_preview = fixture.layout_view_model->selectionRects();
    QVERIFY(std::any_of(
        cross_page_preview.cbegin(), cross_page_preview.cend(), [](const QVariant& rectangle) {
            return rectangle.toMap().value(QStringLiteral("pageIndex")).toInt() == 1;
        }));
    fixture.pointer_handler->setProperty("selectionDragActive", true);
    QTRY_VERIFY_WITH_TIMEOUT(!fixture.canvas->selectionRects().isEmpty(), 3000);
    QVERIFY(!fixture.canvas->caretPreviewGeometry().isEmpty());
    QVERIFY(fixture.layout_view_model->insertText(QStringLiteral("x"))
                .value(QStringLiteral("valid"))
                .toBool());
    QTRY_VERIFY_WITH_TIMEOUT(fixture.pointer_handler->property("dragFocus").toMap().isEmpty(),
                             3000);
    QVERIFY(fixture.canvas->selectionPreviewRects().isEmpty());
    QVERIFY(fixture.canvas->caretPreviewGeometry().isEmpty());
}

void ShellDocumentInteractionTest::mathTextDragUsesSharedPreviewAndCommitProtocol() {
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
                        {"type":"math","id":"structure-x","kind":"symbol","text":"x"},
                        {"type":"math","id":"structure-plus","kind":"symbol","text":"+"},
                        {"type":"math","id":"structure-y","kind":"symbol","text":"y"},
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

    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture, QSize(600, 700)),
             qPrintable(fixture.error));
    auto& window = fixture.window;
    auto* view_model = fixture.layout_view_model;
    auto* canvas = fixture.canvas;
    QCOMPARE(canvas->caretWidthCentipixels(), 100);

    QSignalSpy open_finished(view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(open_finished.isValid());
    const QString initial_session_id = view_model->sessionId();
    const QString initial_document_generation = view_model->documentGeneration();
    const QVariantMap open_result = view_model->openDocument(path);
    QVERIFY2(open_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_result.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(open_finished.size(), 1, 5000);
    QVERIFY2(open_finished.first().at(0).toBool(),
             qPrintable(open_finished.first().at(1).toString()));
    const QString opened_session_id = view_model->sessionId();
    QVERIFY(!opened_session_id.isEmpty());
    QVERIFY(opened_session_id != initial_session_id);
    QCOMPARE(view_model->documentGeneration(), initial_document_generation);
    QVERIFY(!view_model->fontGeneration().isEmpty());

    window.requestActivate();
    canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
    QObject* selection_drag = fixture.selection_drag;
    QObject* pointer_handler = fixture.pointer_handler;

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
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    QVERIFY(canvas->selectionPreviewRects().isEmpty());
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));
    const QVariantMap input_context = view_model->inputMethodContext();
    QCOMPARE(input_context.value(QStringLiteral("surroundingText")).toString(),
             QStringLiteral("xabcdef"));
    QCOMPARE(input_context.value(QStringLiteral("anchorPosition")).toInt(), 1);
    QCOMPARE(input_context.value(QStringLiteral("cursorPosition")).toInt(), 7);
    QVERIFY(input_context.value(QStringLiteral("enabled")).toBool());
    // 公式内部选区无法构造完整 Caret 片段：复制明确失败且不覆盖系统剪贴板。
    const QString clipboard_before_math_copy = QGuiApplication::clipboard()->text();
    QVERIFY(!view_model->copySelectionToClipboard().value(QStringLiteral("valid")).toBool());
    QCOMPARE(QGuiApplication::clipboard()->text(), clipboard_before_math_copy);

    QVERIFY(view_model->clearSelection().value(QStringLiteral("valid")).toBool());
    QVERIFY(view_model->selectionRects().isEmpty());
    const QPointF body_left_window = canvas->mapToItem(window.contentItem(), stop_point(body_left));
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, body_left_window.toPoint());
    QMouseEvent body_to_math_move(QEvent::MouseMove, start_window, start_window,
                                  window.mapToGlobal(start_window.toPoint()), Qt::NoButton,
                                  Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &body_to_math_move);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    QCOMPARE(pointer_handler->property("dragFocus")
                 .toMap()
                 .value(QStringLiteral("position"))
                 .toMap()
                 .value(QStringLiteral("kind"))
                 .toString(),
             QStringLiteral("math"));
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, start_window.toPoint());
    // 跨正文与行内公式内部的结构选区同样不能生成完整片段，复制失败保留剪贴板。
    const QString clipboard_before_span_copy = QGuiApplication::clipboard()->text();
    QVERIFY(!view_model->copySelectionToClipboard().value(QStringLiteral("valid")).toBool());
    QCOMPARE(QGuiApplication::clipboard()->text(), clipboard_before_span_copy);
    QVERIFY(view_model->clearSelection().value(QStringLiteral("valid")).toBool());
    QVariantMap structural_stop;
    for (const QVariant& value : drag_stops) {
        const QVariantMap stop = value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("kind")).toString() == QStringLiteral("mathTree") &&
            position.value(QStringLiteral("target")).toMap().value(QStringLiteral("mathId")) ==
                QStringLiteral("inline-structure-math") &&
            position.value(QStringLiteral("offsetUtf8")).toString() == QStringLiteral("1") &&
            position.value(QStringLiteral("treePath")).toList().isEmpty()) {
            structural_stop = stop;
            break;
        }
    }
    QVERIFY(!structural_stop.isEmpty());
    const QPointF structural_window =
        canvas->mapToItem(window.contentItem(), stop_point(structural_stop));
    QVERIFY(view_model->selectionRects().isEmpty());

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
    const QVariantMap structural_anchor = pointer_handler->property("dragAnchor").toMap();
    QVERIFY(!structural_anchor.isEmpty());
    QCOMPARE(structural_anchor.value(QStringLiteral("domainId")).toString(),
             QStringLiteral("body"));
    QCOMPARE(structural_anchor.value(QStringLiteral("position"))
                 .toMap()
                 .value(QStringLiteral("kind"))
                 .toString(),
             QStringLiteral("mathTree"));
    QVERIFY(canvas->selectionPreviewRects().isEmpty());
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, body_window.toPoint());
    QVERIFY(!view_model->selectionRects().isEmpty());
    // 数学树内部选区不进入富剪贴板；失败时不覆盖系统剪贴板。
    const QString clipboard_before_tree_copy = QGuiApplication::clipboard()->text();
    QVERIFY(!view_model->copySelectionToClipboard().value(QStringLiteral("valid")).toBool());
    QCOMPARE(QGuiApplication::clipboard()->text(), clipboard_before_tree_copy);
    QCOMPARE(view_model->documentGeneration(), QStringLiteral("0"));

    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, start_window.toPoint());
    QInputMethodEvent preedit(QStringLiteral("ni"), {});
    QCoreApplication::sendEvent(canvas, &preedit);
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->compositionRects().isEmpty(), 3000);
    QTest::mousePress(&window, Qt::LeftButton, Qt::NoModifier, start_window.toPoint());
    QMouseEvent preedit_drag(QEvent::MouseMove, end_window, end_window,
                             window.mapToGlobal(end_window.toPoint()), Qt::NoButton, Qt::LeftButton,
                             Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &preedit_drag);
    QTRY_VERIFY_WITH_TIMEOUT(selection_drag->property("active").toBool(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!pointer_handler->property("dragAnchor").toMap().isEmpty(), 3000);
    QTest::mouseRelease(&window, Qt::LeftButton, Qt::NoModifier, end_window.toPoint());
    QTRY_VERIFY_WITH_TIMEOUT(!view_model->selectionRects().isEmpty(), 3000);
    pointer_handler->setProperty("selectionDragActive", true);
    pointer_handler->setProperty("dragRebasePending", true);
    pointer_handler->setProperty(
        "dragRebaseIdentity",
        QVariantMap{
            {QStringLiteral("documentGeneration"), view_model->documentGeneration()},
            {QStringLiteral("fontGeneration"), view_model->fontGeneration()},
            {QStringLiteral("sessionId"), view_model->sessionId()},
            {QStringLiteral("layoutRevision"), QString::number(view_model->layoutRevision())}});
    const QVariantMap replacement_open_result = view_model->openDocument(path);
    QVERIFY2(replacement_open_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(replacement_open_result.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(open_finished.size(), 2, 5000);
    QVERIFY(open_finished.last().at(0).toBool());
    QVERIFY(!pointer_handler->property("dragRebasePending").toBool());
    QVERIFY(pointer_handler->property("dragAnchor").toMap().isEmpty());
}

void ShellDocumentInteractionTest::emptyInlineMathFocusRendersInsertionSlotAcrossZoom() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString path = directory.filePath(QStringLiteral("empty-inline-math.cdoc"));
    QFile document_file(path);
    QVERIFY(document_file.open(QIODevice::WriteOnly | QIODevice::Text));
    const QByteArray document = QByteArrayLiteral(R"json({
        "format":"caret","version":1,
        "requiredFeatures":["caret.math.content-flow.v2","caret.math.equation","caret.math.inline",
            "caret.math.syntax.v1","caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings","caret.text.default-font-size"],
        "media":[],"document":{
            "globalSettings":{},"body":[{"type":"paragraph","id":"paragraph","inlines":[
                {"type":"text","runs":[{"text":"A"}]},
                {"type":"inlineMath","id":"inline-empty-math","mathTree":{
                    "schemaVersion":3,"root":{"id":"empty-root","content":[]}}},
                {"type":"text","runs":[{"text":"B"}]}
            ]}],
            "pageSections":[{"id":"page-section","props":{}}]
        }
    })json");
    QCOMPARE(document_file.write(document), document.size());
    document_file.close();

    DocumentInteractionFixture fixture;
    QVERIFY2(prepareDocumentInteractionFixture(fixture), qPrintable(fixture.error));
    QSignalSpy open_finished(fixture.layout_view_model,
                             &caret::LayoutViewModel::documentFileOperationFinished);
    QVERIFY(open_finished.isValid());
    const QVariantMap open_result = fixture.layout_view_model->openDocument(path);
    QVERIFY2(open_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(open_result.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(open_finished.size(), 1, 5000);
    QVERIFY2(open_finished.first().at(0).toBool(),
             qPrintable(open_finished.first().at(1).toString()));

    fixture.canvas->setCaretWidthCentipixels(100);
    fixture.window.requestActivate();
    fixture.canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY_WITH_TIMEOUT(fixture.canvas->hasActiveFocus(), 3000);

    // 用拖选命中图中 offset=1（"A" 之后、空公式之前）定位插入槽中心。
    const QVariantMap hit_map = fixture.layout_view_model->dragHitMap(0);
    QVERIFY2(hit_map.value(QStringLiteral("valid")).toBool(),
             qPrintable(hit_map.value(QStringLiteral("error")).toString()));
    QVariantMap boundary;
    for (const QVariant& value : hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = value.toMap();
        if (stop.value(QStringLiteral("position"))
                .toMap()
                .value(QStringLiteral("offsetUtf8"))
                .toString() == QStringLiteral("1")) {
            boundary = stop;
            break;
        }
    }
    QVERIFY2(!boundary.isEmpty(), "拖选命中图缺少空公式前的正文边界");
    const qreal boundary_y_pt = (boundary.value(QStringLiteral("y0Pt")).toReal() +
                                 boundary.value(QStringLiteral("y1Pt")).toReal()) /
                                2.0;
    const QVariantMap placed = fixture.layout_view_model->placeCaretAtPage(
        0, boundary.value(QStringLiteral("xPt")).toReal(), boundary_y_pt);
    QVERIFY2(placed.value(QStringLiteral("valid")).toBool(),
             qPrintable(placed.value(QStringLiteral("error")).toString()));
    QVERIFY2(placed.value(QStringLiteral("mathTree")).toBool(),
             "空 InlineMath 插入槽应进入公式编辑");
    QTRY_COMPARE_WITH_TIMEOUT(fixture.layout_view_model->inlineHighlightRects().size(), 1, 3000);
    const QVariantMap slot = fixture.layout_view_model->inlineHighlightRects().first().toMap();
    const QVariantMap caret = fixture.layout_view_model->caretGeometry();
    const qreal slot_x0 = slot.value(QStringLiteral("x0Pt")).toReal();
    const qreal slot_x1 = slot.value(QStringLiteral("x1Pt")).toReal();
    // 12pt 默认字号：槽宽 max(0.5em, 4pt) = 6pt，且以公式 caret 居中。
    QVERIFY(qAbs((slot_x1 - slot_x0) - 6.0) < 1e-3);
    QVERIFY(qAbs((slot_x0 + slot_x1) / 2.0 - caret.value(QStringLiteral("xPt")).toReal()) < 1e-3);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->inlineHighlightRects().size(), 1, 3000);

    const QColor highlight_color = fixture.canvas->inlineHighlightColor();
    const qreal sample_x = slot_x0 + 1.5;
    const qreal sample_y = (slot.value(QStringLiteral("y0Pt")).toReal() +
                            slot.value(QStringLiteral("y1Pt")).toReal()) /
                           2.0;
    const auto pixel_at_document_point = [&fixture](int page_index, const QPointF& point_pt) {
        const QPointF content = fixture.canvas->documentToContent(page_index, point_pt);
        const QPointF window_point =
            fixture.canvas->mapToItem(fixture.window.contentItem(), content);
        const QImage frame = fixture.window.grabWindow();
        if (frame.isNull()) {
            return QColor{};
        }
        const qreal scale_x = static_cast<qreal>(frame.width()) / fixture.window.width();
        const qreal scale_y = static_cast<qreal>(frame.height()) / fixture.window.height();
        return frame.pixelColor(qFloor(window_point.x() * scale_x),
                                qFloor(window_point.y() * scale_y));
    };
    QTRY_COMPARE_WITH_TIMEOUT(pixel_at_document_point(0, QPointF(sample_x, sample_y)),
                              highlight_color, 3000);

    // zoom≠1：同一文档点映射到缩放后的内容坐标，槽仍以该高亮色绘制。
    fixture.canvas->setZoom(2.0);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.canvas->zoom(), 2.0, 3000);
    QTRY_COMPARE_WITH_TIMEOUT(pixel_at_document_point(0, QPointF(sample_x, sample_y)),
                              highlight_color, 3000);
}

void ShellDocumentInteractionTest::documentInputDeltaKeepsUntouchedPagesAndParagraphs() {
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

    // 直接用 LayoutViewModel 的公开命令验证输入增量，不把测试逻辑放进内嵌 QML/JS。
    caret::LayoutViewModel view_model;
    const QVariantList initial_paragraphs = view_model.paragraphLayouts();
    QVERIFY2(initial_paragraphs.size() == 100, "初始快照应包含 paste 产生的 100 个段落");
    const QVariantMap last_paragraph = initial_paragraphs.last().toMap();
    const QVariantList initial_page_sizes = view_model.pageSizesPt();
    QVERIFY2(!initial_page_sizes.isEmpty(), "初始快照缺少页面尺寸");
    const int last_page_index = static_cast<int>(initial_page_sizes.size()) - 1;
    const QVariantMap last_hit_map = view_model.dragHitMap(last_page_index);
    QVERIFY2(last_hit_map.value(QStringLiteral("valid")).toBool(),
             qPrintable(last_hit_map.value(QStringLiteral("error")).toString()));
    const QString end_offset =
        QString::number(last_paragraph.value(QStringLiteral("text")).toString().length());
    QVariantMap end_stop;
    for (const QVariant& stop_value : last_hit_map.value(QStringLiteral("stops")).toList()) {
        const QVariantMap stop = stop_value.toMap();
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        if (position.value(QStringLiteral("paragraphId")).toString() ==
                last_paragraph.value(QStringLiteral("paragraphId")).toString() &&
            position.value(QStringLiteral("offsetUtf8")).toString() == end_offset) {
            end_stop = stop;
            break;
        }
    }
    QVERIFY2(!end_stop.isEmpty(), "Unable to find the final caret stop");
    const QVariantMap selection_result = view_model.setSelection(end_stop, end_stop);
    QVERIFY2(selection_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(selection_result.value(QStringLiteral("error")).toString()));
    const QVariantList initial_glyph_revisions = view_model.pageGlyphRevisions();
    const QVariantMap result = view_model.insertText(QStringLiteral("x"));
    const QVariantList post_insert_glyph_revisions = view_model.pageGlyphRevisions();
    const QVariantMap preedit_result =
        view_model.inputMethodEvent(QStringLiteral("n"), QString(), 0, 0, 1);
    const int preedit_paragraph_count = static_cast<int>(view_model.paragraphLayouts().size());
    const int preedit_rect_count = static_cast<int>(view_model.compositionRects().size());
    const QVariantMap updated_preedit_result =
        view_model.inputMethodEvent(QStringLiteral("ni"), QString(), 0, 0, 2);
    const QVariantMap commit_result =
        view_model.inputMethodEvent(QString(), QStringLiteral("你"), 0, 0, 0);
    const QVariantList paragraphs_after_commit = view_model.paragraphLayouts();
    QVERIFY2(paragraphs_after_commit.size() >= 100, "输入法提交后段落数不应减少");
    const QVariantMap cross_anchor{
        {QStringLiteral("position"),
         QVariantMap{{QStringLiteral("paragraphId"),
                      paragraphs_after_commit.at(98).toMap().value(QStringLiteral("paragraphId"))},
                     {QStringLiteral("offsetUtf8"), QStringLiteral("0")},
                     {QStringLiteral("affinity"), QStringLiteral("downstream")}}},
        {QStringLiteral("visualAffinity"), QStringLiteral("downstream")}};
    const QVariantMap cross_focus{
        {QStringLiteral("position"),
         QVariantMap{{QStringLiteral("paragraphId"),
                      paragraphs_after_commit.at(99).toMap().value(QStringLiteral("paragraphId"))},
                     {QStringLiteral("offsetUtf8"), QStringLiteral("0")},
                     {QStringLiteral("affinity"), QStringLiteral("upstream")}}},
        {QStringLiteral("visualAffinity"), QStringLiteral("upstream")}};
    const QVariantMap cross_selection_result = view_model.setSelection(cross_anchor, cross_focus);
    const QVariantMap cross_preedit_result =
        view_model.inputMethodEvent(QStringLiteral("x"), QString(), 0, 0, 1);
    const int cross_preedit_paragraph_count =
        static_cast<int>(view_model.paragraphLayouts().size());
    const QVariantMap cross_cancel_result =
        view_model.inputMethodEvent(QString(), QString(), 0, 0, 0);
    const int cross_cancel_paragraph_count = static_cast<int>(view_model.paragraphLayouts().size());

    QVERIFY2(result.value(QStringLiteral("valid")).toBool(),
             qPrintable(result.value(QStringLiteral("error")).toString()));
    QVERIFY2(preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(preedit_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(preedit_paragraph_count, 100);
    QVERIFY(preedit_rect_count > 0);
    QVERIFY2(updated_preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(updated_preedit_result.value(QStringLiteral("error")).toString()));
    QVERIFY2(commit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(commit_result.value(QStringLiteral("error")).toString()));
    QVERIFY2(cross_selection_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_selection_result.value(QStringLiteral("error")).toString()));
    QVERIFY2(cross_preedit_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_preedit_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(cross_preedit_paragraph_count, 99);
    QVERIFY2(cross_cancel_result.value(QStringLiteral("valid")).toBool(),
             qPrintable(cross_cancel_result.value(QStringLiteral("error")).toString()));
    QCOMPARE(cross_cancel_paragraph_count, 100);
    const QVariantList pages = view_model.pageSizesPt();
    const QVariantList glyph_revisions = view_model.pageGlyphRevisions();
    const QVariantList page_layouts = view_model.pageLayouts();
    const QVariantList paragraphs = view_model.paragraphLayouts();
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
}

void ShellDocumentInteractionTest::invalidDocumentFileStatusPreservesThePublishedProjection() {
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

void ShellDocumentInteractionTest::interactionResponseErrorsRefreshAutoSaveDeadline() {
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

void ShellDocumentInteractionTest::richClipboardCopyPreservesMathStructure() {
    QString error;
    QVERIFY2(installClipboardDocumentRuntime(QByteArray(kRichClipboardDocument), &error),
             qPrintable(error));
    caret::LayoutViewModel view_model;
    QVERIFY2(view_model.error().isEmpty(), qPrintable(view_model.error()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("0"));

    // 正文文本 + InlineMath 选区复制：同一 Rust 复制同时产出专用 MIME 与 TeX 纯文本回退。
    QGuiApplication::clipboard()->clear();
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-one"), 0,
                                            QStringLiteral("paragraph-one"), 16));
    const QVariantMap copy = view_model.copySelectionToClipboard();
    QVERIFY2(copy.value(QStringLiteral("valid")).toBool(),
             qPrintable(copy.value(QStringLiteral("error")).toString()));
    const QMimeData* mime = QGuiApplication::clipboard()->mimeData();
    if (mime == nullptr)
        QFAIL("clipboard must expose the copied MIME data");
    QVERIFY(mime->hasFormat(clipboard_fragment_mime_name()));
    const QString plain_text = mime->text();
    QVERIFY(!plain_text.isEmpty());
    QVERIFY(!plain_text.contains(QChar(0xFFFC)));
    QVERIFY(plain_text.contains(QStringLiteral("before")));
    QVERIFY(plain_text.contains(QStringLiteral("after")));
    QVERIFY(plain_text.contains(QStringLiteral("\\(")));
    QVERIFY(plain_text.contains(QStringLiteral("x")));

    const QJsonObject envelope =
        QJsonDocument::fromJson(mime->data(clipboard_fragment_mime_name())).object();
    QCOMPARE(envelope.value(QStringLiteral("format")).toString(), QStringLiteral("caret.fragment"));
    QCOMPARE(envelope.value(QStringLiteral("version")).toInt(), 1);
    QCOMPARE(envelope.value(QStringLiteral("shape")).toString(), QStringLiteral("inline"));
    const QJsonArray blocks = envelope.value(QStringLiteral("blocks")).toArray();
    QCOMPARE(blocks.size(), 1);
    bool inline_math_copied = false;
    for (const QJsonValue value :
         blocks.first().toObject().value(QStringLiteral("inlines")).toArray()) {
        const QJsonObject inline_object = value.toObject();
        if (inline_object.value(QStringLiteral("type")).toString() ==
                QStringLiteral("inlineMath") &&
            inline_object.value(QStringLiteral("mathTree"))
                    .toObject()
                    .value(QStringLiteral("schemaVersion"))
                    .toInt() == 3) {
            inline_math_copied = true;
        }
    }
    QVERIFY(inline_math_copied);

    // 跨段落 Blocks 片段保留 Equation/Align，text/plain 用规范 TeX 定界符且不含 U+FFFC。
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-one"), 0,
                                            QStringLiteral("paragraph-two"), 4));
    const QVariantMap blocks_copy = view_model.copySelectionToClipboard();
    QVERIFY2(blocks_copy.value(QStringLiteral("valid")).toBool(),
             qPrintable(blocks_copy.value(QStringLiteral("error")).toString()));
    const QMimeData* blocks_mime = QGuiApplication::clipboard()->mimeData();
    QVERIFY(blocks_mime->hasFormat(clipboard_fragment_mime_name()));
    const QJsonObject blocks_envelope =
        QJsonDocument::fromJson(blocks_mime->data(clipboard_fragment_mime_name())).object();
    QCOMPARE(blocks_envelope.value(QStringLiteral("shape")).toString(), QStringLiteral("blocks"));
    const QString blocks_text = blocks_mime->text();
    QVERIFY(blocks_text.contains(QStringLiteral("\\[")));
    QVERIFY(blocks_text.contains(QStringLiteral("aligned")));
    QVERIFY(!blocks_text.contains(QChar(0xFFFC)));
}

void ShellDocumentInteractionTest::richClipboardPasteRoundTripsMathStructureAndUndo() {
    QString error;
    QVERIFY2(installClipboardDocumentRuntime(QByteArray(kRichClipboardDocument), &error),
             qPrintable(error));
    caret::LayoutViewModel view_model;
    QVERIFY2(view_model.error().isEmpty(), qPrintable(view_model.error()));

    QGuiApplication::clipboard()->clear();
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-one"), 0,
                                            QStringLiteral("paragraph-one"), 16));
    QVERIFY2(view_model.copySelectionToClipboard().value(QStringLiteral("valid")).toBool(),
             "复制正文与 InlineMath 选区失败");
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-two"), 4,
                                            QStringLiteral("paragraph-two"), 4));
    const QVariantMap paste = view_model.pasteFromClipboard();
    QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
             qPrintable(paste.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("1"));
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")),
             QStringLiteral("tailbefore ") + QChar(0xFFFC) + QStringLiteral(" after"));

    // 粘贴后的 InlineMath 仍在片段中携带完整 MathTree，可再次往返。
    QGuiApplication::clipboard()->clear();
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-two"), 0,
                                            QStringLiteral("paragraph-two"), 20));
    QVERIFY(view_model.copySelectionToClipboard().value(QStringLiteral("valid")).toBool());
    const QMimeData* mime = QGuiApplication::clipboard()->mimeData();
    QVERIFY(mime->hasFormat(clipboard_fragment_mime_name()));
    const QJsonObject envelope =
        QJsonDocument::fromJson(mime->data(clipboard_fragment_mime_name())).object();
    QVERIFY(QJsonDocument(envelope).toJson().contains("mathTree"));
    QVERIFY(mime->text().contains(QStringLiteral("\\(")));
    QVERIFY(!mime->text().contains(QChar(0xFFFC)));

    const QVariantMap undo = view_model.undo();
    QVERIFY2(undo.value(QStringLiteral("valid")).toBool(),
             qPrintable(undo.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("2"));
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")),
             QStringLiteral("tail"));
    QVERIFY(view_model.canRedo());
}

void ShellDocumentInteractionTest::richClipboardBlocksPasteRoundTripsDisplayMathAndUndo() {
    QString error;
    QVERIFY2(installClipboardDocumentRuntime(QByteArray(kRichClipboardDocument), &error),
             qPrintable(error));
    caret::LayoutViewModel view_model;
    QVERIFY2(view_model.error().isEmpty(), qPrintable(view_model.error()));
    QCOMPARE(math_fragment_count(view_model), 2);
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("0"));

    // 跨段 Blocks 选区包含正文、Equation 与 Align：复制后信封保留两类数学块。
    QGuiApplication::clipboard()->clear();
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-one"), 0,
                                            QStringLiteral("paragraph-two"), 4));
    QVERIFY2(view_model.copySelectionToClipboard().value(QStringLiteral("valid")).toBool(),
             "跨段 Blocks 复制失败");
    const QMimeData* mime = QGuiApplication::clipboard()->mimeData();
    if (mime == nullptr)
        QFAIL("clipboard must expose the copied MIME data");
    QVERIFY(mime->hasFormat(clipboard_fragment_mime_name()));
    const QJsonObject envelope =
        QJsonDocument::fromJson(mime->data(clipboard_fragment_mime_name())).object();
    QCOMPARE(envelope.value(QStringLiteral("shape")).toString(), QStringLiteral("blocks"));
    const QJsonArray blocks = envelope.value(QStringLiteral("blocks")).toArray();
    QCOMPARE(blocks.size(), 4);
    QCOMPARE(blocks.at(0).toObject().value(QStringLiteral("type")).toString(),
             QStringLiteral("paragraph"));
    QCOMPARE(blocks.at(1).toObject().value(QStringLiteral("type")).toString(),
             QStringLiteral("equation"));
    QCOMPARE(blocks.at(2).toObject().value(QStringLiteral("type")).toString(),
             QStringLiteral("align"));
    QCOMPARE(blocks.at(3).toObject().value(QStringLiteral("type")).toString(),
             QStringLiteral("paragraph"));

    // 片段内容逐项断言：InlineMath 的 MathTree 符号 x、Equation 符号 y、Align 首格符号 a。
    bool inline_symbol_x = false;
    for (const QJsonValue value :
         blocks.at(0).toObject().value(QStringLiteral("inlines")).toArray()) {
        const QJsonObject inline_object = value.toObject();
        if (inline_object.value(QStringLiteral("type")).toString() != QStringLiteral("inlineMath"))
            continue;
        const QJsonArray content = inline_object.value(QStringLiteral("mathTree"))
                                       .toObject()
                                       .value(QStringLiteral("root"))
                                       .toObject()
                                       .value(QStringLiteral("content"))
                                       .toArray();
        inline_symbol_x = !content.isEmpty() &&
                          content.at(0).toObject().value(QStringLiteral("text")).toString() ==
                              QStringLiteral("x");
    }
    QVERIFY(inline_symbol_x);
    const auto math_tree_symbol = [](const QJsonObject& tree) {
        const QJsonArray content = tree.value(QStringLiteral("root"))
                                       .toObject()
                                       .value(QStringLiteral("content"))
                                       .toArray();
        return content.isEmpty()
                   ? QString()
                   : content.at(0).toObject().value(QStringLiteral("text")).toString();
    };
    QCOMPARE(math_tree_symbol(blocks.at(1).toObject().value(QStringLiteral("mathTree")).toObject()),
             QStringLiteral("y"));
    const QJsonArray align_cells = blocks.at(2)
                                       .toObject()
                                       .value(QStringLiteral("rows"))
                                       .toArray()
                                       .at(0)
                                       .toObject()
                                       .value(QStringLiteral("cells"))
                                       .toArray();
    QCOMPARE(align_cells.size(), 1);
    QCOMPARE(math_tree_symbol(align_cells.at(0).toObject()), QStringLiteral("a"));
    const QString blocks_text = mime->text();
    QVERIFY(blocks_text.contains(QStringLiteral("\\(x\\)")));
    QVERIFY(blocks_text.contains(QStringLiteral("\\[y\\]")));
    QVERIFY(blocks_text.contains(QStringLiteral("aligned")));
    QVERIFY(!blocks_text.contains(QChar(0xFFFC)));
    QCOMPARE(inline_math_placeholder_count(view_model), 1);
    const QStringList source_math_ids = math_fragment_ids(view_model);
    QCOMPARE(source_math_ids.size(), 2);

    // 粘贴到 paragraph-two 末尾：正文、InlineMath、Equation、Align 全部进入目标文档，
    // 新显示公式 ID 与源 ID 不冲突，一次 Undo/Redo 恢复整份文档快照。
    QVERIFY(select_document_paragraph_range(view_model, QStringLiteral("paragraph-two"), 4,
                                            QStringLiteral("paragraph-two"), 4));
    const QVariantMap paste = view_model.pasteFromClipboard();
    QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
             qPrintable(paste.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("1"));
    QCOMPARE(math_fragment_count(view_model), 4);
    QCOMPARE(inline_math_placeholder_count(view_model), 2);
    bool copied_inline_paragraph = false;
    for (const QVariant& value : view_model.paragraphLayouts()) {
        if (value.toMap().value(QStringLiteral("text")).toString() ==
            QStringLiteral("before ") + QChar(0xFFFC) + QStringLiteral(" after"))
            copied_inline_paragraph = true;
    }
    QVERIFY(copied_inline_paragraph);
    const QStringList pasted_math_ids = math_fragment_ids(view_model);
    QCOMPARE(pasted_math_ids.size(), 4);
    QCOMPARE(QSet<QString>(pasted_math_ids.begin(), pasted_math_ids.end()).size(), 4);
    for (const QString& source_id : source_math_ids)
        QVERIFY(pasted_math_ids.contains(source_id));
    const QSet<QString> source_ids(source_math_ids.begin(), source_math_ids.end());
    const qsizetype new_math_ids =
        std::count_if(pasted_math_ids.begin(), pasted_math_ids.end(),
                      [&source_ids](const QString& id) { return !source_ids.contains(id); });
    QCOMPARE(new_math_ids, 2);

    const QVariantMap undo = view_model.undo();
    QVERIFY2(undo.value(QStringLiteral("valid")).toBool(),
             qPrintable(undo.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("2"));
    QCOMPARE(math_fragment_count(view_model), 2);
    QCOMPARE(inline_math_placeholder_count(view_model), 1);
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")),
             QStringLiteral("tail"));
    QVERIFY(view_model.canRedo());

    const QVariantMap redo = view_model.redo();
    QVERIFY2(redo.value(QStringLiteral("valid")).toBool(),
             qPrintable(redo.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("3"));
    QCOMPARE(math_fragment_count(view_model), 4);
    QCOMPARE(inline_math_placeholder_count(view_model), 2);
}

void ShellDocumentInteractionTest::richClipboardRoundTripsATableAndItsDisplayMath() {
    QString error;
    QVERIFY2(installClipboardDocumentRuntime(QByteArray(kTableWithDisplayMathDocument), &error),
             qPrintable(error));
    {
        caret::LayoutViewModel source;
        const QVariantMap map = source.dragHitMap(0);
        const QVariantMap text = findBodyDragStop(map, QStringLiteral("text"), 0);
        QVERIFY(!text.isEmpty());
        QVariantMap formula;
        for (const QVariant& entry : map.value(QStringLiteral("stops")).toList()) {
            const QVariantMap stop = entry.toMap();
            if (stop.value(QStringLiteral("cellId")) == QStringLiteral("right") &&
                stop.value(QStringLiteral("domainId")) == QStringLiteral("body")) {
                formula = stop;
                break;
            }
        }
        QVERIFY(!formula.isEmpty());
        QVERIFY(source.setSelection(text, formula).value(QStringLiteral("valid")).toBool());
        const QVariantMap copied = source.copySelectionToClipboard();
        QVERIFY2(copied.value(QStringLiteral("valid")).toBool(),
                 qPrintable(copied.value(QStringLiteral("error")).toString()));
    }

    const QMimeData* mime = QGuiApplication::clipboard()->mimeData();
    if (mime == nullptr)
        QFAIL("clipboard must expose the copied MIME data");
    QVERIFY(mime->hasFormat(clipboard_fragment_mime_name()));
    const QJsonObject envelope =
        QJsonDocument::fromJson(mime->data(clipboard_fragment_mime_name())).object();
    QCOMPARE(envelope.value(QStringLiteral("shape")).toString(), QStringLiteral("blocks"));
    const QJsonArray blocks = envelope.value(QStringLiteral("blocks")).toArray();
    QCOMPARE(blocks.size(), 1);
    QCOMPARE(blocks[0].toObject().value(QStringLiteral("type")).toString(),
             QStringLiteral("table"));
    QVERIFY(mime->text().startsWith(QStringLiteral("body text")));
    QVERIFY(mime->text().contains(QStringLiteral("\t\\[x\\]")));
    QVERIFY(!mime->text().contains(QChar(0xFFFC)));

    QVERIFY2(installBlankInteractionRuntime(&error), qPrintable(error));
    caret::LayoutViewModel destination;
    const QVariantMap pasted = destination.pasteFromClipboard();
    QVERIFY2(pasted.value(QStringLiteral("valid")).toBool(),
             qPrintable(pasted.value(QStringLiteral("error")).toString()));
    const auto pasted_regions =
        destination.dragHitMap(0).value(QStringLiteral("cellHitRegions")).toList();
    QCOMPARE(pasted_regions.size(), 2);
    for (const QVariant& entry : pasted_regions) {
        const QString id = entry.toMap().value(QStringLiteral("cellId")).toString();
        QVERIFY(!id.isEmpty());
        QVERIFY(id != QStringLiteral("left") && id != QStringLiteral("right"));
    }
    const auto table_formula_ids = [](caret::LayoutViewModel& model) {
        QSet<QString> ids;
        for (const QVariant& entry : model.dragHitMap(0).value(QStringLiteral("stops")).toList()) {
            const auto stop = entry.toMap();
            if (stop.value(QStringLiteral("cellId")).toString().isEmpty()) {
                continue;
            }
            const QString block_id = stop.value(QStringLiteral("position"))
                                         .toMap()
                                         .value(QStringLiteral("target"))
                                         .toMap()
                                         .value(QStringLiteral("blockId"))
                                         .toString();
            if (!block_id.isEmpty()) {
                ids.insert(block_id);
            }
        }
        return ids;
    };
    const auto pasted_formulas = table_formula_ids(destination);
    QCOMPARE(pasted_formulas.size(), 1);
    QVERIFY(!pasted_formulas.contains(QStringLiteral("formula")));
    QVERIFY(destination.undo().value(QStringLiteral("valid")).toBool());
    QVERIFY(destination.dragHitMap(0).value(QStringLiteral("cellHitRegions")).toList().isEmpty());
    QVERIFY(table_formula_ids(destination).isEmpty());
    QVERIFY(destination.redo().value(QStringLiteral("valid")).toBool());
    QCOMPARE(destination.dragHitMap(0).value(QStringLiteral("cellHitRegions")).toList(),
             pasted_regions);
    QCOMPARE(table_formula_ids(destination), pasted_formulas);
}

void ShellDocumentInteractionTest::richClipboardRejectsInvalidFragmentPayload() {
    QString error;
    QVERIFY2(installClipboardDocumentRuntime(QByteArray(kRichClipboardDocument), &error),
             qPrintable(error));
    caret::LayoutViewModel view_model;
    QVERIFY2(view_model.error().isEmpty(), qPrintable(view_model.error()));
    const QString paragraph_one = paragraph_text_by_id(view_model, QStringLiteral("paragraph-one"));
    const QString paragraph_two = paragraph_text_by_id(view_model, QStringLiteral("paragraph-two"));

    // 未知版本且同时带安全的 text/plain：必须整次拒绝，不得降级插入纯文本。
    set_clipboard_fragment_payload(
        QByteArrayLiteral(R"({"format":"caret.fragment","version":99,"shape":"inline",)"
                          R"("startClipped":false,"endClipped":false,"blocks":[]})"),
        QStringLiteral("plain fallback"));
    const QVariantMap unknown_version = view_model.pasteFromClipboard();
    QVERIFY(!unknown_version.value(QStringLiteral("valid")).toBool());
    QVERIFY2(unknown_version.value(QStringLiteral("error"))
                 .toString()
                 .contains(QStringLiteral("clipboard.invalid_fragment")),
             qPrintable(unknown_version.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-one")), paragraph_one);
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")), paragraph_two);
    QVERIFY(!paragraph_text_by_id(view_model, QStringLiteral("paragraph-two"))
                 .contains(QStringLiteral("plain")));
    // 失败不覆盖系统剪贴板中的源载荷。
    QVERIFY(QGuiApplication::clipboard()->mimeData()->hasFormat(clipboard_fragment_mime_name()));
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("plain fallback"));

    // 损坏 JSON 同样拒绝且保持文档状态。
    set_clipboard_fragment_payload(QByteArrayLiteral("{not-json"),
                                   QStringLiteral("plain fallback"));
    const QVariantMap corrupt = view_model.pasteFromClipboard();
    QVERIFY(!corrupt.value(QStringLiteral("valid")).toBool());
    QVERIFY(corrupt.value(QStringLiteral("error"))
                .toString()
                .contains(QStringLiteral("clipboard.invalid_fragment")));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")), paragraph_two);

    // 非 UTF-8 片段字节必须返回错误而不是让 cxx 字符串转换抛异常退出进程。
    set_clipboard_fragment_payload(QByteArray::fromHex("fffe"), QStringLiteral("plain fallback"));
    const QVariantMap non_utf8 = view_model.pasteFromClipboard();
    QVERIFY(!non_utf8.value(QStringLiteral("valid")).toBool());
    QVERIFY2(non_utf8.value(QStringLiteral("error"))
                 .toString()
                 .contains(QStringLiteral("clipboard.invalid_fragment")),
             qPrintable(non_utf8.value(QStringLiteral("error")).toString()));
    QCOMPARE(view_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")), paragraph_two);
}

void ShellDocumentInteractionTest::externalMathTextPasteRecognizesDelimiters() {
    const auto prepare_caret = [](caret::LayoutViewModel& view_model) {
        const QVariantMap hit_map = view_model.dragHitMap(0);
        const QVariantMap caret = findBodyDragStop(hit_map, QStringLiteral("paragraph-two"), 4);
        if (caret.isEmpty())
            return false;
        return view_model.setSelection(caret, caret).value(QStringLiteral("valid")).toBool();
    };
    const auto install_fixture = [](QString* error) {
        return installClipboardDocumentRuntime(QByteArray(kRichClipboardDocument), error);
    };

    // 混合文本中的闭合 \(...\)：插入 InlineMath，而不是普通文本。
    {
        QString error;
        QVERIFY2(install_fixture(&error), qPrintable(error));
        caret::LayoutViewModel view_model;
        QVERIFY(prepare_caret(view_model));
        QGuiApplication::clipboard()->setText(QStringLiteral("before \\(x\\) after"));
        const QVariantMap paste = view_model.pasteFromClipboard();
        QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
                 qPrintable(paste.value(QStringLiteral("error")).toString()));
        QCOMPARE(view_model.documentGeneration(), QStringLiteral("1"));
        QCOMPARE(paragraph_text_by_id(view_model, QStringLiteral("paragraph-two")),
                 QStringLiteral("tailbefore ") + QChar(0xFFFC) + QStringLiteral(" after"));
    }

    // 整段 \[...\]：构造 Equation 块，显示公式片段增加一个。
    {
        QString error;
        QVERIFY2(install_fixture(&error), qPrintable(error));
        caret::LayoutViewModel view_model;
        const qsizetype math_blocks_before = math_fragment_count(view_model);
        QCOMPARE(math_blocks_before, 2);
        QVERIFY(prepare_caret(view_model));
        QGuiApplication::clipboard()->setText(QStringLiteral("\\[y\\]"));
        const QVariantMap paste = view_model.pasteFromClipboard();
        QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
                 qPrintable(paste.value(QStringLiteral("error")).toString()));
        QCOMPARE(view_model.documentGeneration(), QStringLiteral("1"));
        QCOMPARE(math_fragment_count(view_model), math_blocks_before + 1);
    }

    // 整段 aligned 公式：构造 Align 块。
    {
        QString error;
        QVERIFY2(install_fixture(&error), qPrintable(error));
        caret::LayoutViewModel view_model;
        const qsizetype math_blocks_before = math_fragment_count(view_model);
        QVERIFY(prepare_caret(view_model));
        QGuiApplication::clipboard()->setText(
            QStringLiteral("\\[\\begin{aligned}a&=b\\end{aligned}\\]"));
        const QVariantMap paste = view_model.pasteFromClipboard();
        QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
                 qPrintable(paste.value(QStringLiteral("error")).toString()));
        QCOMPARE(math_fragment_count(view_model), math_blocks_before + 1);
    }

    // 整段 $...$：仅当占据全部文本时识别为 InlineMath。
    {
        QString error;
        QVERIFY2(install_fixture(&error), qPrintable(error));
        caret::LayoutViewModel view_model;
        QVERIFY(prepare_caret(view_model));
        QGuiApplication::clipboard()->setText(QStringLiteral("$x$"));
        const QVariantMap paste = view_model.pasteFromClipboard();
        QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
                 qPrintable(paste.value(QStringLiteral("error")).toString()));
        QCOMPARE(replacement_char_count(
                     paragraph_text_by_id(view_model, QStringLiteral("paragraph-two"))),
                 1);
    }

    // 未闭合 \(...\)：整段按普通文本插入，不产生 InlineMath。
    {
        QString error;
        QVERIFY2(install_fixture(&error), qPrintable(error));
        caret::LayoutViewModel view_model;
        const qsizetype math_blocks_before = math_fragment_count(view_model);
        QVERIFY(prepare_caret(view_model));
        QGuiApplication::clipboard()->setText(QStringLiteral("unclosed \\(x"));
        const QVariantMap paste = view_model.pasteFromClipboard();
        QVERIFY2(paste.value(QStringLiteral("valid")).toBool(),
                 qPrintable(paste.value(QStringLiteral("error")).toString()));
        const QString text = paragraph_text_by_id(view_model, QStringLiteral("paragraph-two"));
        QCOMPARE(text, QStringLiteral("tailunclosed \\(x"));
        QCOMPARE(replacement_char_count(text), 0);
        QCOMPARE(math_fragment_count(view_model), math_blocks_before);
    }
}
#endif
