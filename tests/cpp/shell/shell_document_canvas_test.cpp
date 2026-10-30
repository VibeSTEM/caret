// Qt/QML 画布几何、输入和缩放行为的集成测试。
#include "canvas_policy.hpp"
#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "shell_load_test.hpp"
#include "theme_view_model.hpp"
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
#include <algorithm>
#include <limits>
#include <qcontainerfwd.h>
#include <qtestcase.h>
#include <qtestsupport_core.h>
#include <utility>

void ShellLoadTest::canvasPolicyPublishesDocumentedZoomLimits() {
    const caret::CanvasPolicy policy;
    QCOMPARE(policy.defaultZoomPercent(), 100);
    QCOMPARE(policy.minimumZoomPercent(), 50);
    QCOMPARE(policy.maximumZoomPercent(), 300);
    QCOMPARE(policy.zoomStepPercent(), 10);
}

void ShellLoadTest::documentCanvasMapsPageCoordinatesAndHitsPages() {
    constexpr int kTestCursorFlashPeriodMs = 100;
    constexpr int kTestBlinkTransitionTimeoutMs = 500;
    constexpr int kDisabledBlinkObservationMs = 250;

    const QString theme_path =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  caret::shell_test::snapshot_from_theme_facade(theme_path, QColor(Qt::white), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));
    const QString empty_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[{"widthPt":600,"heightPt":800,"fragments":[{"paragraphId":"p1","firstLine":0,"lineEnd":1,"yOffsetPt":0}]}],"paragraphs":[{"paragraphId":"p1","text":"","lines":[{"sourceStart":0,"sourceEnd":0,"consumedStart":0,"consumedEnd":0,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":0,"overflowsWidth":false}]}]})");
    QString max_generation_snapshot = empty_snapshot;
    max_generation_snapshot.replace(
        QStringLiteral("\"documentGeneration\":\"0\""),
        QStringLiteral("\"documentGeneration\":\"18446744073709551615\""));
    QVERIFY(caret::install_layout_snapshot(max_generation_snapshot));
    const caret::LayoutViewModel max_generation_model;
    const QString max_generation = QString::number(std::numeric_limits<qulonglong>::max());
    QCOMPARE(max_generation_model.documentGeneration(), max_generation);
    QQmlEngine generation_engine;
    generation_engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent generation_component(&generation_engine);
    generation_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item { property var generation: LayoutViewModel.documentGeneration }\n"),
        QUrl());
    QVERIFY2(generation_component.isReady(), qPrintable(generation_component.errorString()));
    QObject* generation_object = generation_component.create();
    QVERIFY(generation_object != nullptr);
    QCOMPARE(generation_object->property("generation").toString(), max_generation);
    delete generation_object;
    QVERIFY(caret::install_layout_snapshot(empty_snapshot));

    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("{invalid")));
    QString unsafe_index_snapshot = empty_snapshot;
    unsafe_index_snapshot.replace(QStringLiteral("\"sourceStart\":0"),
                                  QStringLiteral("\"sourceStart\":9223372036854775808"));
    QVERIFY(!caret::install_layout_snapshot(unsafe_index_snapshot));
    const QString split_utf8_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"éx","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":3,"consumedStart":1,"consumedEnd":3,"topPt":12,"baselinePt":21,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(split_utf8_snapshot));
    const QString overlapping_lines_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"ab","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":2,"consumedStart":1,"consumedEnd":2,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(overlapping_lines_snapshot));
    QString negative_line_top = empty_snapshot;
    negative_line_top.replace(QStringLiteral("\"topPt\":0"), QStringLiteral("\"topPt\":-1"));
    QVERIFY(!caret::install_layout_snapshot(negative_line_top));
    QString outside_page_fragment = empty_snapshot;
    outside_page_fragment.replace(QStringLiteral("\"yOffsetPt\":0"),
                                  QStringLiteral("\"yOffsetPt\":1e300"));
    QVERIFY(!caret::install_layout_snapshot(outside_page_fragment));
    const caret::LayoutViewModel retained_snapshot_model;
    QCOMPARE(retained_snapshot_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(retained_snapshot_model.fontGeneration(), QStringLiteral("1"));
    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent view_model_component(&engine);
    view_model_component.setData(
        QByteArrayLiteral(
            "import QtQuick\n"
            "import Caret.ViewModels\n"
            "Item {\n"
            "    property var pageSizes: LayoutViewModel.pageSizesPt\n"
            "    property var pages: LayoutViewModel.pageLayouts\n"
            "    property var paragraphs: LayoutViewModel.paragraphLayouts\n"
            "    property string documentGeneration: LayoutViewModel.documentGeneration\n"
            "    property string fontGeneration: LayoutViewModel.fontGeneration\n"
            "}\n"),
        QUrl());
    QVERIFY2(view_model_component.isReady(), qPrintable(view_model_component.errorString()));
    QObject* view_model = view_model_component.create();
    QVERIFY(view_model != nullptr);
    const QVariantList page_sizes = view_model->property("pageSizes").toList();
    QCOMPARE(page_sizes.size(), 1);
    QCOMPARE(page_sizes.front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(view_model->property("pages").toList().size(), 1);
    QCOMPARE(view_model->property("paragraphs").toList().size(), 1);
    QCOMPARE(view_model->property("documentGeneration").toString(), QStringLiteral("0"));
    QCOMPARE(view_model->property("fontGeneration").toString(), QStringLiteral("1"));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "Item {\n"
                          "    DocumentCanvas {\n"
                          "        objectName: \"canvas\"\n"
                          "        pageSizesPt: [Qt.size(600, 800), Qt.size(300, 400)]\n"
                          "        zoom: 1.5\n"
                          "        width: implicitWidth + 100\n"
                          "    }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* root = component.create();
    QVERIFY(root != nullptr);
    auto* root_item = qobject_cast<QQuickItem*>(root);
    QVERIFY(root_item != nullptr);
    QQuickWindow input_method_window;
    input_method_window.resize(1000, 700);
    root_item->setWidth(1300.0);
    root_item->setHeight(2600.0);
    root_item->setParentItem(input_method_window.contentItem());
    input_method_window.show();
    auto* canvas = root->findChild<caret::DocumentCanvas*>(QStringLiteral("canvas"));
    QVERIFY(canvas != nullptr);
    canvas->forceActiveFocus();
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
    QSignalSpy text_input_spy(canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editing_command_spy(canvas, &caret::DocumentCanvas::editingCommandRequested);
    QVERIFY(editing_command_spy.isValid());
    QSignalSpy editor_key_spy(canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QSignalSpy visual_navigation_spy(canvas, &caret::DocumentCanvas::visualNavigationRequested);
    QVERIFY(visual_navigation_spy.isValid());
    QSignalSpy vertical_navigation_spy(canvas, &caret::DocumentCanvas::verticalNavigationRequested);
    QVERIFY(vertical_navigation_spy.isValid());
    QSignalSpy line_edge_navigation_spy(canvas,
                                        &caret::DocumentCanvas::lineEdgeNavigationRequested);
    QVERIFY(line_edge_navigation_spy.isValid());
    QKeyEvent altgr_input(QEvent::KeyPress, Qt::Key_Q,
                          Qt::ControlModifier | Qt::AltModifier | Qt::GroupSwitchModifier,
                          QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_input);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent altgr_without_group_switch(
        QEvent::KeyPress, Qt::Key_Q, Qt::ControlModifier | Qt::AltModifier, QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_without_group_switch);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent shortcut_input(QEvent::KeyPress, Qt::Key_X, Qt::ControlModifier, QStringLiteral("x"));
    QCoreApplication::sendEvent(canvas, &shortcut_input);
    QCOMPARE(text_input_spy.size(), 0);
    QKeyEvent keypad_enter(QEvent::KeyPress, Qt::Key_Enter, Qt::KeypadModifier, QString());
    QCoreApplication::sendEvent(canvas, &keypad_enter);
    QCOMPARE(editor_key_spy.size(), 1);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QKeyEvent shift_left(QEvent::KeyPress, Qt::Key_Left, Qt::ShiftModifier, QString());
    QCoreApplication::sendEvent(canvas, &shift_left);
    QCOMPARE(visual_navigation_spy.size(), 1);
    const QList<QVariant> navigation_arguments = visual_navigation_spy.takeFirst();
    QCOMPARE(navigation_arguments.at(0).toBool(), false);
    QCOMPARE(navigation_arguments.at(1).toBool(), true);
    QKeyEvent down(QEvent::KeyPress, Qt::Key_Down, Qt::NoModifier, QString());
    QCoreApplication::sendEvent(canvas, &down);
    QCOMPARE(vertical_navigation_spy.size(), 1);
    QCOMPARE(vertical_navigation_spy.takeFirst().at(0).toInt(), 1);
    QKeyEvent line_end(QEvent::KeyPress, Qt::Key_End, Qt::NoModifier, QString());
    QCoreApplication::sendEvent(canvas, &line_end);
    QCOMPARE(line_edge_navigation_spy.size(), 1);
    QCOMPARE(line_edge_navigation_spy.takeFirst().at(0).toBool(), true);

    const QSizeF natural_size = canvas->naturalContentSize();
    QCOMPARE(natural_size.width(), 1296.0);
    QCOMPARE(natural_size.height(), 2544.0);
    const QPointF origin = canvas->pageOrigin(1);
    QCOMPARE(origin, QPointF(398.0, 1696.0));

    const QPointF document_point(12.0, 20.0);
    const QPointF content_point = canvas->documentToContent(1, document_point);
    QCOMPARE(content_point, QPointF(422.0, 1736.0));
    QCOMPARE(canvas->contentToDocument(1, content_point), document_point);
    const QVariantMap hit = canvas->hitTestPage(content_point);
    QCOMPARE(hit.value(QStringLiteral("pageIndex")).toInt(), 1);
    QCOMPARE(hit.value(QStringLiteral("xPt")).toReal(), document_point.x());
    QCOMPARE(hit.value(QStringLiteral("yPt")).toReal(), document_point.y());
    QCOMPARE(canvas->hitTestPage(QPointF(-1.0, -1.0)).value(QStringLiteral("pageIndex")).toInt(),
             -1);

    const QVariantMap caret_geometry{{QStringLiteral("pageIndex"), 1},
                                     {QStringLiteral("xPt"), 12.0},
                                     {QStringLiteral("y0Pt"), 20.0},
                                     {QStringLiteral("y1Pt"), 30.0}};
    canvas->setCaretWidthCentipixels(100);
    canvas->setCaretGeometry(caret_geometry);
    const QVariantMap installed_caret = canvas->caretGeometry();
    QCOMPARE(installed_caret.value(QStringLiteral("pageIndex")).toInt(), 1);
    QVariantMap invalid_caret = caret_geometry;
    invalid_caret.insert(QStringLiteral("pageIndex"), 0.5);
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);

    canvas->setInputMethodContext({{QStringLiteral("surroundingText"), QStringLiteral("A😀B")},
                                   {QStringLiteral("cursorPosition"), 3},
                                   {QStringLiteral("anchorPosition"), 1}});
    QCOMPARE(canvas->inputMethodQuery(Qt::ImSurroundingText).toString(), QStringLiteral("A😀B"));
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorPosition).toInt(), 3);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImAnchorPosition).toInt(), 1);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCurrentSelection).toString(), QStringLiteral("😀"));
    QList<QInputMethodEvent::Attribute> attributes;
    attributes.push_back(QInputMethodEvent::Attribute(QInputMethodEvent::Cursor, 2, 0, {}));
    QInputMethodEvent input_method_event(QStringLiteral("ni hao"), attributes);
    QCoreApplication::sendEvent(canvas, &input_method_event);
    QCOMPARE(canvas->preeditText(), QStringLiteral("ni hao"));
    QCOMPARE(canvas->preeditCursorPosition(), 2);
    const QRectF composition_caret = canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    QCOMPARE(composition_caret.x(), canvas->documentToContent(1, QPointF(12.0, 20.0)).x());
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(), composition_caret);
    canvas->cancelPreedit();
    QVERIFY(canvas->preeditText().isEmpty());
    invalid_caret.insert(QStringLiteral("pageIndex"),
                         QVariant::fromValue<qulonglong>(4294967296ULL));
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);

    // QQuickItem 输入坐标是逻辑像素，pt 映射不接收 DPR。
    for (const qreal zoom : {0.5, 1.0, 2.0}) {
        canvas->setZoom(zoom);
        const QPointF zoomed = canvas->documentToContent(1, document_point);
        QCOMPARE(canvas->contentToDocument(1, zoomed), document_point);
    }

    QQmlComponent surface_component(&engine);
    surface_component.setData(QByteArrayLiteral("import QtQuick\n"
                                                "import Caret.Shell\n"
                                                "PageSurface { width: 500; height: 500 }\n"),
                              QUrl());
    QVERIFY2(surface_component.isReady(), qPrintable(surface_component.errorString()));
    QObject* surface = surface_component.create();
    QVERIFY(surface != nullptr);
    auto* bound_canvas =
        surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(bound_canvas != nullptr);
    QCOMPARE(bound_canvas->pageSizesPt().size(), 1);
    QCOMPARE(bound_canvas->pageSizesPt().front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(bound_canvas->pageColor(), QColor(Qt::white));
    QCOMPARE(bound_canvas->pageBorderColor(), QColor(QStringLiteral("#c9c9c9")));
    QCOMPARE(bound_canvas->caretColor(), QColor(QStringLiteral("#1c1c1c")));
    QCOMPARE(bound_canvas->caretWidthCentipixels(), 100);

    QQmlComponent render_component(&engine);
    render_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "DocumentCanvas {\n"
                          "    width: 200; height: 200\n"
                          "    pageColor: \"#ffffff\"\n"
                          "    pageBorderColor: \"#c8c8c8\"\n"
                          "    caretColor: \"#ff0000\"\n"
                          "    caretWidthCentipixels: 125\n"
                          "    blockEnvironmentHighlightColor: \"#fff2cc\"\n"
                          "    blockEnvironmentHighlightRadius: 4\n"
                          "    pageSizesPt: [Qt.size(60, 60), Qt.size(60, 60)]\n"
                          "    caretGeometry: ({ pageIndex: 0, xPt: 30, y0Pt: 20, y1Pt: 80 })\n"
                          "    pageMarginPt: 12; pageGapPt: 0\n"
                          "}\n"),
        QUrl());
    QVERIFY2(render_component.isReady(), qPrintable(render_component.errorString()));
    auto* rendered_canvas = qobject_cast<caret::DocumentCanvas*>(render_component.create());
    QVERIFY(rendered_canvas != nullptr);
    QCOMPARE(rendered_canvas->property("caretWidthCentipixels").toInt(), 125);
    QVERIFY(rendered_canvas->setProperty("blockEnvironmentHighlightRects",
                                         QVariantList{QVariantMap{{QStringLiteral("pageIndex"), 0},
                                                                  {QStringLiteral("x0Pt"), 10},
                                                                  {QStringLiteral("y0Pt"), 10},
                                                                  {QStringLiteral("x1Pt"), 50},
                                                                  {QStringLiteral("y1Pt"), 20}},
                                                      QVariantMap{{QStringLiteral("pageIndex"), 1},
                                                                  {QStringLiteral("x0Pt"), 10},
                                                                  {QStringLiteral("y0Pt"), 10},
                                                                  {QStringLiteral("x1Pt"), 50},
                                                                  {QStringLiteral("y1Pt"), 20}}}));
    QVERIFY(rendered_canvas->setProperty("blockEnvironmentHighlightColor",
                                         QColor(QStringLiteral("#fff2cc"))));
    constexpr qsizetype glyph_pixel_count = qsizetype{8} * 8;
    QByteArray glyph_pixels(glyph_pixel_count * 4, '\0');
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        glyph_pixels[pixel * 4 + (x < 4 ? 0 : 1)] = static_cast<char>(255);
        glyph_pixels[pixel * 4 + 3] = static_cast<char>(255);
    }
    const QVariantMap glyph_frame{
        {QStringLiteral("valid"), true},
        {QStringLiteral("rasterPxPerPt"), 96.0 / 72.0},
        {QStringLiteral("pageIndex"), 0},
        {QStringLiteral("images"),
         QVariantList{QVariantMap{{QStringLiteral("width"), 8},
                                  {QStringLiteral("height"), 8},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 8},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), glyph_pixels}},
                      QVariantMap{{QStringLiteral("width"), 0},
                                  {QStringLiteral("height"), 1},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), QByteArray{}}}}},
        {QStringLiteral("placements"),
         QVariantList{QVariantMap{{QStringLiteral("imageIndex"), 0},
                                  {QStringLiteral("xPt"), 20.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), true},
                                  {QStringLiteral("clipY0Pt"), 28.0},
                                  {QStringLiteral("clipY1Pt"), 31.0}},
                      QVariantMap{{QStringLiteral("imageIndex"), 1},
                                  {QStringLiteral("xPt"), 30.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), false}}}},
    };
    QVariantMap second_glyph_frame = glyph_frame;
    second_glyph_frame.insert(QStringLiteral("pageIndex"), 1);
    QVERIFY(
        rendered_canvas->setProperty("glyphFrames", QVariantList{glyph_frame, second_glyph_frame}));
    QQuickWindow render_window;
    render_window.setColor(Qt::green);
    render_window.resize(200, 200);
    rendered_canvas->setParent(&render_window);
    rendered_canvas->setParentItem(render_window.contentItem());
    render_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(render_window.isExposed(), 3000);
    rendered_canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY(rendered_canvas->hasActiveFocus());
    const auto count_color_pixels = [](const QImage& image, const QColor& color) {
        qsizetype count = 0;
        for (qsizetype y = 0; y < image.height(); ++y) {
            for (qsizetype x = 0; x < image.width(); ++x) {
                count += image.pixelColor(static_cast<int>(x), static_cast<int>(y)) == color;
            }
        }
        return count;
    };
    const QColor block_environment_highlight_color(QStringLiteral("#fff2cc"));
    QCOMPARE(rendered_canvas->property("blockEnvironmentHighlightRects").toList().size(), 2);
    QCOMPARE(rendered_canvas->property("blockEnvironmentHighlightColor").value<QColor>(),
             block_environment_highlight_color);
    QCOMPARE(rendered_canvas->property("blockEnvironmentHighlightRadius").toReal(), 4.0);
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) > 0,
        3000);
    const QImage highlight_frame = render_window.grabWindow();
    QVERIFY(!highlight_frame.isNull());
    QVERIFY(count_color_pixels(highlight_frame, block_environment_highlight_color) > 0);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{1, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) > 0,
        3000);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) > 0,
        3000);
    const auto pixel_at_logical_point = [&render_window](const QImage& image, qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(image.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(image.height()) / render_window.height();
        return image.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    const QPointF highlight_origin = rendered_canvas->documentToContent(0, QPointF(10.0, 10.0));
    const QPointF highlight_center = rendered_canvas->documentToContent(0, QPointF(30.0, 15.0));
    const QPointF next_page_highlight_center =
        rendered_canvas->documentToContent(1, QPointF(30.0, 15.0));
    QCOMPARE(pixel_at_logical_point(highlight_frame, highlight_center.x(), highlight_center.y()),
             block_environment_highlight_color);
    QCOMPARE(pixel_at_logical_point(highlight_frame, next_page_highlight_center.x(),
                                    next_page_highlight_center.y()),
             block_environment_highlight_color);
    const QColor rounded_highlight_edge = pixel_at_logical_point(
        highlight_frame, highlight_origin.x() + 1.0, highlight_origin.y() + 1.0);
    QVERIFY(rounded_highlight_edge.red() >= block_environment_highlight_color.red());
    QVERIFY(rounded_highlight_edge.green() >= block_environment_highlight_color.green());
    QVERIFY(rounded_highlight_edge.blue() >= block_environment_highlight_color.blue());
    QVERIFY(rounded_highlight_edge != block_environment_highlight_color);
    const QColor rounded_highlight_inner_edge = pixel_at_logical_point(
        highlight_frame, highlight_origin.x() + 4.0, highlight_origin.y() + 4.0);
    QVERIFY(rounded_highlight_inner_edge.red() >= block_environment_highlight_color.red());
    QVERIFY(rounded_highlight_inner_edge.green() >= block_environment_highlight_color.green());
    QVERIFY(rounded_highlight_inner_edge.blue() >= block_environment_highlight_color.blue());
    QVERIFY(rounded_highlight_inner_edge != block_environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{1, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) > 0,
        3000);
    const QImage second_page_frame = render_window.grabWindow();
    QCOMPARE(pixel_at_logical_point(second_page_frame, next_page_highlight_center.x(),
                                    next_page_highlight_center.y()),
             block_environment_highlight_color);
    QVERIFY(pixel_at_logical_point(second_page_frame, highlight_center.x(), highlight_center.y()) !=
            block_environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 1}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) > 0,
        3000);
    const QImage first_page_frame = render_window.grabWindow();
    QCOMPARE(pixel_at_logical_point(first_page_frame, highlight_center.x(), highlight_center.y()),
             block_environment_highlight_color);
    QVERIFY(pixel_at_logical_point(first_page_frame, next_page_highlight_center.x(),
                                   next_page_highlight_center.y()) !=
            block_environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 2}));
    QVERIFY(rendered_canvas->setProperty("blockEnvironmentHighlightRects", QVariantList{}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), block_environment_highlight_color) == 0,
        3000);
    const QImage frame = render_window.grabWindow();
    QVERIFY(!frame.isNull());
    QCOMPARE(pixel_at_logical_point(frame, 60.5, 50.0), QColor(QStringLiteral("#c8c8c8")));
    QCOMPARE(pixel_at_logical_point(frame, 90.0, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 88.5, 55.0), QColor(Qt::red));
    QCOMPARE(pixel_at_logical_point(frame, 92.5, 55.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(frame, 92.0, 51.0), QColor(Qt::white));
    QVERIFY(pixel_at_logical_point(frame, 101.0, 50.0) != QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 102.5, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 100.5, 95.5), QColor(QStringLiteral("#ff0000")));
    QCOMPARE(pixel_at_logical_point(frame, 101.0, 180.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(frame, 88.5, 134.5), QColor(Qt::red));

    const auto current_caret_pixel = [&render_window]() {
        const QImage current_frame = render_window.grabWindow();
        return current_frame.pixelColor(
            qFloor(100.5 * current_frame.width() / render_window.width()),
            qFloor(50.0 * current_frame.height() / render_window.height()));
    };
    const int cursor_flash_time = QGuiApplication::styleHints()->cursorFlashTime();
    if (cursor_flash_time > 0) {
        QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                                 cursor_flash_time + 500);
        QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                                  cursor_flash_time + 500);
    }

    QStyleHints* style_hints = QGuiApplication::styleHints();
    const int original_cursor_flash_time = style_hints->cursorFlashTime();
    [[maybe_unused]] const auto restore_cursor_flash_time =
        qScopeGuard([style_hints, original_cursor_flash_time]() {
            style_hints->setCursorFlashTime(original_cursor_flash_time);
        });
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    rendered_canvas->setProperty("caretBlinkEnabled", false);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    rendered_canvas->setProperty("caretBlinkEnabled", true);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    style_hints->setCursorFlashTime(0);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);

    rendered_canvas->setFocus(false);
    QTRY_VERIFY(!rendered_canvas->hasActiveFocus());
    const QImage unfocused_frame = render_window.grabWindow();
    QVERIFY(!unfocused_frame.isNull());
    QCOMPARE(unfocused_frame.pixelColor(qFloor(101.0 * frame.width() / render_window.width()),
                                        qFloor(50.0 * frame.height() / render_window.height())),
             QColor(Qt::white));

    QByteArray changed_glyph_pixels = glyph_pixels;
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        if (x < 4) {
            changed_glyph_pixels[pixel * 4 + 0] = '\0';
            changed_glyph_pixels[pixel * 4 + 2] = static_cast<char>(255);
        }
    }
    QVariantMap changed_second_image =
        second_glyph_frame.value(QStringLiteral("images")).toList().front().toMap();
    changed_second_image.insert(QStringLiteral("pixels"), changed_glyph_pixels);
    QVariantMap changed_second_frame = second_glyph_frame;
    const QVariantList changed_images{
        changed_second_image,
        second_glyph_frame.value(QStringLiteral("images")).toList().at(1),
    };
    changed_second_frame.insert(QStringLiteral("images"), changed_images);
    const QVariantList partially_changed_frames{glyph_frame, changed_second_frame};
    QVERIFY(rendered_canvas->setProperty("glyphFrames", partially_changed_frames));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QVariantMap fractional_width_image = changed_second_image;
    fractional_width_image.insert(QStringLiteral("width"), 8.5);
    QVariantMap malformed_second_frame = changed_second_frame;
    QVariantList malformed_images = changed_images;
    malformed_images[0] = fractional_width_image;
    malformed_second_frame.insert(QStringLiteral("images"), malformed_images);
    QVERIFY(rendered_canvas->setProperty("glyphFrames",
                                         QVariantList{glyph_frame, malformed_second_frame}));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QImage partial_update_frame;
    const auto partial_update_pixel = [&partial_update_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x =
            static_cast<qreal>(partial_update_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(partial_update_frame.height()) / render_window.height();
        return partial_update_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QTRY_VERIFY_WITH_TIMEOUT(([&]() {
                                 partial_update_frame = render_window.grabWindow();
                                 return !partial_update_frame.isNull() &&
                                        partial_update_pixel(88.5, 134.5) == QColor(Qt::blue);
                             }()),
                             3000);
    QCOMPARE(partial_update_pixel(88.5, 55.0), QColor(Qt::red));
    QCOMPARE(partial_update_pixel(88.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(240);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 240.0, 3000);
    const QImage recentered_frame = render_window.grabWindow();
    QVERIFY(!recentered_frame.isNull());
    const auto recentered_pixel = [&recentered_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(recentered_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(recentered_frame.height()) / render_window.height();
        return recentered_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(recentered_pixel(88.5, 55.0), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 55.0), QColor(Qt::red));
    QCOMPARE(recentered_pixel(88.5, 134.5), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(300);
    rendered_canvas->setProperty("zoom", 2.0);
    render_window.resize(300, 200);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 300.0, 3000);
    const QImage zoomed_frame = render_window.grabWindow();
    QVERIFY(!zoomed_frame.isNull());
    const auto zoomed_pixel = [&zoomed_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(zoomed_frame.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(zoomed_frame.height()) / render_window.height();
        return zoomed_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(zoomed_pixel(126.0, 109.0), QColor(Qt::red));
    QCOMPARE(zoomed_pixel(134.0, 109.0), QColor(Qt::green));
    QObject* scroller = surface->findChild<QObject*>(QStringLiteral("documentScroller"));
    QVERIFY(scroller != nullptr);
    bound_canvas->setZoom(2.0);
    QCoreApplication::processEvents();
    const qreal previous_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(previous_max_y > 0.0);
    scroller->setProperty("contentY", previous_max_y);
    QVERIFY(QMetaObject::invokeMethod(surface, "setZoomAtViewportCenter",
                                      Q_ARG(QVariant, QVariant(0.5))));
    QCoreApplication::processEvents();
    const qreal new_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(scroller->property("contentY").toReal() <= std::max(0.0, new_max_y));
    delete surface;
    delete view_model;
    delete root;
}
