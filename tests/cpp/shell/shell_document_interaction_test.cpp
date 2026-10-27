// Qt/QML 文档命中、输入法与编辑交互的集成测试。
#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "shell_load_test.hpp"
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
#include <cstddef>
#include <qcontainerfwd.h>
#include <string>
#include <string_view>
#include <utility>

#ifdef CARET_ENABLE_APP
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
    QSignalSpy editing_command_spy(canvas, &caret::DocumentCanvas::editingCommandRequested);
    QVERIFY(editing_command_spy.isValid());
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
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.split_paragraph"));
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

    QInputMethodEvent cancel_composition(QString(), {});
    QCoreApplication::sendEvent(canvas, &cancel_composition);
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->layoutRevision(), composition_revision + 3, 3000);
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
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("4"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts().size(), 1);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("5"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("6"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());

    QTest::keyClick(&window, Qt::Key_Return, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(), QStringLiteral("text.soft_break"));
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

    QTest::keyClick(&window, Qt::Key_C, Qt::ControlModifier);
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("\nabc"));
    QVERIFY(layout_view_model->setSelection(drag_start, drag_end)
                .value(QStringLiteral("valid"))
                .toBool());
    QTest::keyClick(&window, Qt::Key_C, Qt::ControlModifier);
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
            "    property var result: LayoutViewModel.insertText(\"x\")\n"
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
            "    property var pageLayouts: LayoutViewModel.pageLayouts\n"
            "    property var paragraphs: LayoutViewModel.paragraphLayouts\n"
            "    Component.onCompleted: {\n"
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
    const QVariantList page_layouts = probe->property("pageLayouts").toList();
    const QVariantList paragraphs = probe->property("paragraphs").toList();
    QVERIFY(pages.size() > 1);
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
#endif
